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

/// Render the AI chat drawer on the right side of a window (if open), plus the
/// "run" confirmation window. Called from `AppWindow::render_drawers`.
pub fn render_ai_drawer(window: &mut AppWindow, ctx: &egui::Context, cx: &mut WindowContext) {
    let theme: &crate::ui::ThemeColors = cx.theme;
    let lang = cx.language;
    let llm_config = cx.llm_config.clone();

    let active = window.active_tab;
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

            // Messages (height-limited so the composer stays visible below).
            let mut run_wanted: Option<String> = None;
            let composer_h = 118.0;
            let mut composer_text = {
                let st = handle.lock().unwrap();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height((ui.available_height() - composer_h).max(80.0))
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if st.messages.is_empty() && st.streaming.is_none() {
                            draw_empty_state(ui, theme, lang);
                        }
                        for msg in &st.messages {
                            draw_bubble(ui, msg, theme, lang, &mut run_wanted);
                        }
                        if let Some(buf) = &st.streaming {
                            draw_streaming_bubble(ui, &buf.text, theme);
                        } else if st.running {
                            draw_thinking_bubble(ui, theme, lang);
                        }
                        if let Some(err) = &st.error {
                            draw_error(ui, theme, lang, err);
                        }
                        ui.add_space(8.0);
                    });
                st.input.clone()
            };
            // If a Run button was clicked, ask for confirmation.
            if let Some(cmd) = run_wanted {
                if let Ok(mut st) = handle.lock() { st.confirm_run = Some(cmd); }
            }

            // Composer — fixed block pinned to the bottom.
            draw_composer(ui, theme, lang, &handle, &mut composer_text, &mut do_send);
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
    let max_w = (ui.available_width() - 2.0 * PAD).min(320.0);
    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
        egui::Frame::none()
            .inner_margin(egui::Margin { left: PAD, right: PAD, top: 0.0, bottom: 0.0 })
            .show(ui, |ui| {
                bubble_frame(theme.bg_secondary).show(ui, |ui| {
                    ui.set_max_width(max_w);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(text).color(theme.fg_primary).size(tk::FONT_BASE),
                        )
                        .selectable(true),
                    );
                });
            });
        ui.add_space(BUBBLE_GAP);
    });
}

/// A subtle "assistant is typing…" bubble with three animated accent dots,
/// replacing the default spinner while a reply is awaited (before any token
/// arrives, at which point `draw_streaming_bubble` takes over).
fn draw_thinking_bubble(ui: &mut egui::Ui, theme: &crate::ui::theme::ThemeColors, lang: crate::ui::Language) {
    let now = ui.input(|i| i.time);
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
                        // Three dots with a staggered sine-wave offset.
                        for i in 0..3u32 {
                            let phase = now * 2.4 + i as f64 * 0.9;
                            let a = (phase.sin() * 0.5 + 0.5) * 0.6 + 0.4;
                            let alpha = if a > 0.55 {
                                255u8
                            } else {
                                (a * 255.0) as u8
                            };
                            let color = if alpha > 160 {
                                theme.accent
                            } else {
                                theme.accent_alpha(alpha)
                            };
                            ui.add_space(2.0);
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(5.0, 5.0),
                                egui::Sense::hover(),
                            );
                            ui.painter().circle_filled(rect.center(), 2.5, color);
                        }
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(lang.t("loading"))
                                .color(theme.fg_dim)
                                .size(tk::FONT_SM),
                        );
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
                    ui.set_max_width(ui.available_width() - 2.0 * PAD);
                    ui.label(egui::RichText::new(text).color(theme.red).size(tk::FONT_SM));
                });
        });
}

/// Render a completed message bubble. Assistant replies show "Run" buttons under
/// fenced code blocks (writing the chosen command into `run_wanted`).
fn draw_bubble(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    theme: &crate::ui::theme::ThemeColors,
    lang: crate::ui::Language,
    run_wanted: &mut Option<String>,
) {
    let is_user = msg.role == ChatRole::User;
    let (bg, align) = if is_user {
        (theme.accent_alpha(32), egui::Align::Max)
    } else {
        (theme.bg_secondary, egui::Align::Min)
    };
    // Assistant bubbles carry a small role label; user bubbles stay minimal.
    let label = if is_user { "" } else { "\u{2726}" };

    ui.with_layout(egui::Layout::top_down(align), |ui| {
        let max_w = (ui.available_width() - 2.0 * PAD).min(320.0);
        // Outer margin keeps the bubble off the drawer's left/right border.
        egui::Frame::none()
            .inner_margin(egui::Margin { left: PAD, right: PAD, top: 0.0, bottom: 0.0 })
            .show(ui, |ui| {
                let inner = bubble_frame(bg).show(ui, |ui| {
                    ui.set_max_width(max_w);
                    if !label.is_empty() {
                        ui.label(
                            egui::RichText::new(label).color(theme.accent).size(tk::FONT_XS),
                        );
                        ui.add_space(2.0);
                    }
                    // Assistant replies render as Markdown; user text stays plain.
                    if msg.role == ChatRole::Assistant {
                        // Code blocks are rendered in-place (at their original
                        // position in the prose) via the callback.
                        render_markdown(ui, &msg.content, theme, tk::FONT_BASE, max_w, |ui, block| {
                            code_block(ui, theme, lang, block, run_wanted);
                        });
                    } else {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&msg.content)
                                    .color(theme.fg_primary)
                                    .size(tk::FONT_BASE),
                            )
                            .selectable(true)
                            .wrap(),
                        );
                    }
                });
                // Right-click anywhere on the bubble copies the full message.
                if inner.response.secondary_clicked() {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(msg.content.clone());
                    }
                }
            });
        ui.add_space(BUBBLE_GAP);
    });
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
                    ui.style_mut().spacing.scroll.floating = true;
                    ui.style_mut().spacing.scroll.floating_width = 3.0;
                    ui.style_mut().spacing.scroll.bar_width = 3.0;
                    ui.style_mut().spacing.scroll.bar_inner_margin = 0.0;
                    ui.style_mut().spacing.scroll.bar_outer_margin = 0.0;
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
                        .hint_text(lang.t("ai_input_placeholder"))
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .font(egui::FontId::proportional(tk::FONT_BASE))
                        .frame(false);
                    let resp = ui.add(edit);
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

/// Confirmation window shown when the user clicks "Run" on a code block.
fn render_run_confirm(
    ctx: &egui::Context,
    theme: &crate::ui::ThemeColors,
    lang: crate::ui::Language,
    handle: &std::sync::Arc<std::sync::Mutex<AiChatState>>,
) {
    let Some(cmd) = handle.lock().ok().and_then(|s| s.confirm_run.clone()) else { return };

    let mut confirmed = false;
    let mut cancelled = false;

    egui::Window::new("ai_run_confirm")
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .frame(crate::ui::widgets::dialog_frame(theme))
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(lang.t("ai_confirm_run")).color(theme.fg_primary).size(tk::FONT_BASE).strong());
            ui.add_space(tk::SPACE_SM);
            egui::Frame::none()
                .fill(theme.input_bg)
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(&cmd).color(theme.fg_primary).size(tk::FONT_MD).monospace());
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
