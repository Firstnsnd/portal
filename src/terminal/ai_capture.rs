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
//!
//! That echo is also why the raw wrapper would be visible in the terminal.
//! An armed capture therefore also rewrites the *display* stream (only the
//! grid parse — the capture and asciicast taps still see raw bytes): the
//! shell's echo of the wrapped command is replaced by the bare command, and
//! the executed sentinel lines are never rendered. The rewrite matches the
//! known echo byte-for-byte while skipping whatever the shell interleaves
//! (prompts, ANSI highlighting, redraws) — it can never fall back to
//! showing the wrapper: the worst a divergent stream can do is end the
//! rewrite with the bare command anyway.

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
pub type AiCaptureSlot =
    std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<std::sync::Mutex<AiCapture>>>>>;

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
    /// Display-side rewrite; `None` for a bare capture (no Run arming).
    display: Option<DisplayRewrite>,
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
            display: None,
        }
    }

    /// Arm a capture for an AI-run of `cmd` that was (or is about to be)
    /// typed as `wrapped`: besides capturing output, the terminal display is
    /// rewritten so the shell's echo of the wrapper shows as the bare
    /// command and the executed sentinel lines are never rendered.
    pub fn for_wrapped(cmd: &str, wrapped: &str) -> Self {
        let mut expected = normalize_newlines(wrapped).into_bytes();
        expected.extend_from_slice(b"\r\n"); // echo of the typed Enter
        debug_assert!(expected.starts_with(WRAPPER_ANCHOR));
        Self {
            display: Some(DisplayRewrite {
                expected,
                replacement: {
                    let mut r = normalize_newlines(cmd).into_bytes();
                    r.extend_from_slice(b"\r\n");
                    r
                },
                ei: 0,
                cr_pending: false,
                noise_run: 0,
                noise_total: 0,
                pending: Vec::new(),
                done: 0,
                drop_line_end: 0,
                esc: EscSkip::default(),
                echo_settled: false,
                hold: None,
            }),
            ..Self::new()
        }
    }

    /// Display bytes for one raw output chunk: with the rewrite armed, the
    /// wrapper echo is replaced by the bare command and executed sentinel
    /// lines are dropped; once the capture is done, bytes flow verbatim.
    pub fn rewrite_display(&mut self, bytes: &[u8]) -> Vec<u8> {
        if self.done {
            return bytes.to_vec();
        }
        match &mut self.display {
            Some(d) => d.run(bytes),
            None => bytes.to_vec(),
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
            if i > digits_start
                && i < self.out.len()
                && (self.out[i] == b'\r' || self.out[i] == b'\n')
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

/// Display bytes for one raw chunk through the armed capture slot, if any.
/// The grid parse should consume these; the capture and asciicast taps keep
/// the raw bytes. Falls back to verbatim on lock trouble.
pub fn display_bytes(slot: &AiCaptureSlot, bytes: &[u8]) -> Vec<u8> {
    let Ok(s) = slot.lock() else {
        return bytes.to_vec();
    };
    match s.as_ref() {
        Some(cap) => match cap.lock() {
            Ok(mut cap) => cap.rewrite_display(bytes),
            Err(_) => bytes.to_vec(),
        },
        None => bytes.to_vec(),
    }
}

/// Expand every newline (`\n`, and a typed `\r`) to the `\r\n` the line
/// discipline echoes back, so the echo matcher can compare byte-for-byte.
fn normalize_newlines(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        if c == '\n' || c == '\r' {
            out.push_str("\r\n");
        } else {
            out.push(c);
        }
    }
    out
}

/// Byte prefix of the wrapper's first echoed line. The echo is recognized by
/// this anchor; a mangled `printf '` prefix is still accepted via the bare
/// `__PORTAL_AI_` signature — ordinary output carries neither.
const WRAPPER_ANCHOR: &[u8] = b"printf '__PORTAL_AI_";
const WRAPPER_SIGNATURE: &[u8] = b"__PORTAL_AI_";

/// Lead budget: bytes that may pass without any wrapper signature before the
/// rewrite decides the echo never came (echo disabled) and flows verbatim.
const LEAD_BUDGET: usize = 512;
/// Body: longest stretch of plain noise (no expected byte matching) before
/// the rewrite gives up. Real prompts — bash's `> `, colored `cursh> `,
/// multi-line PS1/PS2, highlighting redraws — are far below this; the cap
/// exists only so a stream that is genuinely not the echo can't be held
/// forever.
const BODY_RUN_CAP: usize = 2048;
/// Body: total noise budget (plain + escape bytes) across the whole echo.
const BODY_BUDGET: usize = 64 * 1024;
/// Bytes swallowed after an executed BEGIN marker spotted in the lead (the
/// marker's own line terminator).
const DROP_LINE_END_CAP: usize = 4;

/// Incremental ANSI escape-sequence skipper. Feed bytes; `true` means the
/// byte is part of an escape sequence and is transparent to the matcher.
/// State spans chunk boundaries.
#[derive(Default)]
struct EscSkip {
    state: EscState,
}

#[derive(Default)]
enum EscState {
    #[default]
    None,
    /// ESC seen, next byte decides the kind.
    Esc,
    /// CSI: parameters/intermediates then a final byte.
    Csi,
    /// OSC string, terminated by BEL or ST (ESC \).
    Osc,
    /// Got the ESC of an ST inside an OSC.
    OscEsc,
}

impl EscSkip {
    fn feed(&mut self, b: u8) -> bool {
        match self.state {
            EscState::None => {
                if b == 0x1b {
                    self.state = EscState::Esc;
                    true
                } else {
                    false
                }
            }
            EscState::Esc => {
                match b {
                    b'[' => self.state = EscState::Csi,
                    b']' => self.state = EscState::Osc,
                    _ => self.state = EscState::None, // two-byte sequence
                }
                true
            }
            EscState::Csi => {
                match b {
                    0x20..=0x3f => true, // parameter / intermediate
                    0x40..=0x7e => {
                        self.state = EscState::None; // final byte
                        true
                    }
                    _ => {
                        self.state = EscState::None;
                        false // control byte inside CSI — reprocess it
                    }
                }
            }
            EscState::Osc => {
                match b {
                    0x07 => self.state = EscState::None, // BEL
                    0x1b => self.state = EscState::OscEsc,
                    _ => {}
                }
                true
            }
            EscState::OscEsc => {
                self.state = EscState::None;
                b == b'\\' // ST complete; anything else is reprocessed
            }
        }
    }
}

/// Display-side rewrite state for an armed AI run: swallow the shell's echo
/// of the wrapped command and show only the bare command instead, then hide
/// the executed sentinel lines.
///
/// The echo is never shown, whatever the shell does around it. Bytes before
/// the wrapper signature are dropped outright (the shell's prompt is already
/// on screen from before the Run). From the signature on, every byte that
/// doesn't continue the expected sequence is prompt/highlight/redraw noise
/// and is skipped (ANSI sequences whole; plain noise bounded by generous
/// budgets). The rewrite ends with the bare command on screen — on a clean
/// match, and on budget exhaustion alike. There is deliberately no "fall
/// back to verbatim" path for the echo itself: divergence can at worst drop
/// noise bytes, never resurrect the wrapper.
struct DisplayRewrite {
    /// The exact byte sequence we expect the shell to echo back: the wrapper
    /// with every newline expanded to `\r\n`, plus the echo of the typed
    /// Enter. ANSI decorations and prompts are skipped as noise around it.
    expected: Vec<u8>,
    /// What to render once the whole echo has been swallowed.
    replacement: Vec<u8>,
    /// Match position within `expected`.
    ei: usize,
    /// `expected[ei - 1]` was the `\r` of a `\r\n` pair, awaiting its `\n`.
    cr_pending: bool,
    /// Contiguous plain-noise bytes in the echo body.
    noise_run: usize,
    /// Total noise bytes (plain + escape) in the echo body.
    noise_total: usize,
    /// Bytes held while deciding (pre-signature search, mid-match holds).
    pending: Vec<u8>,
    /// How much of `pending` has been decided.
    done: usize,
    /// Remaining bytes to swallow right after a lead-spotted BEGIN marker
    /// (its line terminator) before suppression takes over.
    drop_line_end: usize,
    /// In-flight ANSI escape sequence (transparent to matching).
    esc: EscSkip,
    /// The echo phase is over (matched or gave up) — raw flow, sentinel
    /// lines suppressed.
    echo_settled: bool,
    /// Tail bytes that might be the start of an executed sentinel line.
    hold: Option<Vec<u8>>,
}

impl DisplayRewrite {
    fn run(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len());
        if self.echo_settled {
            self.emit_tail(&mut out, bytes);
            return out;
        }
        self.pending.extend_from_slice(bytes);
        self.progress(&mut out);
        if self.echo_settled && self.done < self.pending.len() {
            let tail = self.pending[self.done..].to_vec();
            self.done = self.pending.len();
            self.emit_tail(&mut out, &tail);
        }
        if self.done >= self.pending.len() {
            self.pending.clear();
            self.done = 0;
        } else if self.done > 8192 {
            self.pending.drain(..self.done);
            self.done = 0;
        }
        out
    }

    /// Emit settled-phase bytes: drop a lead-spotted BEGIN marker's line end,
    /// suppress executed sentinel lines, pass everything else through.
    fn emit_tail(&mut self, out: &mut Vec<u8>, bytes: &[u8]) {
        for &b in bytes {
            if self.drop_line_end > 0 {
                self.drop_line_end -= 1;
                if b == b'\n' {
                    self.drop_line_end = 0;
                }
                continue;
            }
            suppress_sentinel_lines(out, &mut self.hold, std::slice::from_ref(&b));
        }
    }

    /// Advance the undecided prefix of `pending`.
    fn progress(&mut self, out: &mut Vec<u8>) {
        while self.done < self.pending.len() && !self.echo_settled {
            if self.ei == 0 {
                // Lead: hold bytes while scanning for our signature.
                let stream = &self.pending[self.done..];
                let (hit, width) = if let Some(p) = find_sub(stream, WRAPPER_ANCHOR) {
                    (p, WRAPPER_ANCHOR.len())
                } else if let Some(p) = find_sub(stream, BEGIN) {
                    // Checked before the bare signature — BEGIN contains it.
                    // The executed marker can only come from execution: the
                    // shell has echo disabled and the run already started.
                    // Swallow the marker line end, then flow.
                    self.done += p + BEGIN.len();
                    self.drop_line_end = DROP_LINE_END_CAP;
                    self.echo_settled = true;
                    continue;
                } else if let Some(p) = find_sub(stream, WRAPPER_SIGNATURE) {
                    // `printf '` mangled but the signature survived — still
                    // our echo; resume aligned just past the signature.
                    (p, WRAPPER_SIGNATURE.len())
                } else if stream.len() > LEAD_BUDGET {
                    // No echo showed up at all (echo off, input swallowed by
                    // a program) — pass what arrived through, markers still
                    // suppressed.
                    out.extend_from_slice(stream);
                    self.done = self.pending.len();
                    self.echo_settled = true;
                    continue;
                } else {
                    return; // keep holding until a decision is possible
                };
                self.done += hit + width;
                self.ei = WRAPPER_ANCHOR.len();
                continue;
            }

            let b = self.pending[self.done];
            if self.esc.feed(b) {
                // ANSI sequence bytes are transparent: syntax-highlighted
                // echoes carry SGR between the tokens of the command itself.
                self.done += 1;
                self.noise_total += 1;
                continue;
            }
            let matched = self.match_byte(b);
            self.done += 1;
            if matched {
                self.noise_run = 0;
            } else {
                // Prompts, redraws and stray control bytes are noise.
                // Bounded, so a genuinely alien stream can't be held forever
                // — and even then the rewrite ends with the bare command,
                // never the wrapper; the unconsumed remainder is dropped.
                self.noise_run += 1;
                self.noise_total += 1;
                if self.noise_run > BODY_RUN_CAP || self.noise_total > BODY_BUDGET {
                    self.ei = self.expected.len();
                }
            }
            if self.ei >= self.expected.len() {
                // Whole echo swallowed (or given up on) — the bare command
                // takes its place; whatever followed it flows on (markers
                // suppressed). On a budget give-up the unconsumed remainder
                // is dropped with the wrapper.
                if self.noise_run > 0 {
                    self.done = self.pending.len();
                }
                self.echo_settled = true;
                out.extend_from_slice(&self.replacement);
            }
        }
    }

    /// Try to consume `b` at the current position; `false` = noise.
    fn match_byte(&mut self, b: u8) -> bool {
        if self.ei >= self.expected.len() {
            return false;
        }
        if self.cr_pending {
            self.cr_pending = false;
            if b == b'\n' {
                self.ei += 1;
                return true;
            }
            // CR-only line end (no ONLCR): the newline half of the expected
            // pair is skipped and this byte re-matches at the next position.
            self.ei += 1;
        }
        if self.expected.get(self.ei) != Some(&b) {
            // A lone LF standing in for an expected `\r\n` pair.
            if b == b'\n' && self.expected[self.ei..].starts_with(b"\r\n") {
                self.ei += 2;
                return true;
            }
            return false;
        }
        self.ei += 1;
        if self.expected.get(self.ei) == Some(&b'\n') && self.expected[self.ei - 1] == b'\r' {
            self.cr_pending = true; // matched `\r` of a pair — await the `\n`
        }
        true
    }
}

