//! # Lightweight Markdown rendering for the AI chat drawer
//!
//! A zero-dependency block + inline renderer for the subset of Markdown an
//! assistant reply typically uses: ATX headings, `**bold**`, `*italic*`,
//! `` `inline code` ``, `[text](url)` links, unordered/ordered lists, block
//! quotes, and `---` horizontal rules. Fenced code blocks are handed to a
//! caller-provided callback *in place* so runnable command cards appear exactly
//! where the prose put them, not collected at the bottom of the bubble.
//!
//! Rendering is immediate-mode: each block is broken into inline spans, and each
//! span is drawn with the appropriate [`egui::RichText`] styling. Links are
//! clickable (opens the URL via `egui::Context::open_url`); the link `path` is
//! duplicated verbatim (with URL-encoded chars decoded) so the copy target is
//! the human-readable URL, not raw Markdown.

use eframe::egui;

use crate::ui::theme::ThemeColors;

/// An inline span: a run of text with a uniform style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    Plain,
    Bold,
    Italic,
    Code,
    Link,
}

struct Span {
    style: Style,
    text: String,
    /// For `Link`: the destination URL.
    href: Option<String>,
}

/// A single ordered unit in a Markdown document: either prose text or a fenced
/// code block. The order matters — a code block stays exactly where it appeared
/// in the source, so "foo\n```sh\nls\n```\nbar" yields `Text(foo), Code(ls),
/// Text(bar)`, not `Text(foo), Text(bar), Code(ls)`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MarkdownBlock {
    Text(String),
    Code(String),
}

/// Split Markdown source into an ordered list of [`MarkdownBlock`]s, separating
/// fenced code blocks from prose while preserving their relative positions.
/// Fences are `` ``` `` (a leading triple backtick, optionally a language tag);
/// blocks without a closing fence are dropped (matching how prose Markdown
/// treats an unterminated fence).
fn split_blocks(text: &str) -> Vec<MarkdownBlock> {
    let mut blocks = Vec::new();
    let mut prose = String::new();
    let mut in_fence = false;
    let mut code = String::new();

    let flush_prose = |blocks: &mut Vec<MarkdownBlock>, prose: &mut String| {
        let trimmed = prose.trim_end();
        if !trimmed.is_empty() {
            blocks.push(MarkdownBlock::Text(trimmed.to_string()));
        }
        prose.clear();
    };

    for line in text.lines() {
        if is_fence_line(line) {
            if in_fence {
                // Closing fence.
                in_fence = false;
                let trimmed = code.trim_end();
                if !trimmed.is_empty() {
                    blocks.push(MarkdownBlock::Code(trimmed.to_string()));
                }
                code.clear();
            } else {
                // Opening fence — flush any pending prose first.
                in_fence = true;
                flush_prose(&mut blocks, &mut prose);
                code.clear();
            }
            continue;
        }
        if in_fence {
            code.push_str(line);
            code.push('\n');
        } else {
            prose.push_str(line);
            prose.push('\n');
        }
    }
    // Trailing prose (an unterminated trailing code block is intentionally
    // dropped, matching the closing-fence requirement).
    flush_prose(&mut blocks, &mut prose);
    blocks
}

