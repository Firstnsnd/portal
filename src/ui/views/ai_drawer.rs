//! # AI chat drawer + draggable orb
//!
//! The orb sits in the bottom-right of a terminal tab; clicking it opens a
//! right-hand chat drawer that talks to the configured LLM (streaming) and can
//! run extracted shell commands in the tab's focused terminal.

use eframe::egui;

use crate::llm::types::{ChatMessage, ChatRole, ExecutionEnvironment};
use crate::ui::state::ai_chat::{spawn_chat, AiChatState};
use crate::ui::pane::AppWindow;
use crate::ui::pane_view::WindowContext;
use crate::ui::tokens as tk;
use super::ai_markdown::render_markdown;

/// Radius (px) of the draggable orb.
pub const ORB_RADIUS: f32 = 15.0;
/// Offset from the tab area's bottom-right corner.
pub const ORB_MARGIN: f32 = 20.0;

/// Horizontal inset used by the drawer content so text never touches the edge.
const PAD: f32 = 16.0;
/// Vertical space between consecutive chat entries so bubbles don't stick together.
const BUBBLE_GAP: f32 = 14.0;
/// Fraction of the drawer's content width a bubble is allowed to span before
/// wrapping. Both user-sent and assistant-reply bubbles share this so their
/// horizontal blank gaps stay consistent. Less than 1.0 gives the bubble a
/// little breathing room on the far side; short content still shrink-wraps.
const BUBBLE_WIDTH_FRAC: f32 = 0.82;
/// Horizontal padding the bubble frame adds on both sides (left + right).
const BUBBLE_PAD_X2: f32 = 28.0;
/// Visible composer rows; longer drafts scroll inside the composer box.
const COMPOSER_EDIT_ROWS: usize = 2;
/// egui's default multiline [`egui::TextEdit`] vertical margin is
/// `Margin::symmetric(4, 2)`, so the edit box is `rows × row_height + 4px`
/// tall. The surrounding ScrollArea cap must include it or the bottom of the
/// box gets clipped.
const TEXT_EDIT_V_MARGIN: f32 = 4.0;

/// Outer height of the composer's edit box: `COMPOSER_EDIT_ROWS` rows of text
/// plus the TextEdit's default vertical margin.
fn composer_edit_height(ui: &egui::Ui) -> f32 {
    let row_h = ui.fonts(|f| f.row_height(&egui::FontId::proportional(tk::FONT_BASE)));
    COMPOSER_EDIT_ROWS as f32 * row_h + TEXT_EDIT_V_MARGIN
}

/// Max width a bubble may span, shared by user-sent and assistant-reply bubbles
/// so wrapped lines leave the same trailing blank on both. Short content still
/// shrink-wraps below this cap.
fn bubble_max_width(ui: &egui::Ui) -> f32 {
    ui.available_width() * BUBBLE_WIDTH_FRAC
}

/// Render the AI chat drawer on the right side of a window (if open), plus the
/// "run" confirmation window. Called from `AppWindow::render_drawers`.
pub fn render_ai_drawer(window: &mut AppWindow, ctx: &egui::Context, cx: &mut WindowContext) {
    let theme: &crate::ui::ThemeColors = cx.theme;
    let lang = cx.language;
    let llm_config = cx.llm_config.clone();

    let active = window.active_tab;
    // Stable id for the composer TextEdit — the terminal panes check who
    // holds keyboard focus via this id (see render_terminal_session).
    // Computed before `tab` borrows the window mutably.
    let composer_id = window.ai_composer_id();
    let Some(tab) = window.tabs.get_mut(active) else { return };
    let handle = tab.ai_chat.clone();

    // Move a confirmed-run command out to `pending_ai_command` (consumed next
    // frame by the terminal runner, mirroring `pending_snippet`), and bail out
    // early when the drawer is closed.
    {
        let mut st = handle.lock().unwrap();
        if let Some(cmd) = st.pending_run.take() {
            tab.pending_ai_command = Some(cmd);
        }
    }
    let open = handle.lock().map(|s| s.open).unwrap_or(false);
    if !open {
        return;
    }

    let mut do_send: Option<String> = None;

    // Width is a fraction of the window so the terminal stays visible beside the
    // chat. Reserve at least MIN_TERMINAL_PX so the terminal is never swallowed.
    const MIN_TERMINAL_PX: f32 = 260.0;
    let screen_w = ctx.screen_rect().width();
    let drawer_width = (screen_w * 0.30)
        .clamp(280.0, 460.0)
        .min((screen_w - MIN_TERMINAL_PX).max(240.0));

    egui::SidePanel::right("ai_drawer")
        .exact_width(drawer_width)
        .resizable(false)
        .frame(drawer_frame(theme))
        .show(ctx, |ui| {
            draw_header(ui, theme, lang, &llm_config, &handle);
            ui.separator();

            let mut run_wanted: Option<String> = None;
            let mut composer_wanted: Option<String> = None;
            let mut composer_text = handle.lock().unwrap().input.clone();

            // Composer docked at the bottom FIRST: the dock claims its natural
            // content height (measured by egui — no hand-computed height
            // budget), so it is always fully visible, and everything above it
            // belongs to the messages.
            egui::TopBottomPanel::bottom("ai_composer_dock")
                .resizable(false)
                .frame(egui::Frame::none())
                .show_inside(ui, |ui| {
                    draw_composer(ui, theme, lang, &handle, &mut composer_text, &mut do_send, composer_id);
                });

            // Messages take exactly the space left above the composer dock.
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    let st = handle.lock().unwrap();
                    if st.messages.is_empty() && st.streaming.is_none() {
                        draw_empty_state(ui, theme, lang);
                    }
                    for msg in &st.messages {
                        draw_bubble(ui, msg, theme, lang, &mut run_wanted, &mut composer_wanted);
                    }
                    if let Some(buf) = &st.streaming {
                        if buf.text.is_empty() {
                            // No token arrived yet — show the animated
                            // "thinking" bubble instead of an empty static one.
                            draw_thinking_bubble(ui, theme);
                        } else {
                            draw_streaming_bubble(ui, &buf.text, theme);
                        }
                    } else if st.running {
                        draw_thinking_bubble(ui, theme);
                    }
                    if let Some(err) = &st.error {
                        draw_error(ui, theme, lang, err);
                    }
                    ui.add_space(8.0);
                });

            // If a Run button was clicked, ask for confirmation.
            if let Some(cmd) = run_wanted {
                if let Ok(mut st) = handle.lock() { st.confirm_run = Some(cmd); }
            }
            // A bubble's right-click menu asked to drop the message into the
            // composer draft (newline-joined onto any existing text). The
            // composer already persisted its draft this frame, so append into
            // the persisted state — visible on the next frame.
            if let Some(text) = composer_wanted {
                if let Ok(mut st) = handle.lock() {
                    if !st.input.is_empty() {
                        st.input.push('\n');
                    }
                    st.input.push_str(&text);
                }
            }
        });

    // Send the message (outside the panel borrow).
    if let Some(text) = do_send {
        let mut st = handle.lock().unwrap();
        st.push_user(text);
        st.input.clear();
        let messages = st.messages.clone();
        drop(st);

        // The command will run in this tab's focused session, so describe *that*
        // environment (local host, or remote host + probed os/arch/shell).
        let env = tab
            .sessions
            .get(tab.focused_session)
            .map(|s| {
                let mut e = s.kind.environment_summary();
                if let Some(cwd) = &s.cwd {
                    e.cwd = cwd.clone();
                }
                e
            })
            .unwrap_or_else(ExecutionEnvironment::local);

        spawn_chat(llm_config, String::new(), messages, env, handle.clone());
    }

    // Confirmation window for a pending "Run" command.
    render_run_confirm(ctx, theme, lang, &handle);
}

