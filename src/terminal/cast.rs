//! # Asciicast v2 recording: capture a terminal session as a text event stream
//!
//! Records PTY output as **asciicast v2** (`.cast`, NDJSON): a header line plus
//! `[elapsed_secs, "o", "data"]` output events, the format used by asciinema.
//! Files are tiny, infinitely sharp on replay, and the text stays searchable /
//! copyable — unlike a screen video.
//!
//! The recorder is a side-channel: reader threads lock the shared
//! `Option<CastRecorder>` slot and write each raw output chunk with a
//! timestamp. Nothing is ever recorded under the grid lock, and the buffer is
//! flushed lazily (≥1s since last flush) plus on Drop, so a crash loses at most
//! ~1s of tail.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Reject recordings larger than this when loading for replay (100 MB).
pub const MAX_CAST_BYTES: u64 = 100 * 1024 * 1024;

/// Header line of an asciicast v2 file.
#[derive(Debug, Clone, PartialEq)]
pub struct CastHeader {
    pub version: u32,
    pub width: u16,
    pub height: u16,
    /// Unix timestamp of recording start (seconds); 0 when absent.
    pub timestamp: i64,
    /// What was recorded (e.g. "user@host", "zsh") — optional per the v2
    /// spec, so old files without it still parse (the hand-rolled parser
    /// maps an absent field to `None`).
    pub title: Option<String>,
    /// Which host the session ran on ("localhost" or "user@host:port") — a
    /// Portal-specific extra header field (ignored by other asciicast tools),
    /// so the replay library can group/filter recordings by host.
    pub portal_host: Option<String>,
}

/// The kind of a recorded event. Input ("i") is reserved for a future opt-in
/// flag — recording blind-typed stdin would write passwords to disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Output,
    Resize,
}

/// One recorded event: a timestamp, a kind, and its payload.
#[derive(Debug, Clone, PartialEq)]
pub struct CastEvent {
    /// Seconds since recording start.
    pub time: f64,
    pub kind: EventKind,
    /// Output text (as recorded), or `"COLSxROWS"` for resize events.
    pub data: String,
}

/// A fully parsed recording, ready for replay.
#[derive(Debug, Clone, PartialEq)]
pub struct CastRecording {
    pub header: CastHeader,
    pub events: Vec<CastEvent>,
    /// Time of the last event (0.0 for empty recordings).
    pub duration: f64,
}

/// A live recorder writing asciicast v2 to a `.cast` file.
pub struct CastRecorder {
    writer: BufWriter<File>,
    start: Instant,
    last_flush: Instant,
    failed: bool,
    path: PathBuf,
}

impl CastRecorder {
    /// Create the file (with parent dirs) and write the v2 header line.
    /// `title` records what the session is (e.g. "user@host"), and `host`
    /// which machine it ran on ("localhost", "user@host:port") — so a
    /// recording can be identified and grouped from the file alone.
    pub fn create(
        path: &Path,
        width: u16,
        height: u16,
        title: Option<String>,
        portal_host: Option<String>,
    ) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        let mut header = serde_json::json!({
            "version": 2,
            "width": width,
            "height": height,
            "timestamp": chrono::Utc::now().timestamp(),
        });
        if let Some(title) = title {
            header["title"] = serde_json::json!(title);
        }
        if let Some(host) = portal_host {
            header["portal_host"] = serde_json::json!(host);
        }
        writeln!(writer, "{header}")?;
        Ok(Self {
            writer,
            start: Instant::now(),
            last_flush: Instant::now(),
            failed: false,
            path: path.to_path_buf(),
        })
    }

    /// Record one chunk of terminal output as an `"o"` event.
    pub fn output(&mut self, data: &[u8]) {
        let text = String::from_utf8_lossy(data);
        self.write_event("o", &text);
    }

    /// Record a terminal resize as an `"r"` event (`"COLSxROWS"`).
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.write_event("r", &format!("{cols}x{rows}"));
    }

    fn write_event(&mut self, kind: &str, data: &str) {
        if self.failed {
            return;
        }
        let event = serde_json::json!([self.start.elapsed().as_secs_f64(), kind, data]);
        if writeln!(self.writer, "{event}").is_err() {
            // A half-failed recording is not worth UI noise: stop writing and
            // log once; Drop still flushes what made it to the buffer.
            self.failed = true;
            log::warn!("cast recording write failed; stopping capture: {}", self.path.display());
        }
        self.flush_if_due();
    }

    /// Flush when at least a second has passed since the last flush, so a
    /// crash loses at most ~1s of tail. Only fires on activity (an event was
    /// just written), so idle sessions cause no I/O.
    fn flush_if_due(&mut self) {
        if self.last_flush.elapsed() >= std::time::Duration::from_secs(1) {
            let _ = self.writer.flush();
            self.last_flush = Instant::now();
        }
    }

    /// Flush and return the recording's path. Consumes the recorder.
    pub fn finish(mut self) -> std::io::Result<PathBuf> {
        self.writer.flush()?;
        Ok(self.path.clone())
    }
}