/// True when a line is a Markdown fence: a leading `` ``` `` (optionally
/// followed by an info string like `sh` or `rust`), possibly indented.
fn is_fence_line(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

/// Render one message body as styled Markdown.
///
/// `max_width` is the inner width available to the bubble content; text wraps
/// within it. `base_size` is the point size of plain body text.
///
/// Fenced code blocks are handed to `code_block` (a runnable command card) at
/// their *original* position in the prose, so a reply like "run this\n```sh\n…
/// ```\nthen check" renders the command between the two paragraphs instead of
/// collecting it at the bottom of the bubble.
pub fn render_markdown(
    ui: &mut egui::Ui,
    text: &str,
    theme: &ThemeColors,
    base_size: f32,
    max_width: f32,
    mut code_block: impl FnMut(&mut egui::Ui, &str),
) {
    for block in split_blocks(text) {
        match block {
            MarkdownBlock::Text(s) => {
                ui.add_space(2.0);
                render_block(ui, &s, theme, base_size, max_width);
            }
            MarkdownBlock::Code(c) => {
                ui.add_space(4.0);
                code_block(ui, &c);
                ui.add_space(4.0);
            }
        }
    }
}

fn render_block(ui: &mut egui::Ui, line: &str, theme: &ThemeColors, base_size: f32, max_width: f32) {
    let trimmed = line.trim_end();

    // Horizontal rule.
    let rule_chars = trimmed.trim();
    if !rule_chars.is_empty()
        && rule_chars.chars().all(|c| c == '-' || c == '_' || c == '*')
        && rule_chars.len() >= 3
    {
        ui.add_space(2.0);
        ui.separator();
        ui.add_space(2.0);
        return;
    }

    // ATX heading: 1–6 leading `#`.
    if let Some((level, rest)) = parse_heading(trimmed) {
        let size = match level {
            1 => base_size + 5.0,
            2 => base_size + 3.0,
            3 => base_size + 1.0,
            _ => base_size,
        };
        let strong = level <= 3;
        ui.set_max_width(max_width);
        render_inline(ui, rest, theme, size, strong, max_width);
        if level <= 2 {
            ui.add_space(2.0);
        }
        return;
    }

    // Block quote: a leading `>`.
    if let Some(rest) = trimmed.strip_prefix('>') {
        let quote = rest.trim_start_matches(' ').trim_start();
        ui.set_max_width(max_width - 8.0);
        egui::Frame::none()
            .inner_margin(egui::Margin { left: 8.0, right: 0.0, top: 0.0, bottom: 0.0 })
            .show(ui, |ui| {
                // Accent left bar.
                let (rect, _) = ui.allocate_exact_size(egui::vec2(2.0, 12.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, theme.divider);
                ui.horizontal(|ui| {
                    render_inline(ui, quote, theme, base_size, false, max_width - 12.0);
                });
            });
        return;
    }

    // Unordered list item: `- `, `* `, or `+ `.
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            render_list_item(ui, "\u{2022}", rest.trim_start(), theme, base_size, max_width);
            return;
        }
    }
    // Ordered list item: `1. `, `2. `, …
    if let Some(rest) = parse_ordered(trimmed) {
        let (num, body) = rest;
        render_list_item(ui, &format!("{num}."), body, theme, base_size, max_width);
        return;
    }

    // Plain paragraph.
    ui.set_max_width(max_width);
    render_inline(ui, trimmed, theme, base_size, false, max_width);
}

/// Parse a leading `1.`/`12.` ordered-list marker; returns `(number, rest)`.
fn parse_ordered(line: &str) -> Option<(u32, &str)> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i >= bytes.len() {
        return None;
    }
    if bytes[i] == b'.' {
        let num: u32 = line[..i].parse().ok()?;
        let rest = line[i + 1..].trim_start();
        Some((num, rest))
    } else {
        None
    }
}

fn parse_heading(line: &str) -> Option<(u32, &str)> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let after = &line[hashes..];
    if !after.starts_with(' ') && !after.is_empty() {
        return None;
    }
    Some((hashes as u32, after.trim_start()))
}

fn render_list_item(
    ui: &mut egui::Ui,
    marker: &str,
    body: &str,
    theme: &ThemeColors,
    base_size: f32,
    max_width: f32,
) {
    ui.set_max_width(max_width);
    ui.horizontal_top(|ui| {
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(marker)
                .color(theme.fg_dim)
                .size(base_size),
        );
        ui.add_space(4.0);
        render_inline(ui, body, theme, base_size, false, max_width - 18.0);
    });
}