/// Copy `bytes` to `out`, dropping the executed `__PORTAL_AI_BEGIN__` /
/// `__PORTAL_AI_DONE__:<code>` lines. A chunk-boundary tail that could still
/// grow into a marker is held in `hold` until it resolves (the shell always
/// prints a fresh prompt right after DONE, so the hold is momentary).
fn suppress_sentinel_lines(out: &mut Vec<u8>, hold: &mut Option<Vec<u8>>, bytes: &[u8]) {
    for &b in bytes {
        match hold {
            None if b == BEGIN[0] => *hold = Some(vec![b]),
            None => out.push(b),
            Some(h) => {
                h.push(b);
                if sentinel_line_full(h) {
                    *hold = None; // executed marker — never rendered
                } else if sentinel_line_prefix(h) {
                    // Still could become a marker line — keep holding.
                } else {
                    out.extend_from_slice(h);
                    *hold = None;
                }
            }
        }
    }
}

/// `h` could still grow into a full sentinel line (`\r\n`-terminated; DONE
/// carries one or more exit-code digits).
fn sentinel_line_prefix(h: &[u8]) -> bool {
    if BEGIN.starts_with(h) || DONE.starts_with(h) {
        return true;
    }
    let rest = if h.starts_with(BEGIN) {
        &h[BEGIN.len()..]
    } else if h.starts_with(DONE) {
        let d = &h[DONE.len()..];
        let digits = d.iter().take_while(|b| b.is_ascii_digit()).count();
        if digits == 0 {
            return false;
        }
        &d[digits..]
    } else {
        return false;
    };
    b"\r\n".starts_with(rest)
}