impl Drop for CastRecorder {
    fn drop(&mut self) {
        // Best-effort flush on drop (session closed, tab closed, reconnect).
        let _ = self.writer.flush();
    }
}

/// Directory where recordings are stored — also the target of the replay
/// library's "Open Folder" action and the retention sweep.
pub fn recordings_dir() -> PathBuf {
    crate::config::config_dir().join("recordings")
}

/// Default location for new recordings:
/// `<config_dir>/portal/recordings/portal-<label>-YYYYMMDD-HHMMSS.cast`.
/// `label` identifies the session ("zsh", "myhost") so the filename alone
/// says what was recorded. When the name exists, `-2` is appended once.
pub fn new_recording_path(label: &str) -> std::io::Result<PathBuf> {
    let dir = recordings_dir();
    std::fs::create_dir_all(&dir)?;
    // Filesystem-safe: keep [a-z0-9], collapse everything else to '-'.
    let mut safe = String::new();
    for c in label.chars() {
        if c.is_ascii_alphanumeric() {
            safe.push(c.to_ascii_lowercase());
        } else if !safe.ends_with('-') && !safe.is_empty() {
            safe.push('-');
        }
    }
    let safe = safe.trim_matches('-');
    let label = if safe.is_empty() { "session" } else { safe };
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let path = dir.join(format!("portal-{label}-{stamp}.cast"));
    if path.exists() {
        Ok(dir.join(format!("portal-{label}-{stamp}-2.cast")))
    } else {
        Ok(path)
    }
}

/// Read just the header line of a `.cast` file (for listing without a full
/// parse). `None` when the file is unreadable or has no valid header.
pub fn peek_header(path: &Path) -> Option<CastHeader> {
    let file = File::open(path).ok()?;
    let mut line = String::new();
    std::io::BufRead::read_line(&mut std::io::BufReader::new(file), &mut line).ok()?;
    parse_header(&line).ok()
}

/// Rewrite a recording's header `title` in place (library rename). The file
/// is NDJSON: only line 1 changes, all events are preserved byte-for-byte.
pub fn set_header_title(path: &Path, title: &str) -> std::io::Result<()> {
    let text = std::fs::read_to_string(path)?;
    let mut lines = text.lines();
    let Some(header_line) = lines.next() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "empty recording file",
        ));
    };
    let mut value: serde_json::Value = serde_json::from_str(header_line)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    value["title"] = serde_json::json!(title);
    let mut out = String::with_capacity(text.len() + title.len() + 16);
    out.push_str(&value.to_string());
    out.push('\n');
    // Preserve everything after the old header (dropping its line terminator,
    // which we just wrote back ourselves).
    out.push_str(text[header_line.len()..].trim_start_matches(['\r', '\n']));
    std::fs::write(path, out)
}

