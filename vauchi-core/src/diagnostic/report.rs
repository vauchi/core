// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt::Write;

use serde::{Deserialize, Serialize};

use super::log_event::LogEvent;
use super::snapshot::SnapshotMetadata;
use super::tuner::{DeviceCapabilityProfile, Platform, TuningResult, rank_configs};

/// Results from a single scanner backend in the multi-backend comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendBenchmark {
    /// Scanner backend name (e.g. "ML Kit", "ZXing", "rqrr (raw)", "rqrr (preprocessed)").
    pub backend_name: String,
    /// QR version tested (0 = mixed).
    pub qr_version: u32,
    /// Total frames processed.
    pub frames_total: u32,
    /// Frames successfully decoded.
    pub frames_decoded: u32,
    /// Decode rate (0.0–1.0).
    pub decode_rate: f32,
    /// Average decode latency in milliseconds.
    pub avg_latency_ms: f32,
    /// Average preprocessing time in microseconds (0 for platform-native).
    pub avg_preprocessing_us: u64,
    /// Frames skipped by sharpness gating (rqrr only).
    pub frames_skipped: u32,
}

/// Throughput test results for one scanner backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputBenchmark {
    /// Scanner backend name.
    pub backend_name: String,
    /// Beacon fps setting.
    pub beacon_fps: u32,
    /// Effective bytes per second successfully decoded.
    pub bytes_per_sec: f64,
    /// Frame loss rate (0.0–1.0).
    pub frame_loss_rate: f32,
    /// Total frames in sequence.
    pub total_frames: u32,
    /// Frames successfully decoded.
    pub decoded_frames: u32,
}

#[derive(Debug)]
#[non_exhaustive]
pub enum ReportError {
    FormatError(std::fmt::Error),
}

impl std::fmt::Display for ReportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReportError::FormatError(e) => write!(f, "format error: {}", e),
        }
    }
}

impl std::error::Error for ReportError {}