// ── Sub-renderers ────────────────────────────────────────────────────────────

fn drawer_frame(theme: &crate::ui::ThemeColors) -> egui::Frame {
    egui::Frame {
        fill: theme.bg_elevated,
        inner_margin: egui::Margin::ZERO,
        rounding: egui::Rounding::ZERO,
        shadow: egui::epaint::Shadow {
            offset: egui::vec2(-6.0, 0.0),
            blur: 24.0,
            spread: 2.0,
            color: egui::Color32::from_black_alpha(40),
        },
        stroke: egui::Stroke::NONE,
        ..Default::default()
    }
}

fn draw_header(
    ui: &mut egui::Ui,
    theme: &crate::ui::ThemeColors,
    lang: crate::ui::Language,
    llm_config: &crate::llm::types::LlmConfig,
    handle: &std::sync::Arc<std::sync::Mutex<AiChatState>>,
) {
    egui::Frame::none()
        .inner_margin(egui::Margin { left: PAD, right: 12.0, top: 14.0, bottom: 12.0 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Accent dot + title
                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 4.0, theme.accent);
                        ui.label(
                            egui::RichText::new(lang.t("ai_title"))
                                .color(theme.fg_primary)
                                .size(tk::FONT_BASE)
                                .strong(),
                        );
                    });
                    if !llm_config.model.is_empty() {
                        ui.label(
                            egui::RichText::new(&llm_config.model)
                                .color(theme.fg_dim)
                                .size(tk::FONT_XS),
                        );
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(
                        egui::Button::new(
                            egui::RichText::new("\u{2715}").color(theme.fg_dim).size(15.0),
                        )
                        .frame(false),
                    ).clicked() {
                        if let Ok(mut st) = handle.lock() { st.open = false; }
                    }
                });
            });
        });
}

fn draw_empty_state(ui: &mut egui::Ui, theme: &crate::ui::ThemeColors, lang: crate::ui::Language) {
    ui.add_space(16.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new("\u{2726}").color(theme.accent_alpha(140)).size(28.0));
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(lang.t("ai_input_placeholder"))
                .color(theme.fg_dim)
                .size(tk::FONT_MD),
        );
    });
    ui.add_space(12.0);
}

fn draw_streaming_bubble(ui: &mut egui::Ui, text: &str, theme: &crate::ui::ThemeColors) {
    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
        egui::Frame::none()
            .inner_margin(egui::Margin { left: PAD, right: PAD, top: 0.0, bottom: 0.0 })
            .show(ui, |ui| {
                bubble_frame(theme.bg_secondary).show(ui, |ui| {
                    // Lay out a break-anywhere galley so the bubble
                    // shrink-wraps to its actual content width (soft-wrap) —
                    // a plain wrapping Label fills the whole available width
                    // even when the wrapped text's widest line is narrower,
                    // which left a wide blank gap on the right.
                    let max_w = bubble_max_width(ui);
                    let mut job = egui::text::LayoutJob::simple(
                        text.to_owned(),
                        egui::FontId::proportional(tk::FONT_BASE),
                        theme.fg_primary,
                        max_w,
                    );
                    job.wrap.break_anywhere = true;
                    job.wrap.max_width = max_w;
                    let galley = ui.fonts(|f| f.layout_job(job));
                    ui.set_max_width(max_w);
                    ui.add(
                        egui::Label::new(egui::WidgetText::Galley(galley)).selectable(true),
                    );
                });
            });
        ui.add_space(BUBBLE_GAP);
    });
}

