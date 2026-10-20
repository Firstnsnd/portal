//! # SSH 2FA (keyboard-interactive) prompt window
//!
//! Renders the OTP / multi-prompt challenge delivered by
//! [`crate::ssh::auth_prompt::AuthPromptBridge`] while a connection is in
//! its `Authenticating` phase. Used by both the terminal panes
//! (render.rs, after the grid lock) and the SFTP view's connecting panels.
//!
//! Input state (typed values + last-served prompt id) lives in egui
//! temporary memory keyed by the window id, so both surfaces share this
//! code without the callers owning per-session buffers.

use eframe::egui;

use crate::ssh::{AuthPrompt, AuthPromptBridge};
use crate::ui::theme::ThemeColors;
use crate::ui::i18n::Language;

/// Pure state-sync: keep the input buffers in step with the prompt.
///
/// - First time an id is seen → fresh empty buffers, one per prompt field.
/// - Same id → buffers untouched (the user may be mid-typing).
/// - New id (a new auth round) → buffers reset, so a stale OTP from a
///   previous round can never leak into the next one.
/// - Field-count change under the same id (defensive) → resize, keeping
///   values where positions still exist.
pub fn sync_prompt_inputs(
    last_prompt_id: &mut Option<u64>,
    inputs: &mut Vec<String>,
    prompt: &AuthPrompt,
) {
    let want = prompt.prompts.len();
    if *last_prompt_id != Some(prompt.id) {
        inputs.clear();
        *last_prompt_id = Some(prompt.id);
    }
    if inputs.len() != want {
        inputs.resize(want, String::new());
    }
}

/// What the caller should do after the window was shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthPromptUiAction {
    /// Nothing happened; keep waiting.
    None,
    /// The user cancelled — the caller should abort/close (the bridge has
    /// already been cancelled, waking the auth task).
    Cancelled,
}

/// Render the keyboard-interactive challenge window.
///
/// `id_source` must be unique per surface (e.g. the pane id or
/// "sftp_left"/"sftp_right") so concurrent prompts in different panes keep
/// separate input state.
///
/// Submitting answers the bridge directly (the auth task resumes and the
/// pending prompt is consumed — the window disappears on the next poll).
/// Cancelling calls `bridge.cancel()` and returns [`AuthPromptUiAction::Cancelled`].
pub fn render_auth_prompt_window(
    ctx: &egui::Context,
    id_source: impl std::hash::Hash,
    bridge: &AuthPromptBridge,
    prompt: &AuthPrompt,
    theme: &ThemeColors,
    language: &Language,
) -> AuthPromptUiAction {
    // Input state keyed by the window id (see module doc).
    let base_id = egui::Id::new(id_source);
    let inputs_id = base_id.with("2fa_inputs");
    let last_id_key = base_id.with("2fa_last_prompt_id");

    let mut inputs: Vec<String> = ctx.data(|d| d.get_temp(inputs_id).unwrap_or_default());
    let mut last_prompt_id: Option<u64> = ctx.data(|d| d.get_temp(last_id_key).unwrap_or(None));
    sync_prompt_inputs(&mut last_prompt_id, &mut inputs, prompt);
    ctx.data_mut(|d| {
        d.insert_temp(inputs_id, inputs.clone());
        d.insert_temp(last_id_key, last_prompt_id);
    });

    let mut action = AuthPromptUiAction::None;
    let mut submitted = false;

    egui::Window::new(language.t("ssh_2fa_title"))
        .id(base_id)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(egui::Frame {
            fill: theme.bg_elevated,
            inner_margin: egui::Margin::symmetric(20.0, 16.0),
            stroke: egui::Stroke::new(1.0, theme.border),
            rounding: egui::Rounding::same(8.0),
            ..Default::default()
        })
        .show(ctx, |ui| {
            ui.set_min_width(320.0);

            if !prompt.name.is_empty() {
                ui.label(
                    egui::RichText::new(prompt.name.as_str())
                        .color(theme.fg_primary)
                        .size(14.0)
                        .strong(),
                );
            }
            if !prompt.instructions.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(prompt.instructions.as_str())
                        .color(theme.fg_dim)
                        .size(12.0),
                );
            }
            ui.add_space(10.0);

            // One input per prompt; echo=false → masked (password) field.
            let mut focus_first = true;
            let mut enter_pressed = false;
            for (field, buf) in prompt.prompts.iter().zip(inputs.iter_mut()) {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(field.prompt.as_str())
                            .color(theme.fg_primary)
                            .size(12.0),
                    );
                    let mut edit = egui::TextEdit::singleline(buf)
                        .font(egui::FontId::monospace(13.0))
                        .desired_width(180.0)
                        .hint_text(if field.echo { "" } else { "••••••" });
                    if !field.echo {
                        edit = edit.password(true);
                    }
                    let resp = ui.add(edit);
                    if focus_first && !resp.has_focus() && !ctx.wants_keyboard_input() {
                        // Focus the first field when the window opens so the
                        // user can type the code immediately.
                        resp.request_focus();
                    }
                    focus_first = false;
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        enter_pressed = true;
                    }
                });
                ui.add_space(4.0);
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let submit = ui.add(
                    egui::Button::new(
                        egui::RichText::new(language.t("ssh_2fa_submit"))
                            .color(theme.bg_primary)
                            .size(13.0)
                            .strong(),
                    ),
                );
                if ui.add(
                    egui::Button::new(
                        egui::RichText::new(language.t("cancel"))
                            .color(theme.red)
                            .size(13.0),
                    )
                    .frame(false),
                )
                .clicked()
                {
                    action = AuthPromptUiAction::Cancelled;
                }
                if submit.clicked() || enter_pressed {
                    submitted = true;
                }
            });
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(language.t("ssh_2fa_cancel_hint"))
                    .color(theme.fg_dim)
                    .size(10.0),
            );
        });

    if submitted {
        // Count is guaranteed in sync by sync_prompt_inputs. On success the
        // pending prompt is consumed; the next round arrives with a new id
        // and resets these buffers.
        if !bridge.respond(inputs.clone()) {
            log::warn!("2FA: answer count mismatch against pending prompt");
        }
    }
    if action == AuthPromptUiAction::Cancelled {
        bridge.cancel();
        // Drop the typed secrets along with the attempt.
        ctx.data_mut(|d| {
            d.remove::<Vec<String>>(inputs_id);
            d.remove::<Option<u64>>(last_id_key);
        });
    }

    action
}
