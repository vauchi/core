// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Version compatibility types for the force-update mechanism.
//!
//! The relay and CDN serve version policies that clients compare against
//! [`APP_COMPAT_VERSION`] to determine whether an update is needed.
//! This module is pure logic — no network, no IO.

/// Current application compatibility version.
///
/// Bumped whenever a protocol-breaking change is deployed.
/// Clients compare this against the relay's minimum required version.
pub const APP_COMPAT_VERSION: u16 = 1;

/// A version policy received from the relay or CDN manifest.
///
/// - `min_version`: versions below this are **required** to update.
/// - `warn_version`: versions below this (but >= min) get a soft "update available" prompt.
/// - `grace_deadline`: optional unix timestamp (seconds) after which the hard block activates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionPolicy {
    pub min_version: u16,
    pub warn_version: u16,
    pub grace_deadline: Option<u64>,
}

/// Result of comparing the app's version against a [`VersionPolicy`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppUpdateStatus {
    /// App version meets or exceeds the warn threshold — no action needed.
    UpToDate,
    /// App version is between min and warn — soft prompt to update.
    UpdateAvailable,
    /// App version is below the minimum — must update.
    /// `grace_deadline` (unix seconds) indicates how long the user has before hard block.
    UpdateRequired { grace_deadline: Option<u64> },
}

impl VersionPolicy {
    /// Evaluate whether `app_version` needs an update according to this policy.
    ///
    /// `now_secs` is the current time as seconds since the UNIX epoch.
    /// Pass it explicitly so callers (and tests) control the clock.
    pub fn evaluate(&self, app_version: u16, now_secs: u64) -> AppUpdateStatus {
        if app_version >= self.warn_version {
            AppUpdateStatus::UpToDate
        } else if app_version >= self.min_version {
            AppUpdateStatus::UpdateAvailable
        } else {
            AppUpdateStatus::UpdateRequired {
                grace_deadline: self.active_grace_deadline(now_secs),
            }
        }
    }

    /// Parse a version policy from HTTP response headers.
    ///
    /// Missing or invalid headers default to 0 / None.
    pub fn from_headers(min: Option<&str>, warn: Option<&str>, deadline: Option<&str>) -> Self {
        Self {
            min_version: min.and_then(|v| v.parse().ok()).unwrap_or(0),
            warn_version: warn.and_then(|v| v.parse().ok()).unwrap_or(0),
            grace_deadline: deadline.and_then(|v| v.parse().ok()),
        }
    }

    /// Returns the grace deadline only if it is still in the future.
    ///
    /// A past deadline means grace has expired — callers should treat `None` as "hard block now".
    fn active_grace_deadline(&self, now_secs: u64) -> Option<u64> {
        self.grace_deadline.filter(|&d| d > now_secs)
    }

    /// Returns `true` if this policy carries no version constraints (both thresholds are 0).
    pub fn is_none_policy(&self) -> bool {
        self.min_version == 0 && self.warn_version == 0
    }

    /// Parse a version policy from a CDN JSON manifest.
    ///
    /// Expected shape:
    /// ```json
    /// {
    ///   "min_version": 2,
    ///   "warn_version": 4,
    ///   "grace_deadline": 1700000000    // or "2024-01-15T00:00:00Z" or null
    /// }
    /// ```
    ///
    /// `grace_deadline` accepts:
    /// - a JSON number (unix timestamp seconds),
    /// - a JSON string in ISO 8601 format (`YYYY-MM-DDThh:mm:ssZ`),
    /// - `null` or absent (treated as `None`).
    pub fn from_cdn_json(json: &str) -> Result<Self, String> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;

        let obj = value
            .as_object()
            .ok_or_else(|| "expected JSON object".to_string())?;