/// A subtle "assistant is typing…" bubble with three animated accent dots,
/// replacing the default spinner while a reply is awaited (before any token
/// arrives, at which point `draw_streaming_bubble` takes over).
fn draw_thinking_bubble(ui: &mut egui::Ui, theme: &crate::ui::theme::ThemeColors) {
    let now = ui.input(|i| i.time);
    // The app repaints on activity, not a fixed 60 FPS, so while waiting for the
    // first token we must keep asking for repaints or the dots freeze on frame one.
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
        egui::Frame::none()
            .inner_margin(egui::Margin { left: PAD, right: PAD, top: 0.0, bottom: 0.0 })
            .show(ui, |ui| {
                bubble_frame(theme.bg_secondary).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("\u{2726}").color(theme.accent).size(tk::FONT_XS),
                        );
                        ui.add_space(4.0);
                        // Three accent dots bouncing vertically in a staggered
                        // wave — each shifts up/down on a sine, offset per index.
                        for i in 0..3u32 {
                            let bounce = (now * 5.0 + i as f64 * 0.9).sin();
                            let y_off = (bounce * 2.5) as f32;
                            ui.add_space(2.0);
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(6.0, 12.0),
                                egui::Sense::hover(),
                            );
                            let center = egui::pos2(
                                rect.center().x,
                                rect.center().y + y_off,
                            );
                            ui.painter().circle_filled(center, 3.0, theme.accent);
                        }
                        ui.add_space(8.0);
                    });
                });
            });
        ui.add_space(BUBBLE_GAP);
    });
}

/// Render an error line clearly, translated for the sentinel `no_api_key` case.
fn draw_error(
    ui: &mut egui::Ui,
    theme: &crate::ui::ThemeColors,
    lang: crate::ui::Language,
    err: &str,
) {
    ui.add_space(4.0);
    let text = if err == "no_api_key" { lang.t("ai_no_key_configured") } else { err };
    egui::Frame::none()
        .inner_margin(egui::Margin { left: PAD, right: PAD, top: 0.0, bottom: 0.0 })
        .show(ui, |ui| {
            egui::Frame::none()
                .fill(theme.error_dim)
                .rounding(egui::Rounding::same(tk::RADIUS_SM))
                .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                .show(ui, |ui| {
                    // No explicit width cap: this Ui is already inset by both
                    // frames' margins, and labels wrap at the available width.
                    ui.label(egui::RichText::new(text).color(theme.red).size(tk::FONT_SM));
                });
        });
}

/// Render a completed message bubble. Assistant replies show "Run" buttons under
/// fenced code blocks (writing the chosen command into `run_wanted`); any
/// bubble's right-click menu offers copy / add-to-composer (`composer_wanted`).
fn draw_bubble(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    theme: &crate::ui::theme::ThemeColors,
    lang: crate::ui::Language,
    run_wanted: &mut Option<String>,
    composer_wanted: &mut Option<String>,
) {
    let is_user = msg.role == ChatRole::User;
    let bg = if is_user {
        theme.accent_alpha(32)
    } else {
        theme.bg_secondary
    };
    // Assistant bubbles carry a small role label; user bubbles stay minimal.
    let label = if is_user { "" } else { "\u{2726}" };

    // Measure the content's natural (unwrapped) width so a short message
    // shrink-wraps its bubble instead of filling the row.
    let content_w = measure_bubble_content(ui, msg, label);

    // The bubble frame adds 14px padding on each side. Content wraps at the
    // bubble's *inner* width, not the measured unwrapped width: for a long
    // message the bubble is capped at a fraction of the drawer width (so a
    // user bubble on the right still keeps a gap from the left edge, and an
    // assistant bubble on the left keeps one from the right) and the text
    // soft-wraps inside it instead of clipping.
    let max_w = bubble_max_width(ui) - BUBBLE_PAD_X2;
    let wrap_w = content_w.min(max_w);
    let w = wrap_w + BUBBLE_PAD_X2;

    // User bubbles align RIGHT (like ChatGPT/WeChat); assistant bubbles align
    // LEFT. Shrink-wrapping is done by allocating the frame exactly `w` wide,
    // because a filling frame under `Align::Max`/`Min` otherwise stretches to
    // the full row regardless of content. A trailing `PAD` keeps the bubble off
    // the drawer edge on its aligned side.
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 0.0),
        if is_user {
            egui::Layout::right_to_left(egui::Align::Min)
        } else {
            egui::Layout::left_to_right(egui::Align::Min)
        },
        |ui| {
            ui.add_space(PAD);
            ui.allocate_ui_with_layout(
                egui::vec2(w, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_width(w);
                    bubble_frame(bg).show(ui, |ui| {
                        // Text always reads left-aligned, even inside the
                        // right-floating user bubble (else every wrapped line
                        // would right-align and leave a ragged left edge).
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_max_width(wrap_w);
                            if !label.is_empty() {
                                ui.label(
                                    egui::RichText::new(label).color(theme.accent).size(tk::FONT_XS),
                                );
                                ui.add_space(2.0);
                            }
                            if msg.role == ChatRole::Assistant {
                                render_markdown(
                                    ui,
                                    &msg.content,
                                    theme,
                                    tk::FONT_BASE,
                                    wrap_w,
                                    |ui, block| {
                                        code_block(ui, theme, lang, block, run_wanted);
                                    },
                                    |ui| message_menu(ui, lang, &msg.content, composer_wanted),
                                );
                            } else {
                                draw_sent_message(ui, &msg.content, theme, wrap_w, lang, composer_wanted);
                            }
                        });
                    });
                },
            );
        },
    );
    ui.add_space(BUBBLE_GAP);
}