/// Render one line's inline content, splitting on `**`, `` ` ``, `*`, and links.
fn render_inline(
    ui: &mut egui::Ui,
    text: &str,
    theme: &ThemeColors,
    size: f32,
    strong: bool,
    max_width: f32,
) {
    let spans = parse_spans(text);
    ui.set_max_width(max_width);
    ui.horizontal_wrapped(|ui| {
        for span in spans {
            let mut rt = egui::RichText::new(&span.text).size(size).color(theme.fg_primary);
            match span.style {
                Style::Bold => {
                    rt = rt.strong();
                }
                Style::Italic => {
                    rt = rt.italics();
                }
                Style::Code => {
                    // Inline code keeps the SAME point size as surrounding prose
                    // (only the font family changes to monospace) so all spans in
                    // a wrapped line share one line height and stay baseline-
                    // aligned. Tinting the glyphs + background marks it as code
                    // without disturbing the layout.
                    rt = rt
                        .monospace()
                        .color(theme.accent)
                        .background_color(theme.input_bg);
                }
                Style::Link => {
                    rt = rt.color(theme.accent).underline();
                }
                Style::Plain => {}
            }
            if strong && span.style != Style::Code {
                rt = rt.strong();
            }

            if let Some(href) = &span.href {
                let resp = ui
                    .add(egui::Label::new(rt).selectable(true).sense(egui::Sense::click()))
                    .on_hover_text(href);
                if resp.clicked() {
                    ui.ctx().open_url(egui::OpenUrl::same_tab(href));
                }
            } else {
                ui.add(egui::Label::new(rt).selectable(true));
            }
        }
    });
}

