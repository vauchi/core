// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! YOLO-based QR code detector using ONNX Runtime.
//!
//! Runs a YOLOv8n-seg model (qrdet-n, 12.6 MB, 320x320) to detect QR
//! code bounding boxes in camera frames. Detected regions are cropped
//! and fed to rqrr for decoding.
//!
//! Optimizations:
//! - NNAPI execution provider for hardware acceleration on Android
//! - Reusable pre-allocated input buffer (avoids per-frame allocation)
//! - Downscale to 320x320 via fast_image_resize (SIMD)
//! - Single-pass grayscale→RGB+normalize without intermediate image

use image::GrayImage;
use std::path::Path;

/// A detected QR code region in the image.
#[derive(Debug, Clone)]
pub struct QrDetection {
    /// Bounding box: (x_center, y_center, width, height) in pixels.
    pub bbox: (f32, f32, f32, f32),
    /// Detection confidence (0.0–1.0).
    pub confidence: f32,
}

/// YOLO QR detector wrapping an ONNX Runtime inference session.
pub struct YoloDetector {
    session: ort::session::Session,
    input_width: u32,
    input_height: u32,
    /// Pre-allocated input buffer to avoid per-frame allocation.
    input_buf: Vec<f32>,
}

impl YoloDetector {
    /// Load a YOLO ONNX model from a file path.
    ///
    /// Attempts to register NNAPI (Android hardware acceleration) as the
    /// preferred execution provider, falling back to CPU if unavailable.
    pub fn load(model_path: &Path) -> Result<Self, String> {
        let session = ort::session::Session::builder()
            .map_err(|e| format!("builder: {e}"))?
            .with_execution_providers([ort::ep::NNAPI::default().build()])
            .map_err(|e| format!("EP: {e}"))?
            .commit_from_file(model_path)
            .map_err(|e| format!("load: {e}"))?;
        Self::from_session(session)
    }

    /// Load from embedded bytes.
    pub fn load_from_bytes(model_bytes: &[u8]) -> Result<Self, String> {
        let session = ort::session::Session::builder()
            .map_err(|e| format!("builder: {e}"))?
            .with_execution_providers([ort::ep::NNAPI::default().build()])
            .map_err(|e| format!("EP: {e}"))?
            .commit_from_memory(model_bytes)
            .map_err(|e| format!("load bytes: {e}"))?;
        Self::from_session(session)
    }

    fn from_session(session: ort::session::Session) -> Result<Self, String> {
        let (h, w) = extract_dims(&session);
        let buf_size = 3 * (h as usize) * (w as usize);
        Ok(Self {
            session,
            input_width: w,
            input_height: h,
            input_buf: vec![0.0f32; buf_size],
        })
    }

    /// Detect QR codes in a grayscale image.
    pub fn detect(
        &mut self,
        img: &GrayImage,
        confidence_threshold: f32,
    ) -> Result<Vec<QrDetection>, String> {
        let (orig_w, orig_h) = img.dimensions();
        let w = self.input_width as usize;
        let h = self.input_height as usize;

        self.prepare_input_fast(img, w, h);

        let input_tensor =
            ort::value::TensorRef::from_array_view(([1usize, 3, h, w], &*self.input_buf))
                .map_err(|e| format!("tensor: {e}"))?;

        let outputs = self
            .session
            .run(ort::inputs![input_tensor])
            .map_err(|e| format!("run: {e}"))?;

        // Parse YOLOv8 output: [1, features, candidates]
        let output_val = outputs.values().next().ok_or("no output")?;
        let (shape, data) = output_val
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("extract: {e}"))?;

        let shape_vec: Vec<i64> = shape.iter().copied().collect();
        if shape_vec.len() != 3 {
            return Err(format!("unexpected shape: {shape_vec:?}"));
        }
        let num_features = shape_vec[1] as usize;
        let num_candidates = shape_vec[2] as usize;

        let sx = orig_w as f32 / self.input_width as f32;
        let sy = orig_h as f32 / self.input_height as f32;

