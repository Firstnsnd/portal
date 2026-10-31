//! Embedded SFTP file editor.
//!
//! While open, an editor replaces one half of the SFTP view (left or right)
//! entirely — no floating window. Each half holds at most one editor, so two
//! files can be edited or compared side by side. All egui ids are scoped by
//! the half name so simultaneous editors never share widget state.

use eframe::egui;

use crate::sftp::{LocalBrowser, SftpBrowser};
use crate::ui::pane::AppWindow;
use crate::ui::pane_view::WindowContext;
use crate::ui::types::sftp_types::{SftpEmbeddedEditor, SftpPanel};

/// Actions the embedded editor surface wants the caller to perform.
pub struct EditorOutput {
    /// ✕ pressed (second press when armed) or error Cancel → close the editor.
    pub close: bool,
    /// Save pressed → persist this content to the editor's file.
    pub save: Option<String>,
}

/// Open `file_name` from `panel` in the editor that occupies that panel's
/// half. Replaces any editor already open in that half. Local reads are
/// synchronous; remote reads start a `ReadFile` round-trip and open with
/// `loading: true` (delivery via `AppWindow::sftp_editor_deliver_*`).
pub fn open_editor_in_side(window: &mut AppWindow, panel: SftpPanel, file_name: &str) {
    match panel {
        SftpPanel::LeftLocal => {
            let editor = open_local_editor(&mut window.local_browser_left, panel, file_name);
            window.sftp_editor_left = Some(editor);
        }
        SftpPanel::RightLocal => {
            let editor = open_local_editor(&mut window.local_browser_right, panel, file_name);
            window.sftp_editor_right = Some(editor);
        }
        SftpPanel::LeftRemote => {
            if let Some(ref mut browser) = window.sftp_browser_left {
                let editor = open_remote_editor(browser, panel, file_name);
                window.sftp_editor_left = Some(editor);
            }
        }
        SftpPanel::RightRemote => {
            if let Some(ref mut browser) = window.sftp_browser {
                let editor = open_remote_editor(browser, panel, file_name);
                window.sftp_editor_right = Some(editor);
            }
        }
    }
}

fn open_local_editor(browser: &mut LocalBrowser, panel: SftpPanel, file_name: &str) -> SftpEmbeddedEditor {
    let dir = browser.current_path.clone();
    let (content, error) = match browser.read_file(file_name) {
        Ok(content) => (content, String::new()),
        Err(e) => (String::new(), e),
    };
    SftpEmbeddedEditor {
        panel,
        file_path: format!("{}/{}", dir.trim_end_matches('/'), file_name),
        file_name: file_name.to_string(),
        directory: dir,
        content: content.clone(),
        original_content: content,
        loading: false,
        error,
        confirm_close: false,
    }
}

fn open_remote_editor(browser: &mut SftpBrowser, panel: SftpPanel, file_name: &str) -> SftpEmbeddedEditor {
    let dir = browser.current_path.clone();
    let full_path = format!("{}/{}", dir.trim_end_matches('/'), file_name);
    browser.read_file(&full_path);
    SftpEmbeddedEditor {
        panel,
        file_path: full_path,
        file_name: file_name.to_string(),
        directory: dir,
        content: String::new(),
        original_content: String::new(),
        loading: true,
        error: String::new(),
        confirm_close: false,
    }
}

/// Persist `content` to `path` via the browser that owns `panel`. Local
/// writes are synchronous (they also refresh the local listing); remote
/// writes are fire-and-forget followed by a directory re-list. `Ok(())`
/// means the caller may close the editor; `Err(msg)` should be surfaced
/// in the editor's error slot.
pub fn execute_editor_save(window: &mut AppWindow, panel: SftpPanel, path: &str, content: &str) -> Result<(), String> {
    match panel {
        SftpPanel::LeftLocal => save_local(&mut window.local_browser_left, path, content),
        SftpPanel::RightLocal => save_local(&mut window.local_browser_right, path, content),
        SftpPanel::LeftRemote => {
            if let Some(ref mut browser) = window.sftp_browser_left {
                save_remote(browser, path, content);
            }
            Ok(())
        }
        SftpPanel::RightRemote => {
            if let Some(ref mut browser) = window.sftp_browser {
                save_remote(browser, path, content);
            }
            Ok(())
        }
    }
}