/// Measure the natural (unwrapped) width of a bubble's content: the widest
/// line a body renders at the given font, used to shrink-wrap the bubble frame
/// for short messages. Returns the width in points, before any frame padding.
fn measure_bubble_content(ui: &mut egui::Ui, msg: &ChatMessage, label: &str) -> f32 {
    let font_id = egui::FontId::proportional(tk::FONT_BASE);
    let galley = ui.fonts(|f| f.layout_no_wrap(msg.content.to_owned(), font_id, egui::Color32::WHITE));
    let body_w = galley.size().x;
    let label_w = if label.is_empty() {
        0.0
    } else {
        let id = egui::FontId::proportional(tk::FONT_XS);
        ui.fonts(|f| f.layout_no_wrap(label.to_owned(), id, egui::Color32::WHITE).size().x)
    };
    body_w.max(label_w)
}

/// Render a sent (user) message the way ChatGPT / Claude render their own
/// bubbles: soft-wrap at the bubble width, and break *anywhere* (even inside
/// a long URL / command / base64 run) so the bubble always shrink-wraps its
/// content. No horizontal scrolling — that is what left wide blank margins on
/// both sides when a line overruns.
fn draw_sent_message(
    ui: &mut egui::Ui,
    content: &str,
    theme: &crate::ui::theme::ThemeColors,
    max_w: f32,
    lang: crate::ui::Language,
    composer_wanted: &mut Option<String>,
) {
    let font_id = egui::FontId::proportional(tk::FONT_BASE);

    // Build a wrap-anywhere layout job and lay it out into a galley ourselves,
    // because `WidgetText::LayoutJob` has its `max_width`/wrap fields overridden
    // by the surrounding layout — the galley form is the only way to guarantee
    // `break_anywhere` (the CSS `overflow-wrap: anywhere` equivalent) applies.
    let mut job = egui::text::LayoutJob::simple(
        content.to_owned(),
        font_id,
        theme.fg_primary,
        max_w,
    );
    job.wrap.break_anywhere = true;
    job.wrap.max_width = max_w;
    let galley = ui.fonts(|f| f.layout_job(job));

    ui.set_max_width(max_w);
    let resp = ui.add(
        egui::Label::new(egui::WidgetText::Galley(galley)).selectable(true),
    );
    resp.context_menu(|ui| {
        message_menu(ui, lang, content, composer_wanted);
    });
}

/// Right-click menu shared by every chat bubble (and every selectable span in
/// it): copy the whole message to the clipboard, or push it into the composer
/// as a draft.
fn message_menu(
    ui: &mut egui::Ui,
    lang: crate::ui::Language,
    content: &str,
    composer_wanted: &mut Option<String>,
) {
    if ui.button(lang.t("copy")).clicked() {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(content.to_owned());
        }
        ui.close_menu();
    }
    if ui.button(lang.t("add_to_input")).clicked() {
        *composer_wanted = Some(content.to_owned());
        ui.close_menu();
    }
}

/// Floating, thin scroll bars that overlay their content instead of allocating
/// their own tall row beneath it. Shared by the code cards, the user-message
/// panes and the run-confirmation dialog.
fn thin_floating_scroll(ui: &mut egui::Ui) {
    ui.style_mut().spacing.scroll.floating = true;
    ui.style_mut().spacing.scroll.floating_width = 3.0;
    ui.style_mut().spacing.scroll.bar_width = 3.0;
    ui.style_mut().spacing.scroll.bar_inner_margin = 0.0;
    ui.style_mut().spacing.scroll.bar_outer_margin = 0.0;
}

/// A single fenced code block: a `input_bg` box with a monospace command and
/// accent "Run" / "Copy" actions on its right.
fn code_block(
    ui: &mut egui::Ui,
    theme: &crate::ui::theme::ThemeColors,
    lang: crate::ui::Language,
    block: &str,
    run_wanted: &mut Option<String>,
) {
    let cmd = block.trim().to_string();
    if cmd.is_empty() {
        return;
    }
    egui::Frame::none()
        .fill(theme.input_bg)
        .rounding(egui::Rounding::same(tk::RADIUS_SM))
        .inner_margin(egui::Margin { left: 10.0, right: 10.0, top: 7.0, bottom: 7.0 })
        .show(ui, |ui| {
            // Push a unique id scope per block using an auto-incrementing id
            // (NOT the command text — identical commands must still get distinct
            // ids). This isolates the horizontal ScrollArea and Run/Copy buttons
            // from siblings and the enclosing vertical scroll area.
            ui.push_id(ui.next_auto_id(), |ui| {
                // Floating, thin scroll bar so it overlays the command instead
                // of allocating its own tall row beneath it.
                ui.scope(|ui| {
                    thin_floating_scroll(ui);
                    egui::ScrollArea::horizontal()
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                        .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&cmd)
                                        .color(theme.fg_primary)
                                        .size(tk::FONT_SM)
                                        .monospace(),
                                )
                                .selectable(true),
                            );
                            // Fixed gap so the command's last characters never
                            // touch the Run/Copy actions when scrolled to the end.
                            ui.add_space(56.0);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let small = |text: &str| {
                                    egui::RichText::new(text)
                                        .color(theme.accent)
                                        .size(tk::FONT_SM)
                                        .strong()
                                };
                                if ui.add(egui::Button::new(small("Run")).frame(false)).clicked() {
                                    *run_wanted = Some(cmd.clone());
                                }
                                if ui.add(egui::Button::new(small(lang.t("copy"))).frame(false)).clicked() {
                                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                        let _ = clipboard.set_text(cmd.clone());
                                    }
                                }
                            });
                        });
                    });
                });
            });
        });
}

