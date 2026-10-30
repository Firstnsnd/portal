//! # AI run-feedback capture
//!
//! When the user confirms "Run" on an assistant-suggested command, Portal types
//! it into the interactive shell wrapped in sentinel markers and taps the
//! session's output stream (the same tap point as the asciicast recorder), so
//! the command's output can be fed back to the AI as part of the conversation.
//! The manual path — the "Copy" button, execution by hand — never taps
//! anything.
//!
//! Sentinel safety: the shell echoes every typed line back before executing
//! it, so the markers are obtained through `printf` templates
//! (`__PORTAL_AI_%s__`, `%s`). The echo contains the literal templates, which
//! can never match the executed markers — only real output can.

/// Marker printed before the command runs; capture starts at the first byte
/// after it.
pub const BEGIN: &[u8] = b"__PORTAL_AI_BEGIN__";
/// Marker printed after the command exits, followed by `:$?` and a newline;
/// capture stops there. Matched only when digits (the exit code) follow.
pub const DONE: &[u8] = b"__PORTAL_AI_DONE__:";

/// How long to wait for the DONE marker before feeding back partial output.
pub const FEEDBACK_TIMEOUT_SECS: u64 = 30;
/// Feedback output is capped at this many characters (keeping the tail — the
/// end of an output usually carries the error).
pub const FEEDBACK_MAX_CHARS: usize = 4000;

/// A session's capture slot: `Some` while an AI-run capture is active. Both
/// backends hold one; the reader thread feeds the capture through it and the
/// watcher thread detaches its own run when done.
pub type AiCaptureSlot = std::sync::Arc<
    std::sync::Mutex<Option<std::sync::Arc<std::sync::Mutex<AiCapture>>>>,
>;

/// Wrap a user-confirmed command in the sentinel printers. Executed through
/// the interactive shell (unchanged UX: the command still runs exactly where
/// the user is), it types three lines:
///
/// ```text
/// printf '__PORTAL_AI_%s__\n' BEGIN ; {
/// <cmd>
/// } ; printf '\n__PORTAL_AI_DONE__:%s\n' "$?"
/// ```
///
/// The brace group keeps the command on its own lines so trailing comments or
/// multiline bodies can't swallow the closing `}` / the final printf. No
/// stderr redirection is needed: a PTY merges stdout and stderr already.
pub fn wrap_command(cmd: &str) -> String {
    format!(
        "printf '__PORTAL_AI_%s__\\n' BEGIN ; {{\n{}\n}} ; printf '\\n__PORTAL_AI_DONE__:%s\\n' \"$?\"",
        cmd
    )
}

/// Per-run capture state tapped into a session's output stream.
pub struct AiCapture {
    /// Bytes seen before BEGIN (echo of the typed lines, prompts, …).
    pre: Vec<u8>,
    /// Output bytes after BEGIN, until the DONE marker.
    out: Vec<u8>,
    started: bool,
    /// A DONE marker with an exit code was seen — capture complete.
    pub done: bool,
    /// Exit code carried by the DONE marker, when captured.
    pub exit_code: Option<i64>,
}

/// Guard against unbounded buffering when BEGIN never arrives.
const PRE_KEEP: usize = 64;
const PRE_CAP: usize = 8 * 1024;
/// Keep at most the last `MAX_KEEP` bytes of output while waiting for DONE
/// (e.g. a `tail -f` the user interrupted by closing the session).
const MAX_KEEP: usize = 64 * 1024;

impl AiCapture {
    pub fn new() -> Self {
        Self {
            pre: Vec::new(),
            out: Vec::new(),
            started: false,
            done: false,
            exit_code: None,
        }
    }

    /// Feed one raw output chunk. Cheap no-op once the capture is complete.
    pub fn feed(&mut self, bytes: &[u8]) {
        if self.done || bytes.is_empty() {
            return;
        }
        if !self.started {
            self.pre.extend_from_slice(bytes);
            if let Some(pos) = find_sub(&self.pre, BEGIN) {
                self.started = true;
                // Skip the marker plus its trailing newline (printf's `\n`).
                let mut rest = &self.pre[pos + BEGIN.len()..];
                if rest.first() == Some(&b'\r') {
                    rest = &rest[1..];
                }
                if rest.first() == Some(&b'\n') {
                    rest = &rest[1..];
                }
                self.out.extend_from_slice(rest);
                self.pre.clear();
                self.scan_done();
            } else if self.pre.len() > PRE_CAP {
                // Nothing matched in a long stretch (huge echo / no wrapper):
                // keep only the tail so a late BEGIN can still be found.
                let drop = self.pre.len() - PRE_KEEP;
                self.pre.drain(..drop);
            }
            return;
        }
        self.out.extend_from_slice(bytes);
        if self.out.len() > 2 * MAX_KEEP {
            let drop = self.out.len() - MAX_KEEP;
            self.out.drain(..drop);
        }
        self.scan_done();
    }

