//! # Formatting Utilities
//!
//! This module contains reusable formatting functions for UI display.
//! It consolidates duplicate formatting logic from across the codebase.

use std::time::Duration;

/// Format a duration in seconds to a compact string format.
///
/// # Examples
///
/// - `65s` → "1m 5s"
/// - `3665s` → "1h 1m 5s"
///
/// Used for: uptime display, transfer time estimates
pub fn format_duration_compact(duration: Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// Format a duration in seconds to HH:MM:SS format.
///
/// # Examples
///
/// - `65s` → "01:05"
/// - `3665s` → "01:01:05"
///
/// Used for: connection duration, transfer time
pub fn format_duration_hms(duration: Duration) -> String {
    let secs = duration.as_secs();
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, mins, s)
    } else {
        format!("{:02}:{:02}", mins, s)
    }
}

/// Format a duration in seconds to HH:MM:SS format (from u64).
///
/// This is a convenience wrapper for `format_duration_hms`.
pub fn format_duration_hms_from_secs(secs: u64) -> String {
    format_duration_hms(Duration::from_secs(secs))
}