fn bubble_frame(fill: egui::Color32) -> egui::Frame {
    egui::Frame {
        fill,
        inner_margin: egui::Margin { left: 14.0, right: 14.0, top: 10.0, bottom: 10.0 },
        rounding: egui::Rounding {
            nw: 10.0, ne: 10.0, sw: 10.0, se: 2.0,
        },
        stroke: egui::Stroke::NONE,
        ..Default::default()
    }
}

fn draw_composer(
    ui: &mut egui::Ui,
    theme: &crate::ui::ThemeColors,
    lang: crate::ui::Language,
    handle: &std::sync::Arc<std::sync::Mutex<AiChatState>>,
    composer_text: &mut String,
    do_send: &mut Option<String>,
    composer_id: egui::Id,
) {
    let running = handle.lock().map(|s| s.running).unwrap_or(false);

    ui.separator();
    egui::Frame::none()
        .inner_margin(egui::Margin::symmetric(PAD, 10.0))
        .show(ui, |ui| {
            // Input box on a rounded `input_bg` background.
            egui::Frame::none()
                .fill(theme.input_bg)
                .rounding(egui::Rounding::same(10.0))
                .stroke(egui::Stroke::new(1.0, theme.input_border))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    let edit = egui::TextEdit::multiline(composer_text)
                        .id(composer_id)
                        .hint_text(lang.t("ai_input_placeholder"))
                        .desired_rows(COMPOSER_EDIT_ROWS)
                        .desired_width(f32::INFINITY)
                        .font(egui::FontId::proportional(tk::FONT_BASE))
                        .frame(false);
                    // egui multiline TextEdits grow unboundedly with content and
                    // never scroll on their own, so a long draft would push the
                    // Send button off the drawer. Cap the editor at the fixed
                    // two-row height and let overflow scroll inside the composer
                    // (egui scrolls the nearest enclosing ScrollArea only, so
                    // the message list above is unaffected). The cap must cover
                    // the TextEdit's own vertical margin, or the bottom of the
                    // box is clipped.
                    let edit_h = composer_edit_height(ui);
                    let resp = egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(edit_h)
                        .id_salt(composer_id.with("composer_scroll"))
                        .show(ui, |ui| ui.add(edit))
                        .inner;
                    // Enter sends; Shift+Enter inserts a newline.
                    let enter_pressed = resp.has_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
                    let can_send = !composer_text.trim().is_empty() && !running;

                    let mut send_clicked = false;
                    ui.add_space(6.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn = ui.add_enabled(
                            can_send,
                            egui::Button::new(
                                egui::RichText::new(if running { lang.t("loading") } else { lang.t("ai_send") })
                                    .color(theme.button_text)
                                    .size(tk::FONT_MD)
                                    .strong(),
                            )
                            .fill(theme.accent)
                            .rounding(8.0)
                            .min_size(egui::vec2(88.0, 30.0)),
                        );
                        send_clicked = btn.clicked();
                    });

                    // Persist the composer draft so it survives redraws.
                    if let Ok(mut st) = handle.lock() { st.input = composer_text.clone(); }

                    if can_send && (enter_pressed || send_clicked) {
                        *do_send = Some(composer_text.trim().to_string());
                    }
                });
        });
}

/// Height cap of the command box in the run-confirmation window: long commands
/// scroll inside it instead of stretching the window past the screen (which
/// used to push the Run / Cancel buttons out of reach).
fn confirm_code_max_height(screen_h: f32) -> f32 {
    (screen_h * 0.35).clamp(120.0, 320.0)
}

/// Width cap of the run-confirmation window; wider commands pan horizontally
/// inside the command box, like the drawer's code cards.
fn confirm_max_width(screen_w: f32) -> f32 {
    (screen_w * 0.7).min(560.0)
}