/// `h` IS a complete executed sentinel line — suppress it.
fn sentinel_line_full(h: &[u8]) -> bool {
    if h.starts_with(BEGIN) {
        let rest = &h[BEGIN.len()..];
        return rest == b"\r\n" || rest == b"\n";
    }
    if h.starts_with(DONE) {
        let d = &h[DONE.len()..];
        let digits = d.iter().take_while(|b| b.is_ascii_digit()).count();
        let tail = &d[digits..];
        return digits > 0 && (tail == b"\r\n" || tail == b"\n");
    }
    false
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
        let cap = fed(&[b"__PORTAL_AI_BEGIN__\nabc\n__PORTAL_AI_DONE__:xyz\n"]);
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

    /// The echo bytes a line discipline sends back for `wrap_command("echo
    /// hi")` + Enter (templates intact, newlines expanded to `\r\n`).
    fn echoed_wrapper() -> Vec<u8> {
        let mut e = wrap_command("echo hi").into_bytes();
        e.push(b'\r');
        normalize_newlines(std::str::from_utf8(&e).unwrap()).into_bytes()
    }

    #[test]
    fn display_shows_only_bare_command_and_output() {
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let echo = echoed_wrapper();
        let d = cap.rewrite_display(&echo);
        cap.feed(&echo);
        assert_eq!(d, b"echo hi\r\n");
        // Executed markers vanish from the display; real output stays.
        let raw = &b"__PORTAL_AI_BEGIN__\r\nhi\r\n__PORTAL_AI_DONE__:0\r\n"[..];
        let d = cap.rewrite_display(raw);
        cap.feed(raw);
        assert_eq!(d, b"hi\r\n");
        // The capture side saw the raw stream untouched.
        assert!(cap.done);
        assert_eq!(cap.exit_code, Some(0));
        assert_eq!(cap.take_output(), "hi");
    }

    #[test]
    fn display_echo_split_across_chunks_is_replaced() {
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let echo = echoed_wrapper();
        let (a, b) = echo.split_at(17);
        assert_eq!(cap.rewrite_display(a), b""); // held mid-match
        assert_eq!(cap.rewrite_display(b), b"echo hi\r\n");
        assert_eq!(cap.rewrite_display(b"__PORTAL_AI_BEGIN__\n"), b"");
        assert_eq!(cap.rewrite_display(b"out\n"), b"out\n");
    }

    #[test]
    fn display_diverged_echo_still_shows_bare_command() {
        // First byte mangled (echo quirk / redraw): the rewrite must never
        // fall back to showing the wrapper — the bare command still renders.
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let mut echo = echoed_wrapper();
        echo[0] = b'X';
        assert_eq!(cap.rewrite_display(&echo), b"echo hi\r\n");
        // From here on bytes flow as they come, sentinels still suppressed.
        let d = cap.rewrite_display(b"__PORTAL_AI_BEGIN__\r\nx");
        assert_eq!(d, b"x");
    }

    #[test]
    fn display_mangled_anchor_prefix_still_shows_bare_command() {
        // A byte inside the `printf '` prefix gets mangled (echo quirk) —
        // the bare `__PORTAL_AI_` signature still recognizes the echo, the
        // junk prefix is dropped, and only the bare command renders.
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let mut echo = echoed_wrapper();
        echo[2] = b'@'; // breaks `printf '…` — signature still present
        assert_eq!(cap.rewrite_display(&echo), b"echo hi\r\n");
    }

    #[test]
    fn display_lookalike_output_without_signature_flows_verbatim() {
        // Ordinary output (no wrapper signature) before the lead budget is
        // passed through untouched once the rewrite gives up.
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let mut noise = Vec::new();
        for i in 0..80 {
            noise.extend_from_slice(format!("out {i}\r\n").as_bytes());
        }
        let d = cap.rewrite_display(&noise);
        cap.feed(&noise);
        assert_eq!(d, noise);
    }

    #[test]
    fn display_sentinels_split_across_chunks_are_hidden() {
        let mut cap = AiCapture::for_wrapped("true", &wrap_command("true"));
        let echo = echoed_wrapper_for("true");
        cap.rewrite_display(&echo);
        cap.feed(&echo);
        for (raw, shown) in [
            (&b"__PORTAL_AI_BE"[..], &b""[..]),
            (&b"GIN__\r\nok\r\n__PORTAL_AI_DO"[..], &b"ok\r\n"[..]),
            (&b"NE__:7\r\ntail"[..], &b"tail"[..]),
        ] {
            assert_eq!(cap.rewrite_display(raw), shown);
            cap.feed(raw);
        }
        assert!(cap.done);
        assert_eq!(cap.exit_code, Some(7));
    }

    #[test]
    fn display_lookalike_lines_stay_visible() {
        let mut cap = AiCapture::for_wrapped("true", &wrap_command("true"));
        cap.rewrite_display(&echoed_wrapper_for("true"));
        // No digits after DONE:, or non-marker text containing the prefix:
        // neither forms an executed marker line, so both render.
        let d = cap.rewrite_display(b"__PORTAL_AI_DONE__:\r\nsee __PORTAL_AI_X\r\n");
        assert_eq!(d, b"__PORTAL_AI_DONE__:\r\nsee __PORTAL_AI_X\r\n");
    }

    #[test]
    fn display_after_done_passes_through() {
        let mut cap = AiCapture::for_wrapped("true", &wrap_command("true"));
        cap.rewrite_display(&echoed_wrapper_for("true"));
        cap.feed(b"__PORTAL_AI_BEGIN__\nout\n__PORTAL_AI_DONE__:0\n");
        assert!(cap.done);
        // Once the run completed, even marker-shaped bytes render verbatim.
        assert_eq!(
            cap.rewrite_display(b"__PORTAL_AI_BEGIN__"),
            b"__PORTAL_AI_BEGIN__"
        );
    }

    #[test]
    fn bare_capture_display_is_verbatim() {
        let mut cap = AiCapture::new();
        assert_eq!(
            cap.rewrite_display(b"__PORTAL_AI_BEGIN__\r\n"),
            b"__PORTAL_AI_BEGIN__\r\n"
        );
    }

    #[test]
    fn display_bytes_helper_passes_through_when_unarmed() {
        let slot: AiCaptureSlot = std::sync::Arc::new(std::sync::Mutex::new(None));
        assert_eq!(display_bytes(&slot, b"abc"), b"abc");
    }

    /// Echo bytes for an arbitrary command's wrapper (tests above).
    fn echoed_wrapper_for(cmd: &str) -> Vec<u8> {
        let mut e = wrap_command(cmd).into_bytes();
        e.push(b'\r');
        normalize_newlines(std::str::from_utf8(&e).unwrap()).into_bytes()
    }

    /// Bash-style echo: PS2 (`> `) precedes every continuation line; each
    /// line ends with the `\r\n` newline echo; the final Enter adds the last
    /// `\r\n` with no prompt after it.
    fn echoed_wrapper_bash(cmd: &str) -> Vec<u8> {
        let mut e = Vec::new();
        for (i, line) in wrap_command(cmd).split('\n').enumerate() {
            if i > 0 {
                e.extend_from_slice(b"> ");
            }
            e.extend_from_slice(line.as_bytes());
            e.extend_from_slice(b"\r\n");
        }
        e
    }

    #[test]
    fn display_bash_ps2_prompts_are_tolerated() {
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let echo = echoed_wrapper_bash("echo hi");
        let d = cap.rewrite_display(&echo);
        cap.feed(&echo);
        assert_eq!(d, b"echo hi\r\n");
    }

    #[test]
    fn display_line_starting_like_ps2_is_not_mistaken_for_prompt() {
        // `> /tmp/x` as the command: bash echoes `> ` (PS2) + `> /tmp/x`.
        // The smallest-skip rule must resolve k=2, not k=0.
        let cmd = "> /tmp/x";
        let mut cap = AiCapture::for_wrapped(cmd, &wrap_command(cmd));
        let echo = echoed_wrapper_bash(cmd);
        let d = cap.rewrite_display(&echo);
        cap.feed(&echo);
        assert_eq!(d, b"> /tmp/x\r\n");
    }

    #[test]
    fn display_bash_echo_split_across_chunks_is_replaced() {
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let echo = echoed_wrapper_bash("echo hi");
        let (a, b) = echo.split_at(echo.len() / 2);
        assert_eq!(cap.rewrite_display(a), b""); // held mid-match
        assert_eq!(cap.rewrite_display(b), b"echo hi\r\n");
    }

    #[test]
    fn display_multiline_command_keeps_inner_lines() {
        let cmd = "echo a\necho b";
        let mut cap = AiCapture::for_wrapped(cmd, &wrap_command(cmd));
        let echo = echoed_wrapper_bash(cmd);
        let d = cap.rewrite_display(&echo);
        cap.feed(&echo);
        assert_eq!(d, b"echo a\r\necho b\r\n");
    }

    /// ANSI-decorated PS2 (a colored `cursh> `): the escape sequences
    /// inflate the prompt to 22 bytes — past the old 16-byte blind cap,
    /// which made the rewriter bail out and show the raw wrapper echo.
    fn echoed_wrapper_colored_ps2(cmd: &str) -> Vec<u8> {
        let prompt = "\u{1b}[38;5;208mcursh> \u{1b}[0m"; // 11 + 7 + 4 = 22 bytes
        let mut e = Vec::new();
        for (i, line) in wrap_command(cmd).split('\n').enumerate() {
            if i > 0 {
                e.extend_from_slice(prompt.as_bytes());
            }
            e.extend_from_slice(line.as_bytes());
            e.extend_from_slice(b"\r\n");
        }
        e
    }

    #[test]
    fn display_ansi_colored_ps2_prompt_is_tolerated() {
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let echo = echoed_wrapper_colored_ps2("echo hi");
        let d = cap.rewrite_display(&echo);
        cap.feed(&echo);
        assert_eq!(d, b"echo hi\r\n");
    }

    #[test]
    fn display_colored_ps2_split_across_chunks_is_replaced() {
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let echo = echoed_wrapper_colored_ps2("echo hi");
        let (a, b) = echo.split_at(echo.len() / 2);
        assert_eq!(cap.rewrite_display(a), b""); // held mid-match
        assert_eq!(cap.rewrite_display(b), b"echo hi\r\n");
    }

    #[test]
    fn display_blank_line_noise_is_tolerated() {
        // A stray blank line between two echo lines (multi-line prompt
        // jitter) is noise — matching resumes after it; the wrapper never
        // shows.
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let mut echo = echoed_wrapper();
        let at = echo.iter().position(|&b| b == b'\n').unwrap() + 1;
        echo.splice(at..at, b"\r\n".to_vec());
        assert_eq!(cap.rewrite_display(&echo), b"echo hi\r\n");
    }

    /// A heavily decorated prompt (truecolor + bold, ~80 bytes) — the old
    /// 48-byte cap made exactly this shape bail out and show the raw echo.
    fn echoed_wrapper_fancy_ps2(cmd: &str) -> Vec<u8> {
        let prompt = "\u{1b}[1m\u{1b}[38;2;255;165;0muser\u{1b}[0m@\u{1b}[38;5;39mhost\u{1b}[0m \u{1b}[38;5;208mcursh\u{1b}[0m\u{1b}[38;5;46m❯\u{1b}[0m ";
        let mut e = Vec::new();
        for (i, line) in wrap_command(cmd).split('\n').enumerate() {
            if i > 0 {
                e.extend_from_slice(prompt.as_bytes());
            }
            e.extend_from_slice(line.as_bytes());
            e.extend_from_slice(b"\r\n");
        }
        e
    }

    #[test]
    fn display_long_fancy_ps2_prompt_is_tolerated() {
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let echo = echoed_wrapper_fancy_ps2("echo hi");
        assert!(echo.len() > 240);
        let d = cap.rewrite_display(&echo);
        cap.feed(&echo);
        assert_eq!(d, b"echo hi\r\n");
    }

    #[test]
    fn display_syntax_highlighted_echo_is_tolerated() {
        // Shells that highlight the line as they echo it (zsh ZLE,
        // prompt_toolkit) interleave SGR sequences BETWEEN the tokens of the
        // wrapper itself — the whole way through.
        let sgr = b"\x1b[32m";
        let reset = b"\x1b[0m";
        let mut echo = Vec::new();
        for line in wrap_command("echo hi").split('\n') {
            echo.extend_from_slice(sgr);
            echo.extend_from_slice(line.as_bytes());
            echo.extend_from_slice(reset);
            echo.extend_from_slice(b"\r\n");
        }
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let d = cap.rewrite_display(&echo);
        cap.feed(&echo);
        assert_eq!(d, b"echo hi\r\n");
    }

    #[test]
    fn display_readline_redraw_noise_is_tolerated() {
        // Horizontal-scroll redraws: `\r` + clear-to-EOL + re-rendered
        // prompt right in the middle of the echo.
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let mut echo = echoed_wrapper();
        let at = echo.iter().position(|&b| b == b'\n').unwrap() + 1;
        echo.splice(at..at, b"\r\x1b[2K\x1b[1mcursh> \x1b[0m".to_vec());
        let d = cap.rewrite_display(&echo);
        cap.feed(&echo);
        assert_eq!(d, b"echo hi\r\n");
    }

    #[test]
    fn display_lf_only_line_ends_are_matched() {
        let echo = echoed_wrapper()
            .iter()
            .copied()
            .filter(|&b| b != b'\r')
            .collect::<Vec<u8>>();
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let d = cap.rewrite_display(&echo);
        cap.feed(&echo);
        assert_eq!(d, b"echo hi\r\n");
    }

    #[test]
    fn display_cr_only_line_ends_are_matched() {
        let echo = echoed_wrapper()
            .iter()
            .map(|&b| if b == b'\n' { 0u8 } else { b })
            .collect::<Vec<u8>>();
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let d = cap.rewrite_display(&echo);
        cap.feed(&echo);
        assert_eq!(d, b"echo hi\r\n");
    }

    #[test]
    fn display_prompt_before_echo_is_dropped() {
        // The shell's PS1 was still in flight when the Run was confirmed —
        // pre-signature bytes are dropped with the wrapper; the bare
        // command is the only thing that renders.
        let prompt = b"\x1b[38;5;208mcursh> \x1b[0m";
        let mut echo = prompt.to_vec();
        echo.extend_from_slice(&echoed_wrapper());
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let d = cap.rewrite_display(&echo);
        cap.feed(&echo);
        assert_eq!(d, b"echo hi\r\n");
    }

    #[test]
    fn display_echo_off_output_flows_and_markers_stay_hidden() {
        // Echo disabled: the wrapper never comes back. Pre-signature output
        // is dropped with it, the run's own output renders, and no wrapper
        // text ever does.
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let mut stream = Vec::new();
        for i in 0..40 {
            stream.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        stream.extend_from_slice(b"__PORTAL_AI_BEGIN__\r\nhi\r\n__PORTAL_AI_DONE__:0\r\n");
        let d = cap.rewrite_display(&stream);
        cap.feed(&stream);
        let text = String::from_utf8_lossy(&d).into_owned();
        assert!(!text.contains("PORTAL_AI"));
        assert!(!text.contains("printf"));
        assert_eq!(text, "hi\r\n");
        // Capture side still saw everything raw.
        assert!(cap.done);
        assert_eq!(cap.take_output(), "hi");
    }

    #[test]
    fn display_begin_in_lead_never_renders() {
        // Execution output arrives with no echo at all — BEGIN in the very
        // first bytes discards the pre-anchor buffer instead of showing it.
        let mut cap = AiCapture::for_wrapped("true", &wrap_command("true"));
        let raw = &b"__PORTAL_AI_BEGIN__\r\nok\r\n__PORTAL_AI_DONE__:2\r\n"[..];
        let d = cap.rewrite_display(raw);
        cap.feed(raw);
        assert_eq!(d, b"ok\r\n");
        assert!(cap.done);
        assert_eq!(cap.exit_code, Some(2));
    }

    #[test]
    fn display_split_mid_escape_sequence_is_replaced() {
        // Chunk boundary lands inside a prompt's ANSI sequence.
        let mut cap = AiCapture::for_wrapped("echo hi", &wrap_command("echo hi"));
        let echo = echoed_wrapper_fancy_ps2("echo hi");
        // Split right after the ESC of the second line's prompt.
        let second_esc = echo
            .windows(2)
            .enumerate()
            .find(|(i, w)| w == b"\x1b[" && *i > echo.len() / 2)
            .map(|(i, _)| i + 1)
            .unwrap();
        let (a, b) = echo.split_at(second_esc);
        assert_eq!(cap.rewrite_display(a), b""); // held (mid-escape + mid-match)
        assert_eq!(cap.rewrite_display(b), b"echo hi\r\n");
    }
}
