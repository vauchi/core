// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt::Write;

use super::log_event::LogEvent;
use super::snapshot::SnapshotMetadata;
use super::tuner::{DeviceCapabilityProfile, Platform, TuningResult, rank_configs};

#[derive(Debug)]
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

fn write_footer(html: &mut String) -> Result<(), ReportError> {
    writeln!(html, "</body>")?;
    writeln!(html, "</html>")?;
    Ok(())
}
