//! # Lightweight Markdown rendering for the AI chat drawer
//!
//! A zero-dependency block + inline renderer for the subset of Markdown an
//! assistant reply is allowed to use: `**bold**`, `*italic*`, `` `inline
//! code` ``, `[text](url)` links, unordered/ordered lists, block quotes, and
//! `---` horizontal rules. ATX headings (`#`) and tables are NOT part of the
//! contract — the system prompt forbids them, and this renderer degrades both
//! gracefully (headings → bold lines, table rows → pipe-free text). Math is
//! written as LaTeX (`$...$` inline, `$$...$$` display) and converted to
//! plain Unicode. Fenced code blocks are handed to a
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
    /// LaTeX math already converted to Unicode by [`latex_to_unicode`].
    Math,
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
///
/// `context_menu` is attached to every selectable span so a right-click over
/// prose text offers the bubble's actions (copy / add-to-composer) — the
/// labels sit above the bubble frame in the hit test, so the frame alone
/// would only ever see right-clicks on the bubble's margins.
pub fn render_markdown(
    ui: &mut egui::Ui,
    text: &str,
    theme: &ThemeColors,
    base_size: f32,
    max_width: f32,
    mut code_block: impl FnMut(&mut egui::Ui, &str),
    mut context_menu: impl FnMut(&mut egui::Ui),
) {
    for block in split_blocks(text) {
        match block {
            MarkdownBlock::Text(s) => {
                // Line-level Markdown only ever saw the FIRST line of a
                // multi-line prose block (headings/lists mid-reply rendered
                // as raw text), so every line goes through the parser now.
                // A bare `$$` line toggles a display-math section whose
                // lines are collected and rendered as one formula.
                let mut in_display = false;
                let mut display_buf = String::new();
                let mut prev_kind = LineKind::Blank;
                for line in s.split('\n') {
                    let trimmed = line.trim();
                    if in_display {
                        if trimmed == "$$" {
                            in_display = false;
                            render_display_math(ui, &display_buf, theme, base_size, max_width, &mut context_menu);
                            display_buf.clear();
                        } else {
                            display_buf.push_str(trimmed);
                            display_buf.push(' ');
                        }
                        continue;
                    }
                    if trimmed == "$$" {
                        in_display = true;
                        display_buf.clear();
                        continue;
                    }
                    // Tables are deliberately unsupported (the prompt forbids
                    // them): alignment rules are dropped, data rows degrade to
                    // pipe-free "a · b" text so no raw `|` markup shows.
                    if looks_like_table_row(trimmed) {
                        if is_table_separator_row(trimmed) {
                            continue;
                        }
                        if !matches!(prev_kind, LineKind::Blank) {
                            ui.add_space(2.0);
                        }
                        ui.set_max_width(max_width);
                        render_inline(
                            ui,
                            &table_row_text(trimmed),
                            theme,
                            base_size,
                            false,
                            max_width,
                            &mut context_menu,
                        );
                        prev_kind = LineKind::Other;
                        continue;
                    }
                    if trimmed.is_empty() {
                        prev_kind = LineKind::Blank;
                        ui.add_space(4.0);
                        continue;
                    }
                    let kind = classify_line(trimmed);
                    // List items stay tight against each other; everything
                    // else gets a small paragraph gap (blank lines already
                    // spaced themselves).
                    let tight = prev_kind == LineKind::ListItem && kind == LineKind::ListItem;
                    if prev_kind != LineKind::Blank && !tight {
                        ui.add_space(2.0);
                    }
                    render_block(ui, trimmed, theme, base_size, max_width, &mut context_menu);
                    prev_kind = kind;
                }
                if in_display && !display_buf.trim().is_empty() {
                    render_display_math(ui, &display_buf, theme, base_size, max_width, &mut context_menu);
                }
            }
            MarkdownBlock::Code(c) => {
                ui.add_space(4.0);
                code_block(ui, &c);
                ui.add_space(4.0);
            }
        }
    }
}

/// What a single prose line is, for spacing decisions (list items stay tight).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Blank,
    ListItem,
    Other,
}