    /// Look for `DONE:<digits><newline>` in the captured output. The line
    /// terminator is required so a marker split mid-digits across chunks
    /// isn't declared complete early.
    fn scan_done(&mut self) {
        if self.done {
            return;
        }
        if let Some(pos) = find_sub(&self.out, DONE) {
            let mut i = pos + DONE.len();
            let digits_start = i;
            while i < self.out.len() && self.out[i].is_ascii_digit() {
                i += 1;
            }
            if i > digits_start && i < self.out.len() && (self.out[i] == b'\r' || self.out[i] == b'\n')
            {
                let code: i64 = std::str::from_utf8(&self.out[digits_start..i])
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(-1);
                // Cut the captured output at the marker line so the sentinel
                // itself never appears in the feedback.
                self.out.truncate(pos);
                self.exit_code = Some(code);
                self.done = true;
            }
        }
    }

    /// Drain the captured output: ANSI-stripped, trimmed, tail-capped to
    /// [`FEEDBACK_MAX_CHARS`].
    pub fn take_output(&mut self) -> String {
        let raw = String::from_utf8_lossy(&self.out).into_owned();
        self.out.clear();
        let text = strip_ansi(&raw);
        let trimmed = text.trim();
        let total = trimmed.chars().count();
        if total > FEEDBACK_MAX_CHARS {
            let skip = total - FEEDBACK_MAX_CHARS;
            format!(
                "…(truncated, showing the last {FEEDBACK_MAX_CHARS} characters)\n{}",
                trimmed.chars().skip(skip).collect::<String>()
            )
        } else {
            trimmed.to_string()
        }
    }
}

impl Default for AiCapture {
    fn default() -> Self {
        Self::new()
    }
}

/// Compose the user-side message fed back to the AI: what ran, how it ended,
/// and what it printed.
pub fn feedback_text(cmd: &str, output: &str, exit_code: Option<i64>, timed_out: bool) -> String {
    let mut s = String::from(
        "I ran the command you suggested in the terminal. Please review the result.\n\nCommand:\n```sh\n",
    );
    s.push_str(cmd);
    s.push_str("\n```\n");
    s.push_str(&format!(
        "Exit code: {}\n",
        exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    ));
    if timed_out {
        s.push_str(&format!(
            "(no completion marker within {FEEDBACK_TIMEOUT_SECS}s — output below may be partial)\n"
        ));
    }
    s.push_str("\nOutput:\n```\n");
    if output.is_empty() {
        s.push_str("(no output)");
    } else {
        s.push_str(output);
    }
    s.push_str("\n```");
    s
}

