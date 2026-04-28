// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Locale-aware relative-time formatter.
//!
//! Replaces the per-frontend relative-time helpers (iOS Swift's
//! `RelativeDateTimeFormatter`, Android `DateUtils.getRelativeTimeSpanString`)
//! with a single core-owned formatter so the typed
//! `MobileContactDetailViewState.added_time_display` field renders
//! identically across platforms (ADR-021/043 Humble UI). Closes G6 (a)
//! of the screenmodel-api-gaps follow-up.
//!
//! Output buckets — chosen to match Apple's `RelativeDateTimeFormatter`
//! `.named` style for short-term values and `.numeric` style after a
//! day:
//!
//! | Δ (seconds) | Output |
//! |---|---|
//! | < 60 | "Just now" |
//! | 60 – 3 599 | "1 minute ago" / "{count} minutes ago" |
//! | 3 600 – 86 399 | "1 hour ago" / "{count} hours ago" |
//! | 86 400 – 172 799 (~1d–2d) | "Yesterday" |
//! | 172 800 – 604 799 (~2d–7d) | "{count} days ago" |
//! | 604 800 – 2 591 999 (~7d–30d) | "1 week ago" / "{count} weeks ago" |
//! | 2 592 000 – 31 535 999 (~30d–365d) | "1 month ago" / "{count} months ago" |
//! | ≥ 31 536 000 (~1y) | "1 year ago" / "{count} years ago" |
//!
//! `now` and `then` are seconds-since-Unix-epoch. Negative deltas (i.e.
//! `then > now`, future timestamps) collapse to `Just now` — clock skew
//! never produces a confusing "in 3 minutes" string.

use crate::i18n::{Locale, get_string, get_string_with_args};

/// Approximate seconds-per-unit for the bucket boundaries above. Values
/// match Apple's `RelativeDateTimeFormatter` so cross-platform output
/// is consistent (a 30-day "month" + 365-day "year" — calendar-month
/// drift is acceptable for this UI affordance).
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 60 * SECONDS_PER_MINUTE;
const SECONDS_PER_DAY: u64 = 24 * SECONDS_PER_HOUR;
const SECONDS_PER_WEEK: u64 = 7 * SECONDS_PER_DAY;
const SECONDS_PER_MONTH: u64 = 30 * SECONDS_PER_DAY;
const SECONDS_PER_YEAR: u64 = 365 * SECONDS_PER_DAY;

/// The sentinel `i18n::get_string` returns when a key isn't found in
/// any locale (including the bundled English fallback). Surfacing
/// "Missing: <key>" to a user is always wrong for this affordance —
/// fall back to a hardcoded English template instead.
const MISSING_PREFIX: &str = "Missing:";

/// Format a relative timestamp using locale-aware strings.
///
/// `now` and `then` are seconds-since-Unix-epoch. Returns the localized
/// string for the bucket containing `now − then`. Future timestamps
/// (`then > now`) collapse to "Just now" — see module docs.
///
/// **Resilient lookup:** when the locale store doesn't have the
/// `time.relative.*` keys (e.g. CI builds against a stale bundled
/// English fallback whose JSON pre-dates these keys), the formatter
/// falls back to a hardcoded English template instead of returning
/// `i18n`'s `"Missing: <key>"` sentinel. Frontend never sees the
/// sentinel for this surface.
pub fn format_relative_time(now: u64, then: u64, locale: Locale) -> String {
    let delta = now.saturating_sub(then);

    if delta < SECONDS_PER_MINUTE {
        return localize_or_default(locale, "time.relative.just_now", "Just now");
    }
    if delta < SECONDS_PER_HOUR {
        let count = delta / SECONDS_PER_MINUTE;
        return scaled(
            locale,
            count,
            "minute_ago",
            "minutes_ago",
            "minute",
            "minutes",
        );
    }
    if delta < SECONDS_PER_DAY {
        let count = delta / SECONDS_PER_HOUR;
        return scaled(locale, count, "hour_ago", "hours_ago", "hour", "hours");
    }
    // 1 day exactly — use the calendar shortcut.
    if delta < 2 * SECONDS_PER_DAY {
        return localize_or_default(locale, "time.relative.yesterday", "Yesterday");
    }
    if delta < SECONDS_PER_WEEK {
        let count = delta / SECONDS_PER_DAY;
        // Always plural — "Yesterday" already covered the singular.
        return localize_with_count_or_default(locale, "time.relative.days_ago", count, "days");
    }
    if delta < SECONDS_PER_MONTH {
        let count = delta / SECONDS_PER_WEEK;
        return scaled(locale, count, "week_ago", "weeks_ago", "week", "weeks");
    }
    if delta < SECONDS_PER_YEAR {
        let count = delta / SECONDS_PER_MONTH;
        return scaled(locale, count, "month_ago", "months_ago", "month", "months");
    }
    let count = delta / SECONDS_PER_YEAR;
    scaled(locale, count, "year_ago", "years_ago", "year", "years")
}

/// Resolve the singular vs plural i18n key based on `count`. Falls
/// back to a hardcoded English template if the locale lookup misses.
///
/// `singular_key` / `plural_key` are the bare suffixes (e.g.
/// `"minute_ago"` / `"minutes_ago"`); `singular_unit` / `plural_unit`
/// are the English unit names used in the fallback template.
fn scaled(
    locale: Locale,
    count: u64,
    singular_key: &str,
    plural_key: &str,
    singular_unit: &str,
    plural_unit: &str,
) -> String {
    if count == 1 {
        // Singular keys still use {count} so the formatter is uniform —
        // e.g. English "1 minute ago" embeds "1" via the placeholder.
        let key = format!("time.relative.{singular_key}");
        let value = get_string_with_args(locale, &key, &[("count", "1")]);
        if value.starts_with(MISSING_PREFIX) {
            format!("1 {singular_unit} ago")
        } else {
            value
        }
    } else {
        localize_with_count_or_default(
            locale,
            &format!("time.relative.{plural_key}"),
            count,
            plural_unit,
        )
    }
}