        // Parse detections — early exit if confidence column is all low
        let mut dets = Vec::with_capacity(8);
        let conf_offset = 4 * num_candidates;
        for i in 0..num_candidates {
            let conf = if num_features > 4 {
                data[conf_offset + i]
            } else {
                0.0
            };
            if conf >= confidence_threshold {
                dets.push(QrDetection {
                    bbox: (
                        data[i] * sx,
                        data[num_candidates + i] * sy,
                        data[2 * num_candidates + i] * sx,
                        data[3 * num_candidates + i] * sy,
                    ),
                    confidence: conf,
                });
            }
        }

        dets.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(nms(dets, 0.5))
    }

    /// Fast input preparation: resize + gray→RGB + normalize into
    /// pre-allocated buffer. Uses fast_image_resize for SIMD downscale.
    fn prepare_input_fast(&mut self, img: &GrayImage, w: usize, h: usize) {
        use fast_image_resize::images::Image;
        use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};

        let (src_w, src_h) = img.dimensions();

        let resized_data = if src_w == w as u32 && src_h == h as u32 {
            img.as_raw().clone()
        } else {
            let src = Image::from_vec_u8(src_w, src_h, img.as_raw().clone(), PixelType::U8)
                .expect("src image");
            let mut dst = Image::new(w as u32, h as u32, PixelType::U8);
            let opts =
                ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Bilinear));
            let mut resizer = Resizer::new();
            resizer.resize(&src, &mut dst, &opts).expect("resize");
            dst.into_vec()
        };

        // Fill NCHW buffer: 3 identical channels, normalized to [0,1]
        // Layout: [R_plane | G_plane | B_plane], each h*w floats
        let plane_size = h * w;
        for (i, &pixel) in resized_data.iter().enumerate() {
            let val = pixel as f32 * (1.0 / 255.0);
            self.input_buf[i] = val;
            self.input_buf[plane_size + i] = val;
            self.input_buf[2 * plane_size + i] = val;
        }
    }
}

fn extract_dims(session: &ort::session::Session) -> (u32, u32) {
    let inputs = session.inputs();
    if inputs.is_empty() {
        return (320, 320);
    }
    match inputs[0].dtype() {
        ort::value::ValueType::Tensor { shape, .. } => {
            let dims: Vec<i64> = shape.iter().copied().collect();
            if dims.len() == 4 {
                (dims[2] as u32, dims[3] as u32)
            } else {
                (320, 320)
            }
        }
        _ => (320, 320),
    }
}

/// Crop a detected QR region with padding.
pub fn crop_detection(img: &GrayImage, detection: &QrDetection, padding_factor: f32) -> GrayImage {
    let (orig_w, orig_h) = img.dimensions();
    let (cx, cy, w, h) = detection.bbox;
    let pw = w * padding_factor;
    let ph = h * padding_factor;
    let x0 = ((cx - w / 2.0 - pw).max(0.0)) as u32;
    let y0 = ((cy - h / 2.0 - ph).max(0.0)) as u32;
    let x1 = ((cx + w / 2.0 + pw).min(orig_w as f32)) as u32;
    let y1 = ((cy + h / 2.0 + ph).min(orig_h as f32)) as u32;
    image::imageops::crop_imm(
        img,
        x0,
        y0,
        x1.saturating_sub(x0).max(1),
        y1.saturating_sub(y0).max(1),
    )
    .to_image()
}

fn nms(detections: Vec<QrDetection>, iou_threshold: f32) -> Vec<QrDetection> {
    let mut keep = Vec::new();
    for det in &detections {
        if !keep
            .iter()
            .any(|k: &QrDetection| iou(&det.bbox, &k.bbox) > iou_threshold)
        {
            keep.push(det.clone());
        }
    }
    keep
}

fn iou(a: &(f32, f32, f32, f32), b: &(f32, f32, f32, f32)) -> f32 {
    let inter_w =
        ((a.0 + a.2 / 2.0).min(b.0 + b.2 / 2.0) - (a.0 - a.2 / 2.0).max(b.0 - b.2 / 2.0)).max(0.0);
    let inter_h =
        ((a.1 + a.3 / 2.0).min(b.1 + b.3 / 2.0) - (a.1 - a.3 / 2.0).max(b.1 - b.3 / 2.0)).max(0.0);
    let intersection = inter_w * inter_h;
    let union = a.2 * a.3 + b.2 * b.3 - intersection;
    if union > 0.0 {
        intersection / union
    } else {
        0.0
    }
}