/// Confirmation window shown when the user clicks "Run" on a code block.
fn render_run_confirm(
    ctx: &egui::Context,
    theme: &crate::ui::ThemeColors,
    lang: crate::ui::Language,
    handle: &std::sync::Arc<std::sync::Mutex<AiChatState>>,
) {
    let Some(cmd) = handle.lock().ok().and_then(|s| s.confirm_run.clone()) else { return };

    let screen = ctx.screen_rect();
    let max_w = confirm_max_width(screen.width());
    let code_h = confirm_code_max_height(screen.height());

    let mut confirmed = false;
    let mut cancelled = false;

    egui::Window::new("ai_run_confirm")
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .max_width(max_w)
        .frame(crate::ui::widgets::dialog_frame(theme))
        .show(ctx, |ui| {
            ui.set_max_width(max_w - 2.0 * 20.0); // minus the dialog frame's inner margin
            ui.label(egui::RichText::new(lang.t("ai_confirm_run")).color(theme.fg_primary).size(tk::FONT_BASE).strong());
            ui.add_space(tk::SPACE_SM);
            egui::Frame::none()
                .fill(theme.input_bg)
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    // Scroll in BOTH directions, capped in height: many lines
                    // scroll vertically, over-long lines pan horizontally —
                    // the buttons below always stay visible and clickable.
                    ui.push_id("ai_run_confirm_code", |ui| {
                        ui.scope(|ui| {
                            thin_floating_scroll(ui);
                            egui::ScrollArea::both()
                                .auto_shrink([true, true])
                                .max_height(code_h)
                                .scroll_bar_visibility(
                                    egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                                )
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(&cmd)
                                                .color(theme.fg_primary)
                                                .size(tk::FONT_MD)
                                                .monospace(),
                                        )
                                        .selectable(true)
                                        .extend(),
                                    );
                                });
                        });
                    });
                });
            ui.add_space(tk::SPACE_MD);
            ui.horizontal(|ui| {
                if ui.add(crate::ui::widgets::danger_button(lang.t("ai_run"), theme)).clicked() {
                    confirmed = true;
                }
                if ui.add(crate::ui::widgets::secondary_button(lang.t("ai_cancel"), theme)).clicked() {
                    cancelled = true;
                }
            });
        });

    if confirmed || cancelled {
        let mut st = handle.lock().unwrap();
        st.confirm_run = None;
        if confirmed {
            st.pending_run = Some(cmd);
        }
    }
}

/// Layout regression tests for the chat drawer, rendered headlessly with
/// egui's built-in fonts. The real renderers (`draw_bubble`,
/// `draw_streaming_bubble`, `render_run_confirm`) are called directly so the
/// tests cannot drift from the production layout code.
#[cfg(test)]
mod layout_tests {
    use super::*;
    use eframe::egui;

    const SCREEN_W: f32 = 1000.0;
    const SCREEN_H: f32 = 800.0;
    /// Tolerance for comparisons against layout rects.
    const EPS: f32 = 0.5;