/// Remove ANSI escape sequences (CSI, OSC, two-byte ESC) so program colors,
/// cursor moves and OSC 7 titles don't pollute the AI feedback.
pub fn strip_ansi(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != 0x1b {
            out.push(b[i]);
            i += 1;
            continue;
        }
        if i + 1 >= b.len() {
            break; // trailing lone ESC
        }
        match b[i + 1] {
            b'[' => {
                // CSI: parameter bytes 0x30–0x3F, intermediates 0x20–0x2F,
                // final byte 0x40–0x7E.
                let mut j = i + 2;
                while j < b.len() && (0x20..=0x3f).contains(&b[j]) {
                    j += 1;
                }
                if j < b.len() {
                    i = j + 1;
                } else {
                    i = b.len();
                }
            }
            b']' => {
                // OSC: terminated by BEL or ST (ESC \).
                let mut j = i + 2;
                let mut end = b.len();
                while j < b.len() {
                    if b[j] == 0x07 {
                        end = j + 1;
                        break;
                    }
                    if b[j] == 0x1b && j + 1 < b.len() && b[j + 1] == b'\\' {
                        end = j + 2;
                        break;
                    }
                    j += 1;
                }
                i = end;
            }
            _ => {
                // Two-byte sequence (ESC 7, ESC c, …). If the byte after ESC
                // is an intermediate (0x20–0x2F) the sequence runs to its
                // final byte (0x30–0x7E) — e.g. charset select `ESC ( B`.
                if (0x20..=0x2f).contains(&b[i + 1]) {
                    let mut j = i + 1;
                    while j < b.len() && (0x20..=0x2f).contains(&b[j]) {
                        j += 1;
                    }
                    i = if j < b.len() { j + 1 } else { b.len() };
                } else {
                    i += 2;
                }
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// First occurrence of `needle` in `hay`.
fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed a byte-splitted stream through a capture, preserving chunk
    /// boundaries exactly as given.
    fn fed(chunks: &[&[u8]]) -> AiCapture {
        let mut cap = AiCapture::new();
        for c in chunks {
            cap.feed(c);
        }
        cap
    }

    #[test]
    fn wraps_command_with_split_sentinels() {
        let wrapped = wrap_command("echo hi");
        // The typed line must contain the TEMPLATE forms (never the executed
        // markers), or the shell's echo would trip the capture.
        assert!(wrapped.contains("__PORTAL_AI_%s__"));
        assert!(wrapped.contains("__PORTAL_AI_DONE__:%s"));
        assert!(!wrapped.contains("__PORTAL_AI_BEGIN__"));
        // The command sits on its own lines inside the brace group.
        assert!(wrapped.contains("{\necho hi\n}"));
    }

    #[test]
    fn captures_output_between_markers() {
        let mut cap = AiCapture::new();
        cap.feed(wrap_command("echo hi").as_bytes());
        cap.feed(b"\r\n");
        // Shell echo + BEGIN + output + DONE.
        cap.feed(b"printf '__PORTAL_AI_%s__\\n' BEGIN ; {\r\necho hi\r\n} ; printf '\\n__PORTAL_AI_DONE__:%s\\n' \"$?\"\r\n");
        cap.feed(b"__PORTAL_AI_BEGIN__\n");
        cap.feed(b"hi\n");
        cap.feed(b"__PORTAL_AI_DONE__:0\n");
        assert!(cap.done);
        assert_eq!(cap.exit_code, Some(0));
        assert_eq!(cap.take_output(), "hi");
    }

    #[test]
    fn sentinels_split_across_chunks_are_found() {
        let mut cap = AiCapture::new();
        cap.feed(b"noise ");
        // BEGIN split byte-wise, DONE split after the exit code digits.
        for b in b"__PORTAL_AI_BEGIN__" {
            cap.feed(&[*b]);
        }
        cap.feed(b"\nresult\n__PORTAL_AI_DONE__:12");
        assert!(!cap.done, "must wait for the line terminator");
        cap.feed(b"\n");
        assert!(cap.done);
        assert_eq!(cap.exit_code, Some(12));
        assert_eq!(cap.take_output(), "result");
    }

    #[test]
    fn echoed_templates_never_match() {
        // The echo of the typed lines contains the literal printf templates —
        // these must not start capture or complete it.
        let mut cap = fed(&[
            b"printf '__PORTAL_AI_%s__\\n' BEGIN ; {\r\n",
            b"echo hi\r\n} ; printf '\\n__PORTAL_AI_DONE__:%s\\n' \"$?\"\r\n",
            b"__PORTAL_AI_BEGIN__\n",
            b"out\n",
            b"__PORTAL_AI_DONE__:3\n",
        ]);
        assert!(cap.done);
        assert_eq!(cap.exit_code, Some(3));
        assert_eq!(cap.take_output(), "out");
    }

    #[test]
    fn non_digit_or_missing_code_does_not_complete() {
        let cap = fed(&[
            b"__PORTAL_AI_BEGIN__\nabc\n__PORTAL_AI_DONE__:xyz\n",
        ]);
        assert!(!cap.done);
        let cap = fed(&[b"__PORTAL_AI_BEGIN__\nabc\n__PORTAL_AI_DONE__:\n"]);
        assert!(!cap.done);
    }

    #[test]
    fn long_output_is_tail_capped() {
        let mut cap = AiCapture::new();
        cap.feed(b"__PORTAL_AI_BEGIN__\n");
        let long = "x".repeat(FEEDBACK_MAX_CHARS + 500);
        cap.feed(long.as_bytes());
        cap.feed(b"__PORTAL_AI_DONE__:0\n");
        let out = cap.take_output();
        assert!(out.contains("truncated"));
        assert!(out.chars().count() < FEEDBACK_MAX_CHARS + 100);
        // The tail (the end, where errors live) is preserved.
        assert!(out.ends_with("xxx"));
    }

    #[test]
    fn feed_after_done_is_ignored() {
        let mut cap = fed(&[
            b"__PORTAL_AI_BEGIN__\nout\n__PORTAL_AI_DONE__:0\n",
            b"later bytes that must not be captured",
        ]);
        assert_eq!(cap.take_output(), "out");
    }

    #[test]
    fn strip_ansi_removes_csi_osc_and_esc_pairs() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m plain"), "red plain");
        assert_eq!(strip_ansi("a\x1b]0;title\x07b"), "ab");
        // OSC-8 hyperlink: both wrappers vanish, the visible text stays.
        assert_eq!(
            strip_ansi("a\x1b]8;;url\x1b\\link\x1b]8;;\x1b\\b"),
            "alinkb"
        );
        assert_eq!(strip_ansi("a\x1b(Bb"), "ab");
        // Non-escape bytes (including UTF-8) pass through untouched.
        assert_eq!(strip_ansi("中文ok"), "中文ok");
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn feedback_text_carries_command_and_exit() {
        let t = feedback_text("df -h", "Filesystem…", Some(1), false);
        assert!(t.contains("```sh\ndf -h\n```"));
        assert!(t.contains("Exit code: 1"));
        assert!(t.contains("Filesystem…"));
        let t = feedback_text("x", "", None, true);
        assert!(t.contains("Exit code: unknown"));
        assert!(t.contains("may be partial"));
        assert!(t.contains("(no output)"));
    }
}