/// Delete recordings per retention policy: drop files older than `days`
/// (0 = keep forever), then — if the total still exceeds `max_total_mb`
/// (0 = unlimited) — delete oldest files first until it fits. Returns the
/// deleted paths so callers can log or report. Disabled entirely when both
/// limits are 0 (no directory scan happens).
pub fn enforce_retention(dir: &Path, days: u32, max_total_mb: u32) -> Vec<PathBuf> {
    if days == 0 && max_total_mb == 0 {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("cast") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let modified = meta
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        files.push((modified, meta.len(), path));
    }
    if files.is_empty() {
        return Vec::new();
    }

    let mut deleted = Vec::new();
    // Age pass: everything older than the window goes.
    if days > 0 {
        let cutoff =
            std::time::SystemTime::now() - std::time::Duration::from_secs(u64::from(days) * 86_400);
        files.retain(|(modified, _, path)| {
            if *modified < cutoff && std::fs::remove_file(path).is_ok() {
                deleted.push(path.clone());
                return false;
            }
            true
        });
    }
    // Size-budget pass: oldest first until under the cap.
    if max_total_mb > 0 {
        let cap = u64::from(max_total_mb) * 1024 * 1024;
        files.sort_by_key(|(modified, _, _)| *modified);
        let mut total: u64 = files.iter().map(|(_, size, _)| size).sum();
        for (_, size, path) in files {
            if total <= cap {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(size);
                deleted.push(path);
            }
        }
    }
    deleted
}

/// Parse a `.cast` file for replay. Malformed event lines are skipped
/// silently; a missing/invalid header is a hard error.
pub fn parse_cast(path: &Path) -> Result<CastRecording, String> {
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if meta.len() > MAX_CAST_BYTES {
        return Err(format!(
            "recording exceeds {} MB limit",
            MAX_CAST_BYTES / 1024 / 1024
        ));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let mut lines = text.lines();
    let header_line = lines
        .next()
        .ok_or_else(|| "empty recording file".to_string())?;
    let header = parse_header(header_line)?;

    let mut events = Vec::new();
    for line in lines {
        let Some(event) = parse_event_line(line) else { continue };
        events.push(event);
    }
    let duration = events.last().map(|e| e.time).unwrap_or(0.0);
    Ok(CastRecording { header, events, duration })
}

fn parse_header(line: &str) -> Result<CastHeader, String> {
    let value: serde_json::Value =
        serde_json::from_str(line).map_err(|e| format!("invalid header: {e}"))?;
    let width = value
        .get("width")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "header missing width".to_string())?;
    let height = value
        .get("height")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "header missing height".to_string())?;
    Ok(CastHeader {
        version: value.get("version").and_then(|v| v.as_u64()).unwrap_or(2) as u32,
        width: width as u16,
        height: height as u16,
        timestamp: value.get("timestamp").and_then(|v| v.as_i64()).unwrap_or(0),
        title: value
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        portal_host: value
            .get("portal_host")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    })
}