    fn raw_input() -> egui::RawInput {
        let mut raw = egui::RawInput::default();
        raw.screen_rect = Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(SCREEN_W, SCREEN_H),
        ));
        raw
    }

    fn theme() -> crate::ui::ThemeColors {
        crate::ui::theme::ThemePreset::all()[0].colors()
    }

    /// Draw `messages` through the real `draw_bubble` inside a chat drawer of
    /// `drawer_width` px, returning `(list_min_rect, visible_bounds,
    /// Draw `messages` through the real `draw_bubble` inside a chat drawer of
    /// `drawer_width` px, returning `(list_min_rect, visible_bounds)`:
    /// - `list_min_rect` — the content Ui's `min_rect` (spans the viewport
    ///   width by design, `auto_shrink` false),
    /// - `visible_bounds` — union of every mesh's vertex bbox intersected with
    ///   its clip rect (what can actually show up on screen; scissored-away
    ///   geometry excluded).
    fn draw_bubbles_in_drawer(
        drawer_width: f32,
        messages: &[ChatMessage],
    ) -> (egui::Rect, egui::Rect) {
        let ctx = egui::Context::default();
        let mut list_min_rect = egui::Rect::NOTHING;
        let out = ctx.run(raw_input(), |ctx| {
            let theme = theme();
            egui::SidePanel::right("drawer").exact_width(drawer_width).show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for msg in messages {
                            let mut run_wanted: Option<String> = None;
                            let mut composer_wanted: Option<String> = None;
                            draw_bubble(
                                ui,
                                msg,
                                &theme,
                                crate::ui::Language::English,
                                &mut run_wanted,
                                &mut composer_wanted,
                            );
                        }
                        list_min_rect = ui.min_rect();
                    });
            });
        });
        let mut visible = egui::Rect::NOTHING;
        for prim in ctx.tessellate(out.shapes, 1.0) {
            let mut mesh_x = (f32::INFINITY, f32::NEG_INFINITY);
            if let egui::epaint::Primitive::Mesh(mesh) = prim.primitive {
                for v in &mesh.vertices {
                    mesh_x.0 = mesh_x.0.min(v.pos.x);
                    mesh_x.1 = mesh_x.1.max(v.pos.x);
                }
            }
            // Vertices outside the clip rect are scissored away on the GPU, so
            // the visible extent of a mesh never leaves its clip rect.
            let r = egui::Rect::from_x_y_ranges(mesh_x.0..=mesh_x.1, 0.0..=0.0)
                .intersect(prim.clip_rect);
            if !r.is_negative() {
                visible = if visible == egui::Rect::NOTHING {
                    r
                } else {
                    visible.union(r)
                };
            }
        }
        (list_min_rect, visible)
    }

    /// A short user message ("是啥", two chars) must produce a bubble that
    /// shrink-wraps its content, NOT fill the whole drawer row. This guards the
    /// regression where the filling `bubble_frame` stretched to the full
    /// available width regardless of how little text there was.
    #[test]
    fn short_input_bubble_shrink_wraps_instead_of_filling_row() {
        let text = "是啥";
        let ctx = egui::Context::default();
        let theme = theme();
        let user_bg = theme.accent_alpha(32);
        let mut bg_xmin = f32::INFINITY;
        let mut bg_xmax = f32::NEG_INFINITY;
        let out = ctx.run(raw_input(), |ctx| {
            egui::SidePanel::right("drawer").exact_width(460.0).show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let mut run_wanted = None;
                    let mut composer_wanted = None;
                    draw_bubble(
                        ui,
                        &ChatMessage::user(text.to_string()),
                        &theme,
                        crate::ui::Language::English,
                        &mut run_wanted,
                        &mut composer_wanted,
                    );
                });
            });
        });
        // Find the bubble background mesh (the accent-tinted fill) and measure
        // its horizontal extent at the top of the bubble.
        for prim in ctx.tessellate(out.shapes, 1.0) {
            if let egui::epaint::Primitive::Mesh(mesh) = prim.primitive {
                // Only look at the filled quads in the top ~50px (the bubble).
                for v in &mesh.vertices {
                    if v.pos.y < 50.0 && v.color == user_bg {
                        bg_xmin = bg_xmin.min(v.pos.x);
                        bg_xmax = bg_xmax.max(v.pos.x);
                    }
                }
            }
        }
        let bg_w = bg_xmax - bg_xmin;
        // Two CJK glyphs at FONT_BASE ≈ 28px + bubble padding (2×14) ≈ 56px.
        // A full-row fill would be ~412px. Allow generous headroom but well
        // under half the drawer content width.
        assert!(
            bg_w < 150.0,
            "short input bubble filled {bg_w}px wide — should shrink-wrap (~56px), \
             not stretch the whole row"
        );
        // The user bubble must be RIGHT-aligned within the right-side drawer,
        // with a trailing PAD gap (not flush against the edge). Drawer right
        // edge = SCREEN_W = 1000; the bubble's right edge should sit near it
        // minus PAD (~984), neither hugging the left edge nor bleeding past.
        assert!(
            bg_xmax > 1000.0 - 80.0,
            "short input bubble right edge at x={bg_xmax} — should sit near the \
             drawer's right edge (~984), not the left"
        );
        assert!(
            bg_xmax <= 1000.0 - PAD + EPS,
            "short input bubble right edge at x={bg_xmax} — should keep a {PAD}px \
             gap from the drawer's right edge, not be flush against it"
        );
    }

    /// The pre-fix bug: a cap computed from the OUTER Ui ignored the bubble's
    /// own inner margins, so on narrow drawers wide messages stretched the
    /// scroll content up to ~54px past the drawer edge and the text was
    /// clipped ("撑开占满，回复被遮挡").
    #[test]
    fn bubbles_stay_inside_drawer_on_narrow_drawers() {
        let long = "x".repeat(600);
        let messages = vec![
            ChatMessage::user("hi there"),
            ChatMessage::user(long.clone()),
            ChatMessage::assistant(format!("a very long reply {}", long)),
            ChatMessage::assistant(format!("```sh\n{}\n```", long)),
        ];
        for drawer_width in [460.0, 300.0] {
            let (_, visible) = draw_bubbles_in_drawer(drawer_width, &messages);
            assert!(
                visible.max.x <= SCREEN_W + EPS,
                "drawer {drawer_width}: painted bubble content overflows the drawer \
                 edge (max.x = {}, edge = {SCREEN_W})",
                visible.max.x
            );
        }
    }

    /// A long unbreakable run (no spaces — a URL, base64, or one big word) must
    /// still WRAP (break-anywhere), never pan horizontally. This is the
    /// ChatGPT/Claude convention: no horizontal scroll, so no wide blank
    /// margins. The bubble grows to many rows and stays inside the drawer.
    #[test]
    fn long_unbreakable_run_breaks_instead_of_panning() {
        let messages = vec![ChatMessage::user("x".repeat(600))];
        let (union, visible) = draw_bubbles_in_drawer(460.0, &messages);
        // A 600-char break-anywhere wrap yields many rows (247px at FONT_BASE),
        // whereas single-row panning would be one ~18px row + margins.
        assert!(
            union.height() > 2.0 * 18.0 + 20.0,
            "long run did not wrap: bubble only {} px tall",
            union.height()
        );
        // And it still never overflows the drawer edge.
        assert!(
            visible.max.x <= SCREEN_W + EPS,
            "long run overflowed the drawer: painted to {}", visible.max.x
        );
    }

    /// Ordinary wrap-able prose (spaces present) must WRAP and shrink-wrap the
    /// bubble, not render as a single panning row: the bubble's reported height
    /// grows to multiple rows. (A single panning row is what produced the
    /// "too much blank space on both sides" complaint — the unbreakable text
    /// ran off to the right, leaving a wide empty bubble.)
    #[test]
    fn user_prose_wraps_instead_of_panning() {
        let words: String = (0..40).map(|i| format!("word{i} ")).collect();
        let (union, _) = draw_bubbles_in_drawer(460.0, &[ChatMessage::user(words)]);
        // Wrapped prose spans many rows.
        assert!(
            union.height() > 2.0 * 18.0 + 20.0,
            "prose did not wrap: bubble is only {} px tall",
            union.height()
        );
    }

    /// A long run of CJK (space-less, e.g. the user pasting a wall of Chinese
    /// text) must soft-wrap into many rows. This guards the specific regression
    /// where the wrap width was taken from the *unwrapped* measured content
    /// width, so the bubble frame was clamped to the drawer but the galley laid
    /// out at the full unwrapped width and got clipped instead of wrapping.
    #[test]
    fn long_cjk_wall_wraps_into_multiple_rows() {
        let text = "大大叔大叔大叔大叔说是生生世世生生世世生生世世生生世世".repeat(8);
        let (union, _) = draw_bubbles_in_drawer(460.0, &[ChatMessage::user(text)]);
        assert!(
            union.height() > 2.0 * 18.0 + 20.0,
            "long CJK input did not wrap: bubble is only {} px tall",
            union.height()
        );
    }

    /// A LONG user message, when right-aligned, must not stretch so wide that
    /// its left edge presses against the drawer's left border. The cap must be a
    /// fraction of the drawer width (not `avail - PAD`), leaving skin on both
    /// sides regardless of message length. This guards the "超长内容左边框贴着"
    /// regression.
    #[test]
    fn long_user_bubble_keeps_left_gap_not_flush() {
        let long = "x".repeat(600);
        let ctx = egui::Context::default();
        let theme = theme();
        let user_bg = theme.accent_alpha(32);
        let mut bg_xmin = f32::INFINITY;
        let out = ctx.run(raw_input(), |ctx| {
            egui::SidePanel::right("drawer").exact_width(460.0).show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let mut run_wanted = None;
                    let mut composer_wanted = None;
                    draw_bubble(
                        ui,
                        &ChatMessage::user(long.clone()),
                        &theme,
                        crate::ui::Language::English,
                        &mut run_wanted,
                        &mut composer_wanted,
                    );
                });
            });
        });
        // Drawer spans [540, 1000]. Measure the bubble background's left edge.
        for prim in ctx.tessellate(out.shapes, 1.0) {
            if let egui::epaint::Primitive::Mesh(mesh) = prim.primitive {
                for v in &mesh.vertices {
                    if v.pos.y < 200.0 && v.color == user_bg {
                        bg_xmin = bg_xmin.min(v.pos.x);
                    }
                }
            }
        }
        // The bubble is right-aligned (drawer left edge = 540), so a 0.82-width
        // cap leaves ~18% of 460 ≈ 83px of gap from the left edge. It must not
        // start at the drawer's left border.
        assert!(
            bg_xmin > 540.0 + 40.0,
            "long user bubble left edge at x={bg_xmin} — should keep a gap from \
             the drawer's left border (~{:?}), not be flush against it",
            540.0 + 40.0
        );
    }

    /// The pre-fix bug: the run-confirmation window grew with the command, so
    /// a long command pushed the Run / Cancel buttons off-screen. The command
    /// box now scrolls (vertically for many lines, horizontally for long
    /// lines) inside a height cap, keeping the window on screen.
    #[test]
    fn run_confirm_window_stays_on_screen_for_long_commands() {
        let cmd = "echo this-is-a-fairly-long-command-line --with-flags\n".repeat(120);
        let handle: crate::ui::state::ai_chat::AiChatHandle = std::sync::Arc::new(
            std::sync::Mutex::new(AiChatState {
                confirm_run: Some(cmd),
                ..Default::default()
            }),
        );
        let ctx = egui::Context::default();
        let _ = ctx.run(raw_input(), |ctx| {
            render_run_confirm(ctx, &theme(), crate::ui::Language::English, &handle);
        });
        let rect = ctx
            .memory(|m| m.area_rect(egui::Id::new("ai_run_confirm")))
            .expect("confirm window should be rendered");
        assert!(
            rect.height() <= SCREEN_H + EPS,
            "confirm window is {} px tall — the buttons would be unreachable",
            rect.height()
        );
        assert!(
            rect.right() <= SCREEN_W + EPS && rect.bottom() <= SCREEN_H + EPS,
            "confirm window leaves the screen: {rect:?}"
        );
        assert!(
            rect.width() <= confirm_max_width(SCREEN_W) + EPS,
            "confirm window is {} px wide (cap {})",
            rect.width(),
            confirm_max_width(SCREEN_W)
        );
    }
}