/// Look up `key` in the locale store; if missing, return the
/// `default_en` string verbatim.
fn localize_or_default(locale: Locale, key: &str, default_en: &str) -> String {
    let value = get_string(locale, key);
    if value.starts_with(MISSING_PREFIX) {
        default_en.to_string()
    } else {
        value
    }
}

/// Look up a `{count}`-templated `key` in the locale store; if missing,
/// fall back to `"{count} {plural_unit} ago"`.
fn localize_with_count_or_default(
    locale: Locale,
    key: &str,
    count: u64,
    plural_unit: &str,
) -> String {
    let count_str = count.to_string();
    let value = get_string_with_args(locale, key, &[("count", &count_str)]);
    if value.starts_with(MISSING_PREFIX) {
        format!("{count_str} {plural_unit} ago")
    } else {
        value
    }
}

// INLINE_TEST_REQUIRED: exercises bucket boundaries + locale fallback.
#[cfg(test)]
mod tests {
    use super::*;

    /// Reference timestamp: 2026-01-01T00:00:00Z. Picked far enough into
    /// the future of any reasonable test fixture timestamp that
    /// `now.saturating_sub(then)` cannot underflow into 0 by accident.
    const NOW: u64 = 1_767_225_600;

    fn at(secs_ago: u64) -> u64 {
        NOW - secs_ago
    }

    // @internal
    #[test]
    fn just_now_for_under_one_minute() {
        assert_eq!(
            format_relative_time(NOW, at(0), Locale::English),
            "Just now",
        );
        assert_eq!(
            format_relative_time(NOW, at(59), Locale::English),
            "Just now",
        );
    }

    // @internal
    #[test]
    fn future_timestamps_collapse_to_just_now() {
        // Clock skew on the calling device must not produce "in 5 minutes" —
        // saturating subtraction keeps delta == 0.
        assert_eq!(
            format_relative_time(NOW, NOW + 300, Locale::English),
            "Just now",
        );
    }

    // @internal
    #[test]
    fn singular_minute_at_boundary() {
        assert_eq!(
            format_relative_time(NOW, at(60), Locale::English),
            "1 minute ago",
        );
    }

    // @internal
    #[test]
    fn plural_minutes() {
        assert_eq!(
            format_relative_time(NOW, at(120), Locale::English),
            "2 minutes ago",
        );
        assert_eq!(
            format_relative_time(NOW, at(59 * 60), Locale::English),
            "59 minutes ago",
        );
    }

    // @internal
    #[test]
    fn singular_hour_at_boundary() {
        assert_eq!(
            format_relative_time(NOW, at(3600), Locale::English),
            "1 hour ago",
        );
    }

    // @internal
    #[test]
    fn plural_hours() {
        assert_eq!(
            format_relative_time(NOW, at(2 * 3600), Locale::English),
            "2 hours ago",
        );
        assert_eq!(
            format_relative_time(NOW, at(23 * 3600), Locale::English),
            "23 hours ago",
        );
    }

    // @internal
    #[test]
    fn yesterday_for_one_day() {
        assert_eq!(
            format_relative_time(NOW, at(SECONDS_PER_DAY), Locale::English),
            "Yesterday",
        );
        // 1 day + 1 hour still renders as Yesterday.
        assert_eq!(
            format_relative_time(NOW, at(SECONDS_PER_DAY + 3600), Locale::English),
            "Yesterday",
        );
    }

    // @internal
    #[test]
    fn plural_days_after_yesterday() {
        assert_eq!(
            format_relative_time(NOW, at(2 * SECONDS_PER_DAY), Locale::English),
            "2 days ago",
        );
        assert_eq!(
            format_relative_time(NOW, at(6 * SECONDS_PER_DAY), Locale::English),
            "6 days ago",
        );
    }

    // @internal
    #[test]
    fn weeks_with_singular_and_plural() {
        assert_eq!(
            format_relative_time(NOW, at(SECONDS_PER_WEEK), Locale::English),
            "1 week ago",
        );
        assert_eq!(
            format_relative_time(NOW, at(3 * SECONDS_PER_WEEK), Locale::English),
            "3 weeks ago",
        );
    }

    // @internal
    #[test]
    fn months_with_singular_and_plural() {
        assert_eq!(
            format_relative_time(NOW, at(SECONDS_PER_MONTH), Locale::English),
            "1 month ago",
        );
        assert_eq!(
            format_relative_time(NOW, at(6 * SECONDS_PER_MONTH), Locale::English),
            "6 months ago",
        );
    }

    // @internal
    #[test]
    fn years_with_singular_and_plural() {
        assert_eq!(
            format_relative_time(NOW, at(SECONDS_PER_YEAR), Locale::English),
            "1 year ago",
        );
        assert_eq!(
            format_relative_time(NOW, at(5 * SECONDS_PER_YEAR), Locale::English),
            "5 years ago",
        );
    }

    // @internal
    #[test]
    fn locale_fallback_to_english_when_string_missing() {
        // No locale store has been initialised in this test process, so
        // every `get_string` call hits the hardcoded English fallback.
        // Ask for German — the formatter must still produce the English
        // bucket string rather than the "Missing: ..." sentinel.
        let out = format_relative_time(NOW, at(120), Locale::German);
        assert!(
            !out.starts_with("Missing:"),
            "German fallback must not surface the Missing sentinel, got {out:?}",
        );
    }
}