fn classify_line(trimmed: &str) -> LineKind {
    let is_unordered = ["- ", "* ", "+ "].iter().any(|m| trimmed.starts_with(m));
    if is_unordered || parse_ordered(trimmed).is_some() {
        LineKind::ListItem
    } else {
        LineKind::Other
    }
}

/// Render an accumulated display-math section as one formula line.
fn render_display_math(
    ui: &mut egui::Ui,
    latex: &str,
    theme: &ThemeColors,
    base_size: f32,
    max_width: f32,
    context_menu: &mut dyn FnMut(&mut egui::Ui),
) {
    let converted = latex_to_unicode(latex.trim());
    ui.set_max_width(max_width);
    ui.add(egui::Label::new(
        egui::RichText::new(converted)
            .monospace()
            .size(base_size)
            .color(theme.fg_primary),
    ))
    .context_menu(|ui| context_menu(ui));
}

fn render_block(
    ui: &mut egui::Ui,
    line: &str,
    theme: &ThemeColors,
    base_size: f32,
    max_width: f32,
    context_menu: &mut dyn FnMut(&mut egui::Ui),
) {
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

    // ATX headings are banned for assistant replies (the system prompt
    // forbids them). If one still arrives, degrade to a bold body-size line
    // rather than showing raw `#` markup.
    if let Some((_, rest)) = parse_heading(trimmed) {
        ui.set_max_width(max_width);
        render_inline(ui, rest, theme, base_size, true, max_width, context_menu);
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
                    render_inline(ui, quote, theme, base_size, false, max_width - 12.0, context_menu);
                });
            });
        return;
    }

    // Unordered list item: `- `, `* `, or `+ `.
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            render_list_item(ui, "\u{2022}", rest.trim_start(), theme, base_size, max_width, context_menu);
            return;
        }
    }
    // Ordered list item: `1. `, `2. `, …
    if let Some(rest) = parse_ordered(trimmed) {
        let (num, body) = rest;
        render_list_item(ui, &format!("{num}."), body, theme, base_size, max_width, context_menu);
        return;
    }

    // Plain paragraph.
    ui.set_max_width(max_width);
    render_inline(ui, trimmed, theme, base_size, false, max_width, context_menu);
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
    context_menu: &mut dyn FnMut(&mut egui::Ui),
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
        render_inline(ui, body, theme, base_size, false, max_width - 18.0, context_menu);
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
    context_menu: &mut dyn FnMut(&mut egui::Ui),
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
                Style::Math => {
                    // Monospace sets formulas apart from prose; the glyphs are
                    // plain Unicode so no KaTeX font is needed.
                    rt = rt.monospace();
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
                resp.context_menu(|ui| context_menu(ui));
            } else {
                ui.add(egui::Label::new(rt).selectable(true))
                    .context_menu(|ui| context_menu(ui));
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

        // Math `$...$` and single-line `$$...$$` → converted Unicode. On a
        // failed match (currency, unbalanced) only ONE `$` is emitted so the
        // next `$` gets its own chance as an opener.
        if c == '$' {
            let display = i + 1 < chars.len() && chars[i + 1] == '$';
            let mut j = i + if display { 2 } else { 1 };
            let mut inner = String::new();
            while j < chars.len() {
                if chars[j] == '$' && (!display || (j + 1 < chars.len() && chars[j + 1] == '$')) {
                    break;
                }
                inner.push(chars[j]);
                j += 1;
            }
            if j < chars.len() && is_mathish(&inner) {
                flush(&mut buf, &mut spans);
                spans.push(Span { style: Style::Math, text: latex_to_unicode(&inner), href: None });
                i = if display { j + 2 } else { j + 1 };
            } else {
                buf.push('$');
                i += 1;
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

/// Heuristic: does the text between `$` delimiters look like math rather than
/// currency or prose? "$5 and $10" must stay literal. Requires non-space right
/// inside BOTH delimiters, plus an explicit LaTeX signal (backslash / `^` / `_`
/// / braces), or a compact token containing a letter, or a spaced expression
/// with a real operator.
fn is_mathish(content: &str) -> bool {
    if content.is_empty()
        || content.starts_with(char::is_whitespace)
        || content.ends_with(char::is_whitespace)
    {
        return false;
    }
    if content.contains(['\\', '^', '_', '{', '}']) {
        return true;
    }
    let has_alpha = content.chars().any(char::is_alphabetic);
    if !content.contains(char::is_whitespace) {
        // Compact token: $x$, $n+1$, $dx$ — but not $5-$ or $3.50$.
        return has_alpha && !content.chars().all(|c| c.is_ascii_digit() || "+-.,()".contains(c));
    }
    // Spaced math like `$a + b$` / `$2 + 2 = 4$` needs a real operator.
    content.contains(['=', '+', '<', '>'])
        && (has_alpha || content.chars().any(|c| c.is_ascii_digit()))
}

/// Convert a LaTeX math fragment to a plain-Unicode approximation: Greek
/// letters, operators/arrows/relations, `\frac`, `\sqrt`, super/subscripts and
/// common text commands. Anything unrecognized degrades to a readable literal
/// (the bare word, or `^{…}` with the marker kept) rather than hiding content.
/// Good enough for chat prose — a rendering nicety, not a TeX engine.
fn latex_to_unicode(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            if let Some(&n) = chars.get(i + 1) {
                if !n.is_ascii_alphabetic() {
                    // Escaped literals (`\%`, `\$`, …) and spacing macros.
                    match n {
                        ',' | ';' | ':' | ' ' => out.push(' '),
                        '!' => {}
                        '\\' => out.push_str(" | "),
                        _ => out.push(n),
                    }
                    i += 2;
                    continue;
                }
            }
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && chars[end].is_ascii_alphabetic() {
                end += 1;
            }
            let cmd: String = chars[start..end].iter().collect();
            i = end;
            match cmd.as_str() {
                "frac" => {
                    let (a, ni) = read_group(&chars, i);
                    let (b, ni) = read_group(&chars, ni);
                    i = ni;
                    out.push_str(&frac_str(&latex_to_unicode(&a), &latex_to_unicode(&b)));
                }
                "sqrt" => {
                    let mut ni = i;
                    if chars.get(ni) == Some(&'[') {
                        // Optional root degree: \sqrt[3]{x} → ³√(x)
                        let mut j = ni + 1;
                        let mut deg = String::new();
                        while j < chars.len() && chars[j] != ']' {
                            deg.push(chars[j]);
                            j += 1;
                        }
                        ni = (j + 1).min(chars.len());
                        if let Some(s) = to_superscript(&latex_to_unicode(&deg)) {
                            out.push_str(&s);
                        }
                    }
                    let (g, gi) = read_group(&chars, ni);
                    i = gi;
                    out.push_str("√(");
                    out.push_str(&latex_to_unicode(&g));
                    out.push(')');
                }
                "text" | "mathrm" | "operatorname" | "textbf" | "mathit" => {
                    let (g, gi) = read_group(&chars, i);
                    i = gi;
                    out.push_str(&g);
                }
                "mathbb" => {
                    let (g, gi) = read_group(&chars, i);
                    i = gi;
                    out.push_str(&double_struck(&g));
                }
                "begin" | "end" => {
                    // Environment tag: drop the tag, keep converting the rest.
                    let (_g, gi) = read_group(&chars, i);
                    i = gi;
                }
                "left" | "right" => {
                    // Big-delimiter prefix: keep the delimiter itself.
                    if let Some(&d) = chars.get(i) {
                        if d != '.' && !d.is_ascii_alphabetic() {
                            out.push(d);
                            i += 1;
                        }
                    }
                }
                _ => {
                    if let Some(sym) = lookup_symbol(&cmd) {
                        out.push_str(sym);
                    } else {
                        // Unknown command: keep the word (readable degradation).
                        out.push_str(&cmd);
                    }
                }
            }
            continue;
        }
        if c == '^' || c == '_' {
            let mut ni = i + 1;
            let raw: String = if chars.get(ni) == Some(&'{') {
                let (g, gi) = read_group(&chars, ni);
                ni = gi;
                g
            } else if chars.get(ni) == Some(&'\\') {
                // Script of a command: ^\infty — the command (with its
                // backslash, so it converts to a symbol) is the argument.
                let start = ni + 1;
                let mut end = start;
                while end < chars.len() && chars[end].is_ascii_alphabetic() {
                    end += 1;
                }
                ni = end;
                format!("\\{}", chars[start..end].iter().collect::<String>())
            } else {
                let (g, gi) = read_group(&chars, ni);
                ni = gi;
                g
            };
            i = ni;
            let converted = latex_to_unicode(&raw);
            let mapped = if c == '^' {
                to_superscript(&converted)
            } else {
                to_subscript(&converted)
            };
            match mapped {
                Some(s) => out.push_str(&s),
                None => {
                    out.push(c);
                    out.push_str(&converted);
                }
            }
            continue;
        }
        if c == '{' || c == '}' {
            // Bare grouping braces carry no meaning in plain text.
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// LaTeX command → Unicode symbol table (linear scan; called per token only).
const LATEX_SYMBOLS: &[(&str, &str)] = &[
    // Greek
    ("alpha", "α"), ("beta", "β"), ("gamma", "γ"), ("delta", "δ"),
    ("epsilon", "ε"), ("varepsilon", "ε"), ("zeta", "ζ"), ("eta", "η"),
    ("theta", "θ"), ("vartheta", "ϑ"), ("iota", "ι"), ("kappa", "κ"),
    ("lambda", "λ"), ("mu", "μ"), ("nu", "ν"), ("xi", "ξ"), ("pi", "π"),
    ("rho", "ρ"), ("sigma", "σ"), ("tau", "τ"), ("upsilon", "υ"),
    ("phi", "φ"), ("varphi", "ϕ"), ("chi", "χ"), ("psi", "ψ"), ("omega", "ω"),
    ("Gamma", "Γ"), ("Delta", "Δ"), ("Theta", "Θ"), ("Lambda", "Λ"),
    ("Xi", "Ξ"), ("Pi", "Π"), ("Sigma", "Σ"), ("Phi", "Φ"), ("Psi", "Ψ"),
    ("Omega", "Ω"),
    // Operators
    ("times", "×"), ("div", "÷"), ("pm", "±"), ("mp", "∓"), ("cdot", "·"),
    ("ast", "∗"), ("star", "⋆"), ("circ", "∘"), ("bullet", "•"),
    ("oplus", "⊕"), ("otimes", "⊗"), ("perp", "⊥"), ("parallel", "∥"),
    ("angle", "∠"), ("degree", "°"),
    // Relations
    ("leq", "≤"), ("le", "≤"), ("geq", "≥"), ("ge", "≥"), ("neq", "≠"),
    ("ne", "≠"), ("approx", "≈"), ("equiv", "≡"), ("sim", "∼"),
    ("simeq", "≃"), ("cong", "≅"), ("propto", "∝"),
    // Arrows
    ("to", "→"), ("rightarrow", "→"), ("leftarrow", "←"), ("gets", "←"),
    ("leftrightarrow", "↔"), ("Rightarrow", "⇒"), ("Leftarrow", "⇐"),
    ("Leftrightarrow", "⇔"), ("uparrow", "↑"), ("downarrow", "↓"),
    ("mapsto", "↦"),
    // Sets & logic
    ("in", "∈"), ("notin", "∉"), ("ni", "∋"), ("subset", "⊂"),
    ("subseteq", "⊆"), ("supset", "⊃"), ("supseteq", "⊇"), ("cup", "∪"),
    ("cap", "∩"), ("emptyset", "∅"), ("varnothing", "∅"), ("forall", "∀"),
    ("exists", "∃"), ("nexists", "∄"), ("neg", "¬"), ("lnot", "¬"),
    ("land", "∧"), ("wedge", "∧"), ("lor", "∨"), ("vee", "∨"),
    ("therefore", "∴"), ("because", "∵"),
    // Calculus & misc
    ("infty", "∞"), ("partial", "∂"), ("nabla", "∇"), ("hbar", "ℏ"),
    ("ell", "ℓ"), ("aleph", "ℵ"), ("sum", "∑"), ("prod", "∏"),
    ("coprod", "∐"), ("int", "∫"), ("iint", "∬"), ("iiint", "∭"),
    ("oint", "∮"), ("prime", "′"), ("dagger", "†"), ("ddagger", "‡"),
    ("checkmark", "✓"), ("ldots", "…"), ("cdots", "…"), ("dots", "…"),
    ("vdots", "⋮"), ("quad", " "), ("qquad", " "),
    // Multi-letter operators (rendered as plain words)
    ("lim", "lim"), ("log", "log"), ("ln", "ln"), ("exp", "exp"),
    ("sin", "sin"), ("cos", "cos"), ("tan", "tan"), ("cot", "cot"),
    ("sec", "sec"), ("csc", "csc"), ("arcsin", "arcsin"), ("arccos", "arccos"),
    ("arctan", "arctan"), ("min", "min"), ("max", "max"), ("gcd", "gcd"),
    ("det", "det"), ("deg", "deg"),
];

fn lookup_symbol(cmd: &str) -> Option<&'static str> {
    LATEX_SYMBOLS.iter().find(|(name, _)| *name == cmd).map(|(_, sym)| *sym)
}

/// Read the argument at `i`: a braced group (nesting-aware) or a single char.
/// Returns `(content, index_after)`; at end of input returns an empty group.
fn read_group(chars: &[char], i: usize) -> (String, usize) {
    if i >= chars.len() {
        return (String::new(), i);
    }
    if chars[i] != '{' {
        return (chars[i].to_string(), i + 1);
    }
    let mut depth = 0usize;
    let mut inner = String::new();
    let mut j = i;
    while j < chars.len() {
        let c = chars[j];
        if c == '{' {
            depth += 1;
            if depth == 1 {
                j += 1;
                continue;
            }
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                j += 1;
                break;
            }
        }
        inner.push(c);
        j += 1;
    }
    (inner, j)
}

/// `\frac{a}{b}` → `a/b`, parenthesizing compound operands.
fn frac_str(a: &str, b: &str) -> String {
    let wrap = |s: &str| {
        if s.chars().count() > 1 && s.chars().any(|c| !c.is_alphanumeric()) {
            format!("({s})")
        } else {
            s.to_string()
        }
    };
    format!("{}/{}", wrap(a), wrap(b))
}

/// `\mathbb{R}` → ℝ for the common number-system letters.
fn double_struck(g: &str) -> String {
    g.chars()
        .map(|c| match c {
            'N' => 'ℕ',
            'Z' => 'ℤ',
            'Q' => 'ℚ',
            'R' => 'ℝ',
            'C' => 'ℂ',
            'H' => 'ℍ',
            'P' => 'ℙ',
            'E' => '𝔼',
            other => other,
        })
        .collect()
}

/// Map every char to its Unicode superscript, or `None` if any char is
/// unmappable (the caller then keeps the literal `^{…}` form).
fn to_superscript(s: &str) -> Option<String> {
    s.chars()
        .map(|c| Some(match c {
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            '+' => '⁺',
            '-' => '⁻',
            '=' => '⁼',
            '(' => '⁽',
            ')' => '⁾',
            'n' => 'ⁿ',
            'i' => 'ⁱ',
            'a' => 'ᵃ',
            'b' => 'ᵇ',
            'c' => 'ᶜ',
            'd' => 'ᵈ',
            'e' => 'ᵉ',
            'f' => 'ᶠ',
            'g' => 'ᵍ',
            'h' => 'ʰ',
            'j' => 'ʲ',
            'k' => 'ᵏ',
            'l' => 'ˡ',
            'm' => 'ᵐ',
            'o' => 'ᵒ',
            'p' => 'ᵖ',
            'r' => 'ʳ',
            's' => 'ˢ',
            't' => 'ᵗ',
            'u' => 'ᵘ',
            'v' => 'ᵛ',
            'w' => 'ʷ',
            'x' => 'ˣ',
            'y' => 'ʸ',
            'z' => 'ᶻ',
            _ => return None,
        }))
        .collect()
}

/// Map every char to its Unicode subscript, or `None` if any char is
/// unmappable (the caller then keeps the literal `_{…}` form).
fn to_subscript(s: &str) -> Option<String> {
    s.chars()
        .map(|c| Some(match c {
            '0' => '₀',
            '1' => '₁',
            '2' => '₂',
            '3' => '₃',
            '4' => '₄',
            '5' => '₅',
            '6' => '₆',
            '7' => '₇',
            '8' => '₈',
            '9' => '₉',
            '+' => '₊',
            '-' => '₋',
            '=' => '₌',
            '(' => '₍',
            ')' => '₎',
            'a' => 'ₐ',
            'e' => 'ₑ',
            'h' => 'ₕ',
            'i' => 'ᵢ',
            'j' => 'ⱼ',
            'k' => 'ₖ',
            'l' => 'ₗ',
            'm' => 'ₘ',
            'n' => 'ₙ',
            'o' => 'ₒ',
            'p' => 'ₚ',
            'r' => 'ᵣ',
            's' => 'ₛ',
            't' => 'ₜ',
            'u' => 'ᵤ',
            'v' => 'ᵥ',
            'x' => 'ₓ',
            _ => return None,
        }))
        .collect()
}

/// A fully-piped table row, e.g. `| a | b |`. Tables are not part of the
/// reply contract, so these lines degrade to readable text.
fn looks_like_table_row(trimmed: &str) -> bool {
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.chars().count() >= 2
}

/// The `| --- | :---: |` alignment rule row of a table.
fn is_table_separator_row(trimmed: &str) -> bool {
    trimmed.chars().all(|c| matches!(c, '|' | ':' | '-' | ' ')) && trimmed.contains('-')
}

/// `| a | b |` → `a · b` — pipe-free and readable.
fn table_row_text(trimmed: &str) -> String {
    trimmed
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
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

    #[test]
    fn classifies_list_lines() {
        assert_eq!(classify_line("- bullet"), LineKind::ListItem);
        assert_eq!(classify_line("* star"), LineKind::ListItem);
        assert_eq!(classify_line("1. step"), LineKind::ListItem);
        assert_eq!(classify_line("just prose"), LineKind::Other);
    }

    #[test]
    fn math_inline_parses_and_converts() {
        let spans = parse_spans("so $x$ then $E=mc^2$");
        let kinds: Vec<Style> = spans.iter().map(|s| s.style).collect();
        assert_eq!(kinds, vec![Style::Plain, Style::Math, Style::Plain, Style::Math]);
        assert_eq!(spans[1].text, "x");
        assert_eq!(spans[3].text, "E=mc²");
    }

    #[test]
    fn display_math_inline_parses() {
        let spans = parse_spans("$$a+b$$");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style, Style::Math);
        assert_eq!(spans[0].text, "a+b");
    }

    #[test]
    fn currency_stays_literal() {
        let text = "costs $5 and $10 total";
        let spans = parse_spans(text);
        assert!(spans.iter().all(|s| s.style == Style::Plain));
        let joined: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, text);
    }

    #[test]
    fn latex_converts_symbols_and_structures() {
        assert_eq!(latex_to_unicode("\\alpha \\times \\beta"), "α × β");
        assert_eq!(latex_to_unicode("\\frac{1}{2}"), "1/2");
        assert_eq!(latex_to_unicode("\\frac{a+b}{c}"), "(a+b)/c");
        assert_eq!(latex_to_unicode("\\frac{dx}{dt}"), "dx/dt");
        assert_eq!(latex_to_unicode("\\sqrt{2}"), "√(2)");
        assert_eq!(latex_to_unicode("\\sqrt[3]{x}"), "³√(x)");
        assert_eq!(latex_to_unicode("x^{10}"), "x¹⁰");
        assert_eq!(latex_to_unicode("x_{i+1}"), "xᵢ₊₁");
        assert_eq!(latex_to_unicode("\\int_0^\\infty"), "∫₀^∞");
        assert_eq!(latex_to_unicode("\\mathbb{R}"), "ℝ");
        // Unmappable script degrades to a readable literal, not dropped text.
        assert_eq!(latex_to_unicode("2^{99}"), "2⁹⁹");
    }

    #[test]
    fn table_rows_degrade_and_rules_drop() {
        assert!(looks_like_table_row("| a | b |"));
        assert!(!looks_like_table_row("- item"));
        assert!(is_table_separator_row("| --- | :---: |"));
        assert!(!is_table_separator_row("| a | b |"));
        assert_eq!(table_row_text("| a | b |"), "a · b");
        assert_eq!(table_row_text("| 1 | two |"), "1 · two");
    }
}