/// Render the draggable orb in the bottom-right of a tab's terminal area.
///
/// Returns `(clicked, new_offset)`:
/// - `clicked` — a clean click (no drag) that should open the drawer.
/// - `new_offset` — the absolute drag offset to persist.
pub fn render_orb(
    ctx: &egui::Context,
    tab_rect: egui::Rect,
    theme: &crate::ui::ThemeColors,
    prev_offset: egui::Vec2,
) -> (bool, egui::Vec2) {
    let anchor = egui::pos2(
        tab_rect.right() - ORB_MARGIN,
        tab_rect.bottom() - ORB_MARGIN,
    );
    let center = anchor - prev_offset;

    let mut opened = false;
    let mut offset = prev_offset;

    let orb_id = egui::Id::new("llm_orb");
    egui::Area::new(orb_id)
        .fixed_pos(egui::pos2(center.x - ORB_RADIUS, center.y - ORB_RADIUS))
        .order(egui::Order::Foreground)
        .interactable(true)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(ORB_RADIUS * 2.0, ORB_RADIUS * 2.0),
                egui::Sense::click_and_drag(),
            );
            let c = rect.center();
            let hovered = resp.hovered() || resp.dragged();
            let base = if hovered { theme.accent } else { theme.accent_alpha(200) };
            ui.painter().circle_filled(c, ORB_RADIUS, base);
            ui.painter().circle(
                c,
                ORB_RADIUS,
                egui::Color32::TRANSPARENT,
                egui::Stroke::new(2.0, theme.button_text),
            );
            ui.painter().text(
                c,
                egui::Align2::CENTER_CENTER,
                "\u{2726}",
                egui::FontId::proportional(15.0),
                theme.button_text,
            );

            if resp.dragged() {
                offset = prev_offset - resp.drag_delta();
            } else if resp.clicked() {
                opened = true;
            }
        });

    (opened, offset)
}