impl From<std::fmt::Error> for ReportError {
    fn from(e: std::fmt::Error) -> Self {
        ReportError::FormatError(e)
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Generate a self-contained HTML diagnostic report.
///
/// The report includes device info, a ranked config table, an SVG bar chart,
/// snapshot summaries, and a collapsible event log. It has no external
/// dependencies (no external JS, CSS, or URLs).
pub fn generate_html_report(
    profile: &DeviceCapabilityProfile,
    results: &[TuningResult],
    events: &[LogEvent],
    snapshots: &[SnapshotMetadata],
) -> Result<String, ReportError> {
    let mut html = String::with_capacity(8192);

    write_header(&mut html)?;
    write_device_section(&mut html, profile)?;

    if results.is_empty() {
        writeln!(html, "<section><p class=\"empty\">No results</p></section>")?;
    } else {
        let ranked = rank_configs(results);
        write_chart(&mut html, &ranked)?;
        write_config_table(&mut html, results, &ranked)?;
    }

    write_snapshot_section(&mut html, snapshots)?;
    write_event_log(&mut html, events)?;
    write_footer(&mut html)?;

    Ok(html)
}

fn write_header(html: &mut String) -> Result<(), ReportError> {
    write!(
        html,
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Vauchi QR Camera Tuner Report</title>
<style>
:root {{ --bg: #1a1a2e; --fg: #e0e0e0; --accent: #00d4aa; --muted: #888; --card: #16213e; --border: #0f3460; }}
body {{ background: var(--bg); color: var(--fg); font-family: system-ui, sans-serif; margin: 2rem; line-height: 1.6; }}
h1 {{ color: var(--accent); }}
h2 {{ color: var(--accent); border-bottom: 1px solid var(--border); padding-bottom: 0.3rem; }}
section {{ background: var(--card); border: 1px solid var(--border); border-radius: 8px; padding: 1.2rem; margin-bottom: 1.5rem; }}
table {{ border-collapse: collapse; width: 100%; }}
th, td {{ text-align: left; padding: 0.5rem 0.8rem; border-bottom: 1px solid var(--border); }}
th {{ color: var(--accent); }}
.badge {{ display: inline-block; background: var(--accent); color: var(--bg); padding: 0.2rem 0.6rem; border-radius: 4px; font-weight: bold; }}
.empty {{ color: var(--muted); font-style: italic; }}
details {{ margin-top: 1rem; }}
summary {{ cursor: pointer; color: var(--accent); font-weight: bold; }}
pre {{ background: var(--bg); padding: 1rem; border-radius: 4px; overflow-x: auto; font-size: 0.85rem; }}
svg {{ display: block; margin: 1rem 0; }}
</style>
</head>
<body>
<h1>QR Camera Tuner Report</h1>
"#
    )?;
    Ok(())
}

fn platform_name(platform: &Platform) -> &'static str {
    match platform {
        Platform::Android => "Android",
        Platform::Ios => "iOS",
    }
}

fn write_device_section(
    html: &mut String,
    profile: &DeviceCapabilityProfile,
) -> Result<(), ReportError> {
    writeln!(html, "<section>")?;
    writeln!(html, "<h2>Device</h2>")?;
    writeln!(html, "<table>")?;
    writeln!(
        html,
        "<tr><th>Model</th><td>{}</td></tr>",
        html_escape(&profile.device_model)
    )?;
    writeln!(
        html,
        "<tr><th>Platform</th><td>{}</td></tr>",
        platform_name(&profile.platform)
    )?;
    if let Some(ref hw) = profile.hardware_level {
        writeln!(
            html,
            "<tr><th>Hardware Level</th><td>{}</td></tr>",
            html_escape(hw)
        )?;
    }
    writeln!(
        html,
        "<tr><th>Max Resolution</th><td>{}x{}</td></tr>",
        profile.max_resolution.0, profile.max_resolution.1
    )?;
    if let Some((lo, hi)) = profile.iso_range {
        writeln!(html, "<tr><th>ISO Range</th><td>{}-{}</td></tr>", lo, hi)?;
    }
    if let Some((lo, hi)) = profile.exposure_ev_range {
        writeln!(html, "<tr><th>EV Range</th><td>{} to {}</td></tr>", lo, hi)?;
    }
    writeln!(html, "</table>")?;
    writeln!(html, "</section>")?;
    Ok(())
}

fn write_chart(html: &mut String, ranked: &[(u32, f32)]) -> Result<(), ReportError> {
    if ranked.is_empty() {
        return Ok(());
    }

    let max_score = ranked
        .iter()
        .map(|(_, s)| *s)
        .fold(f32::NEG_INFINITY, f32::max)
        .max(0.01);

    let bar_height = 28;
    let gap = 6;
    let chart_width = 500;
    let label_width = 80;
    let total_height = ranked.len() as u32 * (bar_height + gap) + gap;

    writeln!(html, "<section>")?;
    writeln!(html, "<h2>Ranked Configs</h2>")?;
    writeln!(
        html,
        "<svg width=\"{}\" height=\"{}\" role=\"img\" aria-label=\"Config score chart\">",
        chart_width + label_width + 10,
        total_height
    )?;

    for (i, (config_id, score)) in ranked.iter().enumerate() {
        let y = (i as u32) * (bar_height + gap) + gap;
        let bar_w = ((score / max_score) * chart_width as f32).max(1.0) as u32;
        let fill = if i == 0 { "#00d4aa" } else { "#0f3460" };

        writeln!(
            html,
            "<text x=\"0\" y=\"{}\" fill=\"#e0e0e0\" font-size=\"14\" dominant-baseline=\"middle\">Config {}</text>",
            y + bar_height / 2,
            config_id
        )?;
        writeln!(
            html,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" rx=\"4\"/>",
            label_width, y, bar_w, bar_height, fill
        )?;
        writeln!(
            html,
            "<text x=\"{}\" y=\"{}\" fill=\"#e0e0e0\" font-size=\"12\" dominant-baseline=\"middle\">{:.2}</text>",
            label_width + bar_w + 6,
            y + bar_height / 2,
            score
        )?;
    }

    writeln!(html, "</svg>")?;

    if let Some((best_id, best_score)) = ranked.first() {
        writeln!(
            html,
            "<p><span class=\"badge\">Best: Config {} (score {:.2})</span></p>",
            best_id, best_score
        )?;
    }
    writeln!(html, "</section>")?;
    Ok(())
}

fn write_config_table(
    html: &mut String,
    results: &[TuningResult],
    ranked: &[(u32, f32)],
) -> Result<(), ReportError> {
    let score_map: std::collections::HashMap<u32, f32> = ranked.iter().copied().collect();

    writeln!(html, "<section>")?;
    writeln!(html, "<h2>Config Details</h2>")?;
    writeln!(html, "<table>")?;
    writeln!(
        html,
        "<tr><th>ID</th><th>Score</th><th>Decode&nbsp;%</th><th>Latency&nbsp;ms</th><th>Jitter&nbsp;ms</th><th>ISO</th><th>EV</th><th>Thermal</th></tr>"
    )?;

    for r in results {
        let score = score_map.get(&r.camera_config_id).copied().unwrap_or(0.0);
        writeln!(
            html,
            "<tr><td>{}</td><td>{:.2}</td><td>{:.1}</td><td>{:.1}</td><td>{:.1}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            r.camera_config_id,
            score,
            r.decode_rate * 100.0,
            r.avg_latency_ms,
            r.jitter_ms,
            r.actual_iso.map_or("-".to_string(), |v| v.to_string()),
            r.actual_exposure_ev
                .map_or("-".to_string(), |v| v.to_string()),
            r.thermal_events,
        )?;
    }

    writeln!(html, "</table>")?;
    writeln!(html, "</section>")?;
    Ok(())
}

fn write_snapshot_section(
    html: &mut String,
    snapshots: &[SnapshotMetadata],
) -> Result<(), ReportError> {
    if snapshots.is_empty() {
        return Ok(());
    }

    writeln!(html, "<section>")?;
    writeln!(html, "<h2>Snapshots</h2>")?;
    writeln!(html, "<table>")?;
    writeln!(
        html,
        "<tr><th>Time&nbsp;ms</th><th>Config</th><th>Frame</th><th>Decoded</th><th>Latency</th><th>ISO</th><th>EV</th></tr>"
    )?;

    for s in snapshots {
        writeln!(
            html,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            s.timestamp_ms,
            s.config_id,
            s.frame_index,
            if s.decode_result { "yes" } else { "no" },
            s.decode_latency_ms
                .map_or("-".to_string(), |v| format!("{:.1}", v)),
            s.actual_iso.map_or("-".to_string(), |v| v.to_string()),
            s.actual_exposure_ev
                .map_or("-".to_string(), |v| v.to_string()),
        )?;
    }

    writeln!(html, "</table>")?;
    writeln!(html, "</section>")?;
    Ok(())
}

fn write_event_log(html: &mut String, events: &[LogEvent]) -> Result<(), ReportError> {
    if events.is_empty() {
        return Ok(());
    }

    let display_events: &[LogEvent] = if events.len() > 500 {
        &events[events.len() - 500..]
    } else {
        events
    };

    writeln!(html, "<section>")?;
    writeln!(html, "<h2>Event Log</h2>")?;
    writeln!(html, "<details>")?;
    writeln!(
        html,
        "<summary>Show events ({} of {} total)</summary>",
        display_events.len(),
        events.len()
    )?;
    writeln!(html, "<pre>")?;

    for event in display_events {
        if let Ok(json) = serde_json::to_string(event) {
            writeln!(html, "{}", html_escape(&json))?;
        }
    }

    writeln!(html, "</pre>")?;
    writeln!(html, "</details>")?;
    writeln!(html, "</section>")?;
    Ok(())
}

/// Generate a multi-backend comparison report for the QR scanner benchmark.
///
/// Compares decode rate, latency, and throughput across scanner backends
/// at each QR version tested. Produces side-by-side charts and tables.
pub fn generate_comparison_report(
    profile: &DeviceCapabilityProfile,
    benchmarks: &[BackendBenchmark],
    throughput: &[ThroughputBenchmark],
) -> Result<String, ReportError> {
    let mut html = String::with_capacity(16384);

    write_header(&mut html)?;
    // Override title
    html = html.replace(
        "<h1>QR Camera Tuner Report</h1>",
        "<h1>QR Scanner Backend Comparison</h1>",
    );
    write_device_section(&mut html, profile)?;

    if !benchmarks.is_empty() {
        write_comparison_chart(&mut html, benchmarks)?;
        write_comparison_table(&mut html, benchmarks)?;
    }

    if !throughput.is_empty() {
        write_throughput_section(&mut html, throughput)?;
    }

    write_footer(&mut html)?;
    Ok(html)
}

fn write_comparison_chart(
    html: &mut String,
    benchmarks: &[BackendBenchmark],
) -> Result<(), ReportError> {
    // Group by QR version
    let mut versions: Vec<u32> = benchmarks.iter().map(|b| b.qr_version).collect();
    versions.sort_unstable();
    versions.dedup();

    let backends: Vec<&str> = {
        let mut names: Vec<&str> = benchmarks.iter().map(|b| b.backend_name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        names
    };

    let colors = ["#00d4aa", "#e94560", "#0f3460", "#f5a623", "#9b59b6"];

    for &version in &versions {
        let version_data: Vec<&BackendBenchmark> = benchmarks
            .iter()
            .filter(|b| b.qr_version == version)
            .collect();

        if version_data.is_empty() {
            continue;
        }

        let version_label = if version == 0 {
            "Mixed".to_string()
        } else {
            format!("Version {version}")
        };

        writeln!(html, "<section>")?;
        writeln!(html, "<h2>Decode Rate — {version_label}</h2>")?;

        let bar_height = 32u32;
        let gap = 8u32;
        let chart_width = 400u32;
        let label_width = 160u32;
        let total_height = version_data.len() as u32 * (bar_height + gap) + gap;

        writeln!(
            html,
            "<svg width=\"{}\" height=\"{}\" role=\"img\" aria-label=\"Decode rate comparison for {}\">",
            chart_width + label_width + 80,
            total_height,
            version_label,
        )?;

        for (i, bench) in version_data.iter().enumerate() {
            let y = i as u32 * (bar_height + gap) + gap;
            let bar_w = (bench.decode_rate * chart_width as f32).max(1.0) as u32;
            let color_idx = backends
                .iter()
                .position(|&n| n == bench.backend_name)
                .unwrap_or(0);
            let fill = colors[color_idx % colors.len()];

            writeln!(
                html,
                "<text x=\"0\" y=\"{}\" fill=\"#e0e0e0\" font-size=\"13\" \
                 dominant-baseline=\"middle\">{}</text>",
                y + bar_height / 2,
                html_escape(&bench.backend_name),
            )?;
            writeln!(
                html,
                "<rect x=\"{label_width}\" y=\"{y}\" width=\"{bar_w}\" \
                 height=\"{bar_height}\" fill=\"{fill}\" rx=\"4\"/>",
            )?;
            writeln!(
                html,
                "<text x=\"{}\" y=\"{}\" fill=\"#e0e0e0\" font-size=\"12\" \
                 dominant-baseline=\"middle\">{:.0}% ({:.1}ms)</text>",
                label_width + bar_w + 6,
                y + bar_height / 2,
                bench.decode_rate * 100.0,
                bench.avg_latency_ms,
            )?;
        }

        writeln!(html, "</svg>")?;
        writeln!(html, "</section>")?;
    }

    Ok(())
}

fn write_comparison_table(
    html: &mut String,
    benchmarks: &[BackendBenchmark],
) -> Result<(), ReportError> {
    writeln!(html, "<section>")?;
    writeln!(html, "<h2>Backend Details</h2>")?;
    writeln!(html, "<table>")?;
    writeln!(
        html,
        "<tr><th>Backend</th><th>QR&nbsp;Ver</th><th>Decode&nbsp;%</th>\
         <th>Latency&nbsp;ms</th><th>Preproc&nbsp;&micro;s</th>\
         <th>Frames</th><th>Decoded</th><th>Skipped</th></tr>"
    )?;

    for b in benchmarks {
        let ver = if b.qr_version == 0 {
            "mixed".to_string()
        } else {
            b.qr_version.to_string()
        };
        writeln!(
            html,
            "<tr><td>{}</td><td>{}</td><td>{:.1}</td><td>{:.1}</td>\
             <td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            html_escape(&b.backend_name),
            ver,
            b.decode_rate * 100.0,
            b.avg_latency_ms,
            b.avg_preprocessing_us,
            b.frames_total,
            b.frames_decoded,
            b.frames_skipped,
        )?;
    }

    writeln!(html, "</table>")?;
    writeln!(html, "</section>")?;
    Ok(())
}

fn write_throughput_section(
    html: &mut String,
    throughput: &[ThroughputBenchmark],
) -> Result<(), ReportError> {
    writeln!(html, "<section>")?;
    writeln!(html, "<h2>Throughput Comparison</h2>")?;
    writeln!(html, "<table>")?;
    writeln!(
        html,
        "<tr><th>Backend</th><th>Beacon&nbsp;FPS</th>\
         <th>Bytes/s</th><th>Frame&nbsp;Loss</th>\
         <th>Decoded</th><th>Total</th></tr>"
    )?;

    for t in throughput {
        writeln!(
            html,
            "<tr><td>{}</td><td>{}</td><td>{:.0}</td><td>{:.1}%</td>\
             <td>{}</td><td>{}</td></tr>",
            html_escape(&t.backend_name),
            t.beacon_fps,
            t.bytes_per_sec,
            t.frame_loss_rate * 100.0,
            t.decoded_frames,
            t.total_frames,
        )?;
    }

    writeln!(html, "</table>")?;

    // SVG bar chart for throughput
    if !throughput.is_empty() {
        let max_bps = throughput
            .iter()
            .map(|t| t.bytes_per_sec)
            .fold(f64::NEG_INFINITY, f64::max)
            .max(1.0);

        let bar_height = 32u32;
        let gap = 8u32;
        let chart_width = 400u32;
        let label_width = 160u32;
        let total_height = throughput.len() as u32 * (bar_height + gap) + gap;

        writeln!(
            html,
            "<svg width=\"{}\" height=\"{total_height}\" role=\"img\" \
             aria-label=\"Throughput comparison\">",
            chart_width + label_width + 100,
        )?;

        let colors = ["#00d4aa", "#e94560", "#0f3460", "#f5a623"];
        for (i, t) in throughput.iter().enumerate() {
            let y = i as u32 * (bar_height + gap) + gap;
            let bar_w = ((t.bytes_per_sec / max_bps) * chart_width as f64).max(1.0) as u32;
            let fill = colors[i % colors.len()];
            let label = format!("{} @{}fps", t.backend_name, t.beacon_fps);

            writeln!(
                html,
                "<text x=\"0\" y=\"{}\" fill=\"#e0e0e0\" font-size=\"13\" \
                 dominant-baseline=\"middle\">{}</text>",
                y + bar_height / 2,
                html_escape(&label),
            )?;
            writeln!(
                html,
                "<rect x=\"{label_width}\" y=\"{y}\" width=\"{bar_w}\" \
                 height=\"{bar_height}\" fill=\"{fill}\" rx=\"4\"/>",
            )?;
            writeln!(
                html,
                "<text x=\"{}\" y=\"{}\" fill=\"#e0e0e0\" font-size=\"12\" \
                 dominant-baseline=\"middle\">{:.0} B/s</text>",
                label_width + bar_w + 6,
                y + bar_height / 2,
                t.bytes_per_sec,
            )?;
        }

        writeln!(html, "</svg>")?;
    }

    writeln!(html, "</section>")?;
    Ok(())
}

fn write_footer(html: &mut String) -> Result<(), ReportError> {
    writeln!(html, "</body>")?;
    writeln!(html, "</html>")?;
    Ok(())
}