        let min_version = obj
            .get("min_version")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| "missing or invalid min_version".to_string())
            .and_then(|v| {
                u16::try_from(v).map_err(|_| format!("min_version out of u16 range: {v}"))
            })?;

        let warn_version = obj
            .get("warn_version")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| "missing or invalid warn_version".to_string())
            .and_then(|v| {
                u16::try_from(v).map_err(|_| format!("warn_version out of u16 range: {v}"))
            })?;

        let grace_deadline = match obj.get("grace_deadline") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::Number(n)) => Some(
                n.as_u64()
                    .ok_or_else(|| "grace_deadline: invalid number".to_string())?,
            ),
            Some(serde_json::Value::String(s)) => Some(parse_iso8601_to_unix(s)?),
            Some(_) => return Err("grace_deadline: expected number, string, or null".to_string()),
        };

        Ok(Self {
            min_version,
            warn_version,
            grace_deadline,
        })
    }
}

/// Minimal ISO 8601 parser for `YYYY-MM-DDThh:mm:ssZ` → unix timestamp (seconds).
///
/// Only supports UTC (`Z` suffix). This avoids pulling in `chrono` for a single parse site.
fn parse_iso8601_to_unix(s: &str) -> Result<u64, String> {
    // Expected: "2024-01-15T00:00:00Z" (20 chars)
    if !s.ends_with('Z') || s.len() != 20 {
        return Err(format!("unsupported ISO 8601 format: {s}"));
    }

    let b = s.as_bytes();

    // Validate separators: YYYY-MM-DDThh:mm:ssZ
    if b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return Err(format!("unsupported ISO 8601 format: {s}"));
    }

    let year = parse_segment(b, 0, 4, "year")?;
    let month = parse_segment(b, 5, 7, "month")?;
    let day = parse_segment(b, 8, 10, "day")?;
    let hour = parse_segment(b, 11, 13, "hour")?;
    let minute = parse_segment(b, 14, 16, "minute")?;
    let second = parse_segment(b, 17, 19, "second")?;

    if year < 1970 {
        return Err(format!("pre-1970 dates are not supported: {s}"));
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(format!("invalid date components in: {s}"));
    }
    if hour > 23 || minute > 59 || second > 60 {
        return Err(format!("invalid time components in: {s}"));
    }

    // Days from Unix epoch (1970-01-01) to the start of the given date.
    let days = days_since_epoch(year, month, day)?;
    let timestamp =
        days * 86400 + u64::from(hour) * 3600 + u64::from(minute) * 60 + u64::from(second);
    Ok(timestamp)
}

fn parse_segment(b: &[u8], start: usize, end: usize, label: &str) -> Result<u32, String> {
    let slice =
        std::str::from_utf8(&b[start..end]).map_err(|_| format!("invalid UTF-8 in {label}"))?;
    slice
        .parse::<u32>()
        .map_err(|_| format!("invalid {label}: {slice}"))
}

/// Days from 1970-01-01 to the given date using the proleptic Gregorian calendar.
///
/// Returns an error for dates before the Unix epoch (1970-01-01).
fn days_since_epoch(year: u32, month: u32, day: u32) -> Result<u64, String> {
    // Adjust so March = month 1 (simplifies leap year handling).
    let (y, m) = if month <= 2 {
        (year as i64 - 1, month as i64 + 9)
    } else {
        (year as i64, month as i64 - 3)
    };

    // Days within the year from the adjusted month.
    let day_of_year = (153 * m + 2) / 5 + day as i64 - 1;

    // Days from epoch year (adjusted epoch = 0000-03-01).
    let leap_days = y / 4 - y / 100 + y / 400;
    let days_from_zero = y * 365 + leap_days + day_of_year;

    // 719468 = days from 0000-03-01 to 1970-01-01 in the Gregorian calendar.
    u64::try_from(days_from_zero - 719_468)
        .map_err(|_| format!("date {year:04}-{month:02}-{day:02} is before Unix epoch"))
}

/// Format a unix timestamp (seconds) as `YYYY-MM-DD`.
///
/// Uses the proleptic Gregorian calendar, inverse of `days_since_epoch`.
pub fn unix_secs_to_date_string(timestamp: u64) -> String {
    let total_days = (timestamp / 86400) as i64;

    let adjusted = total_days + 719_468; // days from 0000-03-01
    let era = adjusted.div_euclid(146_097);
    let day_of_era = adjusted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146_096) / 365;
    let y = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{day:02}")
}