fn parse_event_line(line: &str) -> Option<CastEvent> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let arr = value.as_array()?;
    if arr.len() != 3 {
        return None;
    }
    let time = arr[0].as_f64()?;
    let kind = match arr[1].as_str()? {
        "o" => EventKind::Output,
        "r" => EventKind::Resize,
        // Unknown kinds ("i", markers…) are skipped, not fatal.
        _ => return None,
    };
    let data = arr[2].as_str()?.to_string();
    Some(CastEvent { time, kind, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a small recording via the real recorder and parse it back.
    fn record_and_parse(data_events: &[&str], resize: Option<(u16, u16)>) -> CastRecording {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.cast");
        {
            let mut rec = CastRecorder::create(
                &path,
                80,
                24,
                Some("tester@host".into()),
                Some("localhost".into()),
            )
            .unwrap();
            for d in data_events {
                rec.output(d.as_bytes());
            }
            if let Some((cols, rows)) = resize {
                rec.resize(cols, rows);
            }
            rec.finish().unwrap();
        }
        parse_cast(&path).unwrap()
    }

    #[test]
    fn round_trip_serialize_parse() {
        let rec = record_and_parse(
            &["hello", "\u{1b}[31mred\u{1b}[0m", "中文"],
            Some((100, 40)),
        );
        assert_eq!(rec.header.version, 2);
        assert_eq!(rec.header.width, 80);
        assert_eq!(rec.header.height, 24);
        assert_eq!(rec.header.title.as_deref(), Some("tester@host"));
        assert_eq!(rec.header.portal_host.as_deref(), Some("localhost"));
        // 3 output + 1 resize
        assert_eq!(rec.events.len(), 4);
        // Times are non-decreasing and non-negative.
        let mut prev = 0.0;
        for e in &rec.events {
            assert!(e.time >= prev);
            prev = e.time;
        }
        assert_eq!(rec.events[0].data, "hello");
        assert_eq!(rec.events[1].data, "\u{1b}[31mred\u{1b}[0m");
        assert_eq!(rec.events[2].data, "中文");
        assert_eq!(rec.events[2].kind, EventKind::Output);
        assert_eq!(rec.events[3].kind, EventKind::Resize);
        assert_eq!(rec.events[3].data, "100x40");
        assert_eq!(rec.duration, rec.events[3].time);
    }

    #[test]
    fn json_escaping_round_trip() {
        let nasty = "quote\" back\\slash\nnewline\u{1b}[2J\u{0007}bell";
        let rec = record_and_parse(&[nasty], None);
        assert_eq!(rec.events[0].data, nasty);
    }

    #[test]
    fn malformed_lines_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.cast");
        std::fs::write(
            &path,
            concat!(
                "{\"version\":2,\"width\":80,\"height\":24}\n",
                "not json at all\n",
                "[0.5,\"o\",\"kept\"]\n",
                "{\"also\":\"not an event\"}\n",
                "[1.0,\"x\",\"unknown kind\"]\n",
                "[1.5,\"o\",\"kept too\"]\n",
            ),
        )
        .unwrap();
        let rec = parse_cast(&path).unwrap();
        assert_eq!(rec.events.len(), 2);
        assert_eq!(rec.events[0].data, "kept");
        assert_eq!(rec.events[1].data, "kept too");
    }

    #[test]
    fn garbage_first_line_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.cast");
        std::fs::write(&path, "garbage\n[0.0,\"o\",\"x\"]\n").unwrap();
        assert!(parse_cast(&path).is_err());
    }

    #[test]
    fn missing_header_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.cast");
        std::fs::write(&path, "").unwrap();
        assert!(parse_cast(&path).is_err());
    }

    #[test]
    fn header_timestamp_defaults_to_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ts.cast");
        std::fs::write(
            &path,
            "{\"version\":2,\"width\":10,\"height\":5}\n[0.0,\"o\",\"a\"]\n",
        )
        .unwrap();
        let rec = parse_cast(&path).unwrap();
        assert_eq!(rec.header.timestamp, 0);
    }

    #[test]
    fn header_title_is_optional() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("titled.cast");
        std::fs::write(
            &path,
            "{\"version\":2,\"width\":10,\"height\":5,\"title\":\"u@h\"}\n[0.0,\"o\",\"a\"]\n",
        )
        .unwrap();
        assert_eq!(parse_cast(&path).unwrap().header.title.as_deref(), Some("u@h"));

        // Old/external files without a title still parse, as None.
        let path = dir.path().join("untitled.cast");
        std::fs::write(
            &path,
            "{\"version\":2,\"width\":10,\"height\":5}\n[0.0,\"o\",\"a\"]\n",
        )
        .unwrap();
        assert_eq!(parse_cast(&path).unwrap().header.title, None);
    }

    #[test]
    fn peek_header_reads_title_without_full_parse() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("peek.cast");
        std::fs::write(
            &path,
            "{\"version\":2,\"width\":10,\"height\":5,\"title\":\"u@h\"}\n[0.0,\"o\",\"a\"]\n",
        )
        .unwrap();
        let header = peek_header(&path).unwrap();
        assert_eq!(header.title.as_deref(), Some("u@h"));
        assert_eq!(header.width, 10);
    }

    #[test]
    fn header_portal_host_is_optional() {
        let dir = tempfile::tempdir().unwrap();

        // Written by Portal: carries portal_host.
        let path = dir.path().join("hosted.cast");
        std::fs::write(
            &path,
            "{\"version\":2,\"width\":10,\"height\":5,\"portal_host\":\"u@h:22\"}\n[0.0,\"o\",\"a\"]\n",
        )
        .unwrap();
        assert_eq!(
            parse_cast(&path).unwrap().header.portal_host.as_deref(),
            Some("u@h:22")
        );

        // External/old files (asciinema itself) don't have it — still parse.
        let path = dir.path().join("plain.cast");
        std::fs::write(
            &path,
            "{\"version\":2,\"width\":10,\"height\":5}\n[0.0,\"o\",\"a\"]\n",
        )
        .unwrap();
        assert_eq!(parse_cast(&path).unwrap().header.portal_host, None);
    }

    #[test]
    fn recording_path_includes_sanitized_label() {
        let p = new_recording_path("My Host! (prod)").unwrap();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with("portal-my-host-prod-"), "{name}");
        assert!(name.ends_with(".cast"));

        // Empty/fully-sanitized-away labels fall back to "session".
        let p = new_recording_path("///").unwrap();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with("portal-session-"), "{name}");
    }

    #[test]
    fn set_header_title_preserves_events() {
        let rec = record_and_parse(&["before", "after"], None);
        let path = new_recording_path("title-test").unwrap();
        {
            let mut r = CastRecorder::create(&path, 80, 24, Some("old".into()), None).unwrap();
            for e in &rec.events {
                if e.kind == EventKind::Output {
                    r.output(e.data.as_bytes());
                }
            }
        }
        set_header_title(&path, "renamed title").unwrap();

        let reparsed = parse_cast(&path).unwrap();
        assert_eq!(reparsed.header.title.as_deref(), Some("renamed title"));
        assert_eq!(reparsed.events.len(), 2);
        assert_eq!(reparsed.events[0].data, "before");
        assert_eq!(reparsed.events[1].data, "after");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn set_header_title_on_empty_file_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.cast");
        std::fs::write(&path, "").unwrap();
        assert!(set_header_title(&path, "x").is_err());
    }

    /// Backdate a file's mtime so retention's age pass can be tested.
    fn backdate(path: &Path, age: std::time::Duration) {
        let f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        let modified = std::time::SystemTime::now() - age;
        f.set_times(std::fs::FileTimes::new().set_modified(modified)).unwrap();
    }

    #[test]
    fn retention_disabled_when_both_limits_zero() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.cast");
        std::fs::write(&a, "x").unwrap();
        backdate(&a, std::time::Duration::from_secs(86_400 * 30));
        assert!(enforce_retention(dir.path(), 0, 0).is_empty());
        assert!(a.exists(), "no policy must delete nothing");
    }

    #[test]
    fn retention_age_pass_deletes_only_old_cast_files() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.cast");
        let new = dir.path().join("new.cast");
        let other = dir.path().join("notes.txt");
        std::fs::write(&old, "x").unwrap();
        std::fs::write(&new, "x").unwrap();
        std::fs::write(&other, "x").unwrap();
        backdate(&old, std::time::Duration::from_secs(86_400 * 10));

        let deleted = enforce_retention(dir.path(), 7, 0);
        assert_eq!(deleted, vec![old.clone()]);
        assert!(!old.exists(), "older-than-7d recording must be deleted");
        assert!(new.exists(), "recent recording must survive");
        assert!(other.exists(), "non-.cast files are never touched");
    }

    #[test]
    fn retention_size_budget_deletes_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        let chunk = vec![b'x'; 400 * 1024];
        let mut paths = Vec::new();
        for (i, age_days) in [10u64, 5, 1].iter().enumerate() {
            let p = dir.path().join(format!("rec{i}.cast"));
            std::fs::write(&p, &chunk).unwrap();
            backdate(&p, std::time::Duration::from_secs(86_400 * age_days));
            paths.push(p);
        }
        // 3 × 400KB = 1.2MB total, cap 1MB → oldest (rec0) must go, then
        // total 800KB ≤ 1MB → stop.
        let deleted = enforce_retention(dir.path(), 0, 1);
        assert_eq!(deleted, vec![paths[0].clone()]);
        assert!(!paths[0].exists());
        assert!(paths[1].exists() && paths[2].exists());
    }

    #[test]
    fn retention_size_budget_over_cap_by_one_file_only() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.cast");
        let small = dir.path().join("small.cast");
        std::fs::write(&big, vec![b'x'; 600 * 1024]).unwrap();
        std::fs::write(&small, vec![b'x'; 100 * 1024]).unwrap();
        backdate(&big, std::time::Duration::from_secs(86_400 * 2));
        backdate(&small, std::time::Duration::from_secs(86_400));
        // 700KB total, cap 1MB → nothing to delete.
        assert!(enforce_retention(dir.path(), 0, 1).is_empty());
        assert!(big.exists() && small.exists());
    }
}