/// Split a line of Markdown into styled spans, resolving the three inline
/// markers: `**bold**`, `` `code` ``, `*italic*`, and `[text](url)`.
fn parse_spans(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut buf = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;

    let flush = |buf: &mut String, spans: &mut Vec<Span>| {
        if !buf.is_empty() {
            spans.push(Span { style: Style::Plain, text: std::mem::take(buf), href: None });
        }
    };

    while i < chars.len() {
        let c = chars[i];

        // Bold `**...**`
        if c == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            flush(&mut buf, &mut spans);
            i += 2;
            let mut inner = String::new();
            while i < chars.len() && !(chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '*') {
                inner.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 2; // consume closing `**`
                spans.push(Span { style: Style::Bold, text: inner, href: None });
            } else {
                // Unterminated: fall back to literal.
                spans.push(Span { style: Style::Plain, text: format!("**{inner}"), href: None });
            }
            continue;
        }

        // Inline code `` `...` ``
        if c == '`' {
            flush(&mut buf, &mut spans);
            i += 1;
            let mut inner = String::new();
            while i < chars.len() && chars[i] != '`' {
                inner.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1; // consume closing backtick
                spans.push(Span { style: Style::Code, text: inner, href: None });
            } else {
                spans.push(Span { style: Style::Plain, text: format!("`{inner}"), href: None });
            }
            continue;
        }

        // Italic `*...*`
        if c == '*' {
            flush(&mut buf, &mut spans);
            i += 1;
            let mut inner = String::new();
            while i < chars.len() && chars[i] != '*' {
                inner.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1;
                spans.push(Span { style: Style::Italic, text: inner, href: None });
            } else {
                spans.push(Span { style: Style::Plain, text: format!("*{inner}"), href: None });
            }
            continue;
        }

        // Link `[text](url)` — only when it starts a fresh token.
        if c == '[' {
            if let Some(rest) = text[i..].strip_prefix('[') {
                if let Some(close) = rest.find(']') {
                    let label = &rest[..close];
                    let after = &rest[close + 1..];
                    if let Some(url) = after.strip_prefix('(').and_then(|s| {
                        s.find(')').map(|p| &s[..p])
                    }) {
                        flush(&mut buf, &mut spans);
                        spans.push(Span {
                            style: Style::Link,
                            text: label.to_string(),
                            href: Some(url.to_string()),
                        });
                        // Advance past `[label](url)`.
                        i += close + 2 + url.len() + 1;
                        continue;
                    }
                }
            }
        }

        buf.push(c);
        i += 1;
    }
    flush(&mut buf, &mut spans);
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_heading_levels() {
        assert_eq!(parse_heading("# Title"), Some((1, "Title")));
        assert_eq!(parse_heading("### Sub"), Some((3, "Sub")));
        assert_eq!(parse_heading("not heading"), None);
        assert_eq!(parse_heading("#NoSpace"), None);
        assert_eq!(parse_heading("####### too many"), None);
    }

    #[test]
    fn parses_ordered_list_items() {
        assert_eq!(parse_ordered("1. first"), Some((1, "first")));
        assert_eq!(parse_ordered("12. twelve"), Some((12, "twelve")));
        assert_eq!(parse_ordered("no-num"), None);
        assert_eq!(parse_ordered("1 no dot"), None);
    }

    #[test]
    fn splits_bold_code_link_italic() {
        let spans = parse_spans("**bold** and `code` and [link](https://x.io) and *it*");
        let kinds: Vec<Style> = spans.iter().map(|s| s.style).collect();
        assert_eq!(
            kinds,
            vec![
                Style::Bold,
                Style::Plain,
                Style::Code,
                Style::Plain,
                Style::Link,
                Style::Plain,
                Style::Italic,
            ]
        );
        // The link carries its URL.
        let link = spans.iter().find(|s| s.style == Style::Link).unwrap();
        assert_eq!(link.text, "link");
        assert_eq!(link.href.as_deref(), Some("https://x.io"));
    }

    #[test]
    fn balks_at_unterminated_bold() {
        let spans = parse_spans("**unclosed");
        assert!(spans.iter().any(|s| s.style == Style::Plain && s.text == "**unclosed"));
    }

    #[test]
    fn detects_fence_lines() {
        assert!(is_fence_line("```sh"));
        assert!(is_fence_line("```"));
        assert!(is_fence_line("   ```rust"));
        assert!(!is_fence_line("echo hi"));
        assert!(!is_fence_line("code"));
        // A triple backtick only counts at the start; inline backticks don't.
        assert!(!is_fence_line("echo ``` not a fence"));
    }

    #[test]
    fn split_keeps_code_block_in_place() {
        // The motivating bug: a code block between two paragraphs used to be
        // re-appended to the bottom of the bubble. It must stay in place.
        let blocks = split_blocks("before\n```sh\nls -la\n```\nafter");
        assert_eq!(
            blocks,
            vec![
                MarkdownBlock::Text("before".into()),
                MarkdownBlock::Code("ls -la".into()),
                MarkdownBlock::Text("after".into()),
            ]
        );
    }

    #[test]
    fn split_handles_multiple_code_blocks_with_identical_content() {
        // Two code blocks with the *same* command must still be two distinct,
        // position-preserved blocks (this guards the ScrollArea id-clash fix).
        let blocks = split_blocks("a\n```sh\necho hi\n```\nb\n```\necho hi\n```\nc");
        assert_eq!(
            blocks,
            vec![
                MarkdownBlock::Text("a".into()),
                MarkdownBlock::Code("echo hi".into()),
                MarkdownBlock::Text("b".into()),
                MarkdownBlock::Code("echo hi".into()),
                MarkdownBlock::Text("c".into()),
            ]
        );
    }

    #[test]
    fn split_keeps_language_tag_out_of_code_body() {
        // The info string (`sh`) belongs to the fence, not the command.
        let blocks = split_blocks("```bash\necho hello\n```");
        assert_eq!(blocks, vec![MarkdownBlock::Code("echo hello".into())]);
    }

    #[test]
    fn split_drops_unterminated_code_block() {
        // No closing fence → the fenced content is not emitted as a code block.
        let blocks = split_blocks("before\n```sh\nnot closed");
        assert_eq!(blocks, vec![MarkdownBlock::Text("before".into())]);
    }

    #[test]
    fn split_trims_trailing_blank_line() {
        let blocks = split_blocks("hello\n");
        assert_eq!(blocks, vec![MarkdownBlock::Text("hello".into())]);
    }

    #[test]
    fn split_empty_input_yields_no_blocks() {
        assert!(split_blocks("").is_empty());
        assert!(split_blocks("\n\n").is_empty());
    }

    #[test]
    fn split_preserves_multiline_code_body() {
        let blocks = split_blocks("```sh\nline1\nline2\n```");
        assert_eq!(blocks, vec![MarkdownBlock::Code("line1\nline2".into())]);
    }
}