fn save_local(browser: &mut LocalBrowser, path: &str, content: &str) -> Result<(), String> {
    let file_name = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    browser.write_file(&file_name, content)
}

fn save_remote(browser: &mut SftpBrowser, path: &str, content: &str) {
    browser.write_file(path, content.as_bytes().to_vec());
    let current = browser.current_path.clone();
    browser.navigate(&current);
}

/// Render one embedded editor filling `rect` (a full half of the SFTP view).
/// `id_side` ("left"/"right") scopes every egui Id so two simultaneous
/// editors cannot share TextEdit/ScrollArea state.
pub fn render_embedded_editor(
    editor: &mut SftpEmbeddedEditor,
    id_side: &'static str,
    ui: &mut egui::Ui,
    cx: &WindowContext,
) -> EditorOutput {
    let mut out = EditorOutput { close: false, save: None };

    // Disarm the close guard when the content is back to its saved state.
    if !editor.is_dirty() {
        editor.confirm_close = false;
    }

    // Slice the half into title strip / body / bottom bar, the same manual
    // rect idiom render_file_panel uses (a bottom-anchored bar plus a
    // height-filling editor cannot come out of plain top-down layout).
    let full = ui.available_rect_before_wrap();
    // Bottom bar sized for the 28px Save button plus frame margins (8+8).
    let bar_h = 44.0_f32.min(full.height() * 0.3);
    let bar_rect = egui::Rect::from_min_max(
        egui::pos2(full.min.x, full.max.y - bar_h),
        egui::pos2(full.max.x, full.max.y),
    );
    // The title strip takes its NATURAL height using the same Frame idiom as
    // the file list's breadcrumb nav, so the two "nav bars" stay equal.
    let title_max_rect =
        egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, bar_rect.min.y));
    let title_rect = ui
        .allocate_new_ui(egui::UiBuilder::new().max_rect(title_max_rect), |ui| {
            egui::Frame {
                fill: cx.theme.bg_secondary,
                inner_margin: egui::Margin::symmetric(8.0, 6.0),
                ..Default::default()
            }
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("\u{1F4C4}").size(12.0).color(cx.theme.accent));
                    ui.add_space(4.0);
                    let badge = if editor.panel.is_local() {
                        cx.language.t("local")
                    } else {
                        cx.language.t("remote")
                    };
                    ui.label(egui::RichText::new(badge).size(9.0).color(cx.theme.fg_dim));
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(&editor.file_name)
                            .size(12.0)
                            .color(cx.theme.fg_primary)
                            .strong(),
                    );
                    if editor.is_dirty() {
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("\u{25CF}").size(10.0).color(cx.theme.accent));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // ✕ — two-step guard while dirty: first press arms
                        // (turns red), second press closes. No new i18n keys.
                        let armed = editor.is_dirty() && editor.confirm_close;
                        let mut close_btn = ui.add(
                            egui::Button::new(
                                egui::RichText::new("\u{2715}").size(12.0)
                                    .color(if armed { cx.theme.red } else { cx.theme.fg_dim }),
                            )
                            .frame(false),
                        );
                        if editor.is_dirty() {
                            close_btn = close_btn.on_hover_text(cx.language.t("unsaved_changes"));
                        }
                        if close_btn.clicked() {
                            if editor.is_dirty() && !editor.confirm_close {
                                editor.confirm_close = true;
                            } else {
                                out.close = true;
                            }
                        }
                        ui.add_space(8.0);
                        if !editor.directory.is_empty() {
                            let dir_display = if editor.directory.len() > 40 {
                                format!("...{}", &editor.directory[editor.directory.len() - 37..])
                            } else {
                                editor.directory.clone()
                            };
                            ui.label(
                                egui::RichText::new(dir_display)
                                    .size(11.0)
                                    .color(cx.theme.fg_dim)
                                    .family(egui::FontFamily::Monospace),
                            );
                        }
                    });
                });
            });
        })
        .response
        .rect;
    let body_rect = egui::Rect::from_min_max(
        egui::pos2(full.min.x, title_rect.max.y),
        egui::pos2(full.max.x, bar_rect.min.y),
    );

    ui.painter().hline(
        body_rect.min.x..=body_rect.max.x,
        body_rect.min.y,
        egui::Stroke::new(1.0, cx.theme.border),
    );

    let has_error = !editor.error.is_empty();
    let interactive = !editor.loading && !has_error;

    // ── Body ─────────────────────────────────────────────────────────────
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body_rect), |ui| {
        if editor.loading {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.spinner();
                ui.add_space(8.0);
                ui.label(egui::RichText::new(cx.language.t("loading")).color(cx.theme.fg_dim));
            });
        } else if has_error {
            egui::Frame {
                fill: cx.theme.bg_primary,
                inner_margin: egui::Margin::same(20.0),
                ..Default::default()
            }
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("\u{26A0}").size(16.0).color(cx.theme.red));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(&editor.error).color(cx.theme.red).size(12.0));
                });
                ui.add_space(16.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(
                        egui::Button::new(
                            egui::RichText::new(cx.language.t("cancel")).color(cx.theme.fg_dim).size(13.0),
                        )
                        .fill(cx.theme.bg_elevated)
                        .rounding(6.0)
                        .min_size(egui::vec2(70.0, 32.0)),
                    )
                    .clicked()
                    {
                        out.close = true;
                    }
                });
            });
        } else {
            let body = ui.available_rect_before_wrap();
            let gutter_w = 44.0_f32.min(body.width() * 0.25).max(24.0);
            let gutter_rect =
                egui::Rect::from_min_max(body.min, egui::pos2(body.min.x + gutter_w, body.max.y));
            let edit_rect =
                egui::Rect::from_min_max(egui::pos2(body.min.x + gutter_w, body.min.y), body.max);

            // No-wrap layouter: a line longer than the viewport extends to the
            // right instead of folding (egui 0.29's TextEdit has no wrap_mode
            // builder, so the wrap width is overridden here). The measured
            // galley width also drives the widened child below.
            let font_id = egui::FontId::monospace(cx.font_size);
            let text_color = cx.theme.fg_primary;
            let galley_width = std::rc::Rc::new(std::cell::Cell::new(0.0_f32));
            let mut layouter = {
                let galley_width = std::rc::Rc::clone(&galley_width);
                let font_id = font_id.clone();
                move |ui: &egui::Ui, text: &str, _wrap_width: f32| {
                    let job = egui::text::LayoutJob::simple(
                        text.to_owned(),
                        font_id.clone(),
                        text_color,
                        f32::INFINITY,
                    );
                    let galley = ui.fonts(|f| f.layout_job(job));
                    galley_width.set(galley.size().x);
                    galley
                }
            };

            // egui 0.29's ScrollArea clamps content width to the viewport
            // (hard-coded "prefer wrapping" branch, fixed upstream in 0.30),
            // which would fold our no-wrap lines. Widen a nested child to the
            // measured galley width so the TextEdit can allocate its full
            // width and a real horizontal scrollbar appears. The measurement
            // lands one frame later — imperceptible on file open.
            let width_key = egui::Id::new("sftp_editor_content_w").with(id_side);
            let measured = ui.ctx().data_mut(|d| d.get_temp::<f32>(width_key).unwrap_or(0.0));

            let scroll_out = ui.allocate_new_ui(egui::UiBuilder::new().max_rect(edit_rect), |ui| {
                egui::ScrollArea::both()
                    .id_salt(egui::Id::new("sftp_editor_scroll").with(id_side))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let viewport = ui.available_rect_before_wrap();
                        let content_w = viewport.width().max(measured + 12.0);
                        let child_rect = egui::Rect::from_min_size(
                            viewport.min,
                            egui::vec2(content_w, viewport.height()),
                        );
                        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(child_rect), |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut editor.content)
                                    .id(egui::Id::new("sftp_editor_content").with(id_side))
                                    .desired_width(f32::INFINITY)
                                    // Fill the whole viewport height even for
                                    // a 1-line file (min_size applies to the
                                    // outer rect — no font-row-height math);
                                    // longer content grows past it and scrolls.
                                    .min_size(egui::vec2(
                                        0.0,
                                        (edit_rect.height() - 8.0).max(60.0),
                                    ))
                                    // Text flush with the top so gutter line
                                    // numbers line up row for row.
                                    .margin(egui::Margin {
                                        left: 8.0,
                                        right: 8.0,
                                        top: 0.0,
                                        bottom: 0.0,
                                    })
                                    .font(egui::FontId::monospace(cx.font_size))
                                    .code_editor()
                                    .lock_focus(true)
                                    .layouter(&mut layouter)
                                    .hint_text(
                                        egui::RichText::new(cx.language.t("file_content"))
                                            .color(cx.theme.hint_color())
                                            .italics(),
                                    )
                                    .text_color(cx.theme.fg_primary),
                            );
                        });
                    })
            });

            ui.ctx().data_mut(|d| d.insert_temp(width_key, galley_width.get()));

            // ── Line-number gutter — painted at fixed screen coords, synced
            //    to the scroll area's actual vertical offset this frame ──
            let offset_y = scroll_out.inner.state.offset.y;
            let line_count = editor.content.split('\n').count();
            let row_h = ui.fonts(|f| f.row_height(&font_id)).max(1.0);
            let painter = ui.painter_at(gutter_rect);
            painter.rect_filled(gutter_rect, 0.0, cx.theme.bg_secondary);
            painter.vline(
                gutter_rect.max.x - 0.5,
                gutter_rect.y_range(),
                egui::Stroke::new(1.0, cx.theme.border),
            );
            let first = (offset_y / row_h).floor().max(0.0) as usize;
            let last = (((offset_y + edit_rect.height()) / row_h).ceil() as usize + 1)
                .min(line_count);
            for i in first..last {
                let y = gutter_rect.min.y + i as f32 * row_h + row_h * 0.5 - offset_y;
                painter.text(
                    egui::pos2(gutter_rect.max.x - 6.0, y),
                    egui::Align2::RIGHT_CENTER,
                    (i + 1).to_string(),
                    font_id.clone(),
                    cx.theme.fg_dim,
                );
            }
        }
    });

    ui.painter().hline(
        bar_rect.min.x..=bar_rect.max.x,
        bar_rect.min.y,
        egui::Stroke::new(1.0, cx.theme.border),
    );

    // ── Bottom bar (stats + Save; inert while loading or in error state —
    //    saving an empty buffer over a file that never loaded would lose data) ──
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bar_rect), |ui| {
        egui::Frame {
            fill: cx.theme.bg_secondary,
            inner_margin: egui::Margin { left: 16.0, right: 16.0, top: 8.0, bottom: 8.0 },
            ..Default::default()
        }
        .show(ui, |ui| {
            if !interactive {
                return;
            }
            ui.horizontal(|ui| {
                let line_count = editor.content.lines().count().max(1);
                let char_count = editor.content.len();
                ui.label(
                    egui::RichText::new(cx.language.tf2(
                        "lines_and_chars",
                        &line_count.to_string(),
                        &char_count.to_string(),
                    ))
                    .size(11.0)
                    .color(cx.theme.fg_dim)
                    .family(egui::FontFamily::Monospace),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(
                        egui::Button::new(
                            egui::RichText::new(cx.language.t("save")).color(cx.theme.button_text).size(12.0),
                        )
                        .fill(cx.theme.accent)
                        .rounding(6.0)
                        .min_size(egui::vec2(64.0, 28.0)),
                    )
                    .clicked()
                    {
                        out.save = Some(editor.content.clone());
                    }
                    if editor.is_dirty() {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(cx.language.t("unsaved_changes"))
                                .color(cx.theme.fg_dim)
                                .size(11.0),
                        );
                    }
                });
            });
        });
    });

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::pane::test_tab;

    fn test_window(tag: &str) -> AppWindow {
        AppWindow::new_window(
            egui::ViewportId(egui::Id::new(format!("editor-test-{}", tag))),
            "Portal",
            test_tab("Tab 1"),
            1,
        )
    }

    /// Fresh temp dir with `files` written into it (cleared first so reruns
    /// start clean; per-test tags keep parallel tests out of each other's way).
    fn temp_dir_with(tag: &str, files: &[(&str, &str)]) -> String {
        let dir = std::env::temp_dir().join(format!("portal_editor_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, content) in files {
            std::fs::write(dir.join(name), content).unwrap();
        }
        dir.to_string_lossy().to_string()
    }

    #[test]
    fn open_local_editor_lands_in_own_slot() {
        let dir = temp_dir_with("right_route", &[("r.txt", "right side")]);
        let mut window = test_window("right_route");
        window.local_browser_right.current_path = dir.clone();

        // Regression: the old dialog path read the LEFT browser on right-panel
        // double-clicks; this file only exists for the right browser, so the
        // content check proves the editor reads its own half's browser.
        open_editor_in_side(&mut window, SftpPanel::RightLocal, "r.txt");
        {
            let ed = window
                .sftp_editor_right
                .as_ref()
                .expect("right slot should hold the editor");
            assert_eq!(ed.panel, SftpPanel::RightLocal);
            assert_eq!(ed.content, "right side");
            assert_eq!(ed.file_name, "r.txt");
            assert_eq!(ed.file_path, format!("{}/r.txt", dir));
            assert!(!ed.loading);
            assert!(ed.error.is_empty());
        }
        assert!(window.sftp_editor_left.is_none());
    }

    #[test]
    fn open_missing_local_file_creates_error_editor() {
        let dir = temp_dir_with("missing", &[]);
        let mut window = test_window("missing");
        window.local_browser_left.current_path = dir;

        open_editor_in_side(&mut window, SftpPanel::LeftLocal, "nope.txt");
        let ed = window
            .sftp_editor_left
            .as_ref()
            .expect("editor still opens, in the error state");
        assert!(!ed.error.is_empty());
        assert_eq!(ed.content, "");
        assert!(!ed.loading);
    }

    #[test]
    fn reopen_replaces_editor_in_same_half() {
        let dir = temp_dir_with("replace", &[("a.txt", "AAA"), ("b.txt", "BBB")]);
        let mut window = test_window("replace");
        window.local_browser_left.current_path = dir.clone();

        open_editor_in_side(&mut window, SftpPanel::LeftLocal, "a.txt");
        open_editor_in_side(&mut window, SftpPanel::LeftLocal, "b.txt");
        let ed = window.sftp_editor_left.as_ref().unwrap();
        assert_eq!(ed.file_name, "b.txt");
        assert_eq!(ed.content, "BBB");
    }

    #[test]
    fn save_local_roundtrip() {
        let dir = temp_dir_with("save_ok", &[("a.txt", "old")]);
        let mut window = test_window("save_ok");
        window.local_browser_left.current_path = dir.clone();

        let path = format!("{}/a.txt", dir);
        execute_editor_save(&mut window, SftpPanel::LeftLocal, &path, "new content")
            .expect("save should succeed");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new content");
    }

    #[test]
    fn save_local_onto_directory_fails() {
        let dir = temp_dir_with("save_fail", &[]);
        std::fs::create_dir(format!("{}/subdir", dir)).unwrap();
        let mut window = test_window("save_fail");
        window.local_browser_left.current_path = dir.clone();

        let path = format!("{}/subdir", dir);
        assert!(execute_editor_save(&mut window, SftpPanel::LeftLocal, &path, "x").is_err());
    }

    #[test]
    fn remote_without_connection_is_safe_noop() {
        let mut window = test_window("remote_guard");

        // No remote browser connected → opening a remote file is a no-op.
        open_editor_in_side(&mut window, SftpPanel::RightRemote, "x.txt");
        assert!(window.sftp_editor_right.is_none());

        // Save without a browser is fire-and-forget → still Ok.
        assert!(
            execute_editor_save(&mut window, SftpPanel::LeftRemote, "/tmp/x.txt", "y").is_ok()
        );
        assert!(window.sftp_editor_left.is_none());
    }

    /// Headless render proof (egui `Context::run`, no backend): with a 1-line
    /// file the TextEdit still fills the half's full width AND height between
    /// the title strip and the bottom bar; with one over-long line the galley
    /// extends far past the viewport so `ScrollArea::both` offers horizontal
    /// scrolling instead of folding the line.
    #[test]
    fn editor_surface_fills_half_and_scrolls_long_lines() {
        use crate::config::{ConnectionRecord, Credential, HostEntry, Snippet};
        use crate::llm::types::LlmConfig;
        use crate::ui::i18n::Language;
        use crate::ui::pane_view::WindowContext;
        use crate::ui::theme::ThemePreset;

        let mut hosts = Vec::<HostEntry>::new();
        let mut credentials = Vec::<Credential>::new();
        let mut snippets = Vec::<Snippet>::new();
        let mut history = Vec::<ConnectionRecord>::new();
        let hosts_file = std::path::PathBuf::from("/tmp/portal-test-hosts.json");
        let credentials_file = std::path::PathBuf::from("/tmp/portal-test-credentials.json");
        let theme = ThemePreset::TokyoNight.colors();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let llm_config = LlmConfig::default();
        let host_sftp_support = std::collections::HashMap::new();
        let cx = WindowContext::new(
            &mut hosts,
            &mut credentials,
            &mut snippets,
            &mut history,
            &hosts_file,
            &credentials_file,
            &theme,
            Language::English,
            &runtime,
            13.0,
            &llm_config,
            &host_sftp_support,
        );

        let content = std::cell::RefCell::new("one short line\n".to_string());
        let ctx = egui::Context::default();
        let content_id = egui::Id::new("sftp_editor_content").with("left");
        let render_once = |ctx: &egui::Context| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 800.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::none())
                    .show(ctx, |ui| {
                        let full = ui.available_rect_before_wrap();
                        // Left half of the fixed 50/50 split (mirror of
                        // sftp_view's rect math).
                        let half_rect = egui::Rect::from_min_size(
                            full.min,
                            egui::vec2(full.width() / 2.0 - 1.0, full.height()),
                        );
                        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(half_rect), |ui| {
                            let text = content.borrow().clone();
                            let mut editor = SftpEmbeddedEditor {
                                panel: SftpPanel::LeftLocal,
                                file_path: "/tmp/hi.txt".to_string(),
                                file_name: "hi.txt".to_string(),
                                directory: "/tmp".to_string(),
                                content: text.clone(),
                                original_content: text,
                                loading: false,
                                error: String::new(),
                                confirm_close: false,
                            };
                            render_embedded_editor(&mut editor, "left", ui, &cx);
                        });
                    });
            });
        };

        // ── Short content: the editor surface still fills the whole half ──
        render_once(&ctx);
        let rect = ctx
            .read_response(content_id)
            .expect("TextEdit must be rendered")
            .rect;
        // Half ≈ 599×800; chrome = title 44 + bar 40, frames ≈ 12. The
        // TextEdit box itself (not just the background) must span both axes.
        assert!(
            rect.width() >= 550.0,
            "TextEdit must fill the half WIDTH (want ≥550, got {})",
            rect.width()
        );
        assert!(
            rect.height() >= 660.0,
            "TextEdit must fill the half HEIGHT below the title (want ≥660, got {})",
            rect.height()
        );
        assert!(rect.min.y >= 24.0 && rect.max.y <= 800.0 - 44.0 + 1.0,
            "TextEdit must sit between the natural title strip and bottom bar, got {:?}", rect);

        // ── One 20 000-char line: no folding — the galley extends far past
        //    the viewport so the ScrollArea offers horizontal scrolling.
        //    The widened child converges one frame later, so render twice. ──
        *content.borrow_mut() = "x".repeat(20_000);
        render_once(&ctx);
        render_once(&ctx);
        let rect = ctx
            .read_response(content_id)
            .expect("TextEdit must be rendered")
            .rect;
        assert!(
            rect.width() > 5000.0,
            "over-long line must extend past the viewport for horizontal scrolling (got {})",
            rect.width()
        );
        assert!(
            rect.height() >= 660.0,
            "height fill must survive long content (got {})",
            rect.height()
        );
    }
}
