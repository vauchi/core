// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! YOLO-based QR code detector using ONNX Runtime.
//!
//! Runs a YOLOv8n-seg model (qrdet-n, 12.6 MB) to detect QR code
//! bounding boxes in camera frames. Detected regions are cropped and
//! fed to rqrr for decoding.

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
}

impl YoloDetector {
    /// Load a YOLO ONNX model from a file path.
    pub fn load(model_path: &Path) -> Result<Self, String> {
        let mut builder = ort::session::Session::builder().map_err(|e| format!("builder: {e}"))?;
        let session = builder
            .commit_from_file(model_path)
            .map_err(|e| format!("load: {e}"))?;
        let (h, w) = extract_dims(&session);
        Ok(Self {
            session,
            input_width: w,
            input_height: h,
        })
    }

    /// Load from embedded bytes.
    pub fn load_from_bytes(model_bytes: &[u8]) -> Result<Self, String> {
        let mut builder = ort::session::Session::builder().map_err(|e| format!("builder: {e}"))?;
        let session = builder
            .commit_from_memory(model_bytes)
            .map_err(|e| format!("load bytes: {e}"))?;
        let (h, w) = extract_dims(&session);
        Ok(Self {
            session,
            input_width: w,
            input_height: h,
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

        // Prepare NCHW float32 input: gray → resize → 3-channel
        let resized = image::imageops::resize(
            img,
            self.input_width,
            self.input_height,
            image::imageops::FilterType::Triangle,
        );

        let mut input_data = vec![0.0f32; 3 * h * w];
        for y in 0..h {
            for x in 0..w {
                let val = resized.get_pixel(x as u32, y as u32)[0] as f32 / 255.0;
                let idx = y * w + x;
                input_data[idx] = val;
                input_data[h * w + idx] = val;
                input_data[2 * h * w + idx] = val;
            }
        }

        // Create tensor from flat data + shape
        let input_tensor =
            ort::value::Tensor::from_array(([1usize, 3, h, w], input_data.into_boxed_slice()))
                .map_err(|e| format!("tensor: {e}"))?;

        // Run inference
        let outputs = self
            .session
            .run(ort::inputs![input_tensor])
            .map_err(|e| format!("run: {e}"))?;

        // Get first output tensor
        let output_val = outputs.values().next().ok_or("no output")?;
        let (shape, data) = output_val
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("extract: {e}"))?;

        // Parse shape — we need [1, features, candidates]
        // Shape implements Deref to [i64], so we can access elements
        let shape_vec: Vec<i64> = shape.iter().copied().collect();
        if shape_vec.len() != 3 {
            return Err(format!("unexpected shape: {shape_vec:?}"));
        }
        let num_features = shape_vec[1] as usize;
        let num_candidates = shape_vec[2] as usize;

        let sx = orig_w as f32 / self.input_width as f32;
        let sy = orig_h as f32 / self.input_height as f32;

        let mut dets = Vec::new();
        for i in 0..num_candidates {
            let cx = data[0 * num_candidates + i];
            let cy = data[1 * num_candidates + i];
            let bw = data[2 * num_candidates + i];
            let bh = data[3 * num_candidates + i];
            let conf = if num_features > 4 {
                data[4 * num_candidates + i]
            } else {
                0.0
            };
            if conf >= confidence_threshold {
                dets.push(QrDetection {
                    bbox: (cx * sx, cy * sy, bw * sx, bh * sy),
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
