//! Portal - Modern Terminal Emulator with egui
//! Termius-inspired UI with native terminal input

#![allow(unexpected_cfgs)]  // Suppress warnings from objc crate's macros

mod app;
mod config;
mod sftp;
mod ssh;
mod terminal;
mod ui;

use app::PortalApp;
use std::time::Duration;

use ui::*;

impl eframe::App for PortalApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(16));

        // Update main window title based on current view and active tab
        if let Some(window) = self.windows.first() {
            let title = match window.current_view {
                AppView::Terminal => {
                    if let Some(tab) = window.tabs.get(window.active_tab) {
                        tab.title.clone()
                    } else {
                        "Terminal".to_string()
                    }
                }
                AppView::Hosts => self.language.t("hosts").to_string(),
                AppView::Sftp => self.language.t("sftp").to_string(),
                AppView::Keychain => self.language.t("keychain").to_string(),
                AppView::Settings => self.language.t("settings").to_string(),
                AppView::Snippets => "Snippets".to_string(),
                AppView::Tunnels => "Tunnels".to_string(),
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }

        // Apply visuals/fonts if dirty
        if self.visuals_dirty {
            self.apply_visuals(ctx);
            self.visuals_dirty = false;
        }
        if self.fonts_dirty {
            self.apply_fonts(ctx);
        }

        // ── Main window close handling ──
        // If the user closes the main window but other windows still exist,
        // hide the main window instead of exiting. Exit only when all windows are gone.
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.windows.len() > 1 {
                // Cancel the close and hide the main window
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                if let Some(main_window) = self.windows.first_mut() {
                    main_window.close_requested = true;  // Mark as hidden
                }
            } else {
                // Only one window - clean up and exit
                self.cleanup_sessions();
            }
        }

        // ── Render non-main windows (using show_viewport_immediate) ─────────────────────────
        let mut pending_detach: Vec<(usize, usize)> = Vec::new();
        let num_windows = self.windows.len();

        // Render main window first to collect its pending_detach
        if !self.windows.first().map_or(true, |w| w.close_requested) {
            let result = self.render_window_content(ctx, 0, false);
            pending_detach.extend(result.pending_detach);
        }

        // Then render non-main windows
        for i in 1..num_windows {
            let viewport_id = self.windows[i].viewport_id;
            let builder = egui::ViewportBuilder::default()
                .with_title("Portal")
                .with_inner_size([1200.0, 800.0])
                .with_min_inner_size([600.0, 400.0]);
            ctx.show_viewport_immediate(viewport_id, builder, |ctx, _class| {
                // Update window title
                let dw = &self.windows[i];
                let title = match dw.current_view {
                    AppView::Terminal => dw.tabs.get(dw.active_tab).map(|t| t.title.clone()).unwrap_or_else(|| "Terminal".to_string()),
                    AppView::Hosts => self.language.t("hosts").to_string(),
                    AppView::Sftp => self.language.t("sftp").to_string(),
                    AppView::Keychain => self.language.t("keychain").to_string(),
                    AppView::Settings => self.language.t("settings").to_string(),
                    AppView::Snippets => "Snippets".to_string(),
                    AppView::Tunnels => "Tunnels".to_string(),
                };
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
                ctx.request_repaint_after(Duration::from_millis(16));

                if ctx.input(|i| i.viewport().close_requested()) {
                    self.windows[i].close_requested = true;
                }

                // Use unified window content rendering
                let result = self.render_window_content(ctx, i, true);
                pending_detach.extend(result.pending_detach);
            });
        }

        // Remove closed windows (but keep at least one - the main window)
        let main_window_closed = self.windows.first().map_or(false, |w| w.close_requested);
        self.windows.retain(|w| !w.close_requested);

        // Process deferred tab detaches (all windows)
        // Process in reverse order so indices stay valid
        for &(win_idx, tab_idx) in pending_detach.iter().rev() {
            if win_idx < self.windows.len() {
                let dw = &mut self.windows[win_idx];
                if dw.tabs.len() > 1 && tab_idx < dw.tabs.len() {
                    let tab = dw.tabs.remove(tab_idx);
                    if dw.active_tab >= dw.tabs.len() {
                        dw.active_tab = dw.tabs.len().saturating_sub(1);
                    }
                    let id_val = self.next_viewport_id;
                    self.next_viewport_id += 1;
                    let viewport_id = egui::ViewportId::from_hash_of(format!("window_{}", id_val));
                    let next_id = dw.next_id;
                    dw.next_id += 100;
                    self.windows.push(AppWindow {
                        viewport_id,
                        title: tab.title.clone(),
                        tabs: vec![tab],
                        active_tab: 0,
                        current_view: AppView::Terminal,
                        close_requested: false,
                        ime_composing: false,
                        ime_preedit: String::new(),
                        next_id,
                        tab_drag: TabDragState::default(),
                        broadcast_state: BroadcastState::default(),
                    });
                }
            }
        }

        // If main window was closed and all other windows are closed, exit the app
        if main_window_closed && self.windows.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // If main window was closed but other windows exist, skip rendering the rest
        if main_window_closed {
            return;
        }

        // ── Delete Confirmation Dialog ──────────────────
        if let Some(idx) = self.confirm_delete_host {
            let host_name = self.hosts.get(idx).map(|h| h.name.clone()).unwrap_or_default();
            let mut open = true;
            egui::Window::new(self.language.t("delete_host"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .fixed_size([340.0, 0.0])
                .title_bar(false)
                .frame(egui::Frame {
                    fill: self.theme.bg_secondary,
                    rounding: egui::Rounding::same(8.0),
                    inner_margin: egui::Margin::same(20.0),
                    stroke: egui::Stroke::new(1.0, self.theme.border),
                    shadow: egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 4.0),
                        blur: 20.0,
                        spread: 2.0,
                        color: egui::Color32::from_black_alpha(80),
                    },
                    ..Default::default()
                })
                .show(ctx, |ui| {
                    // Warning icon + title
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("\u{26A0}").size(18.0).color(self.theme.red));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(self.language.t("delete_host")).size(15.0).color(self.theme.fg_primary).strong());
                    });
                    ui.add_space(10.0);

                    ui.label(
                        egui::RichText::new(self.language.tf("delete_confirm", &host_name))
                            .color(self.theme.fg_primary).size(13.0)
                    );
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(self.language.t("confirm_delete"))
                            .color(self.theme.fg_dim).size(11.0)
                    );
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(egui::RichText::new(self.language.t("delete")).color(egui::Color32::WHITE).size(13.0))
                                    .fill(self.theme.red)
                                    .rounding(6.0)
                                    .min_size(egui::vec2(70.0, 32.0))
                            ).clicked() {
                                self.host_to_delete = Some(idx);
                                self.confirm_delete_host = None;
                            }
                            if ui.add(
                                egui::Button::new(egui::RichText::new(self.language.t("cancel")).color(self.theme.fg_dim).size(13.0))
                                    .fill(self.theme.bg_elevated)
                                    .rounding(6.0)
                                    .min_size(egui::vec2(70.0, 32.0))
                            ).clicked() {
                                self.confirm_delete_host = None;
                            }
                        });
                    });
                });
            if !open {
                self.confirm_delete_host = None;
            }
        }

        // Handle deferred host deletion
        if let Some(idx) = self.host_to_delete.take() {
            if idx < self.hosts.len() && !self.hosts[idx].is_local {
                config::delete_host_credentials(&self.hosts[idx]);
                self.hosts.remove(idx);
                self.save_hosts();
            }
        }

        // ── Poll SFTP browser ──────────────────────────────────────────────
        if let Some(ref mut browser) = self.sftp_browser {
            let had_transfer = browser.transfer.is_some();
            let was_download = browser.transfer.as_ref().map_or(false, |t| !t.is_upload);
            browser.poll();
            // Auto-refresh local browser after download completes
            if had_transfer && browser.transfer.is_none() && was_download {
                self.local_browser_right.refresh();
            }
            // Handle async file read results for the editor
            if let Some((path, data)) = browser.pending_file_content.take() {
                if let Some(ref mut dialog) = self.sftp_editor_dialog {
                    if dialog.loading && dialog.panel == ui::types::SftpPanel::RightRemote {
                        match String::from_utf8(data) {
                            Ok(text) => {
                                dialog.content = text.clone();
                                dialog.original_content = text;
                                dialog.loading = false;
                                dialog.file_path = path;
                            }
                            Err(_) => {
                                dialog.error = "Not valid UTF-8".to_string();
                                dialog.loading = false;
                            }
                        }
                    }
                }
            }
            if let Some((_path, size)) = browser.pending_file_too_large.take() {
                if let Some(ref mut dialog) = self.sftp_editor_dialog {
                    if dialog.loading && dialog.panel == ui::types::SftpPanel::RightRemote {
                        dialog.loading = false;
                        dialog.error = format!("File too large ({} bytes)", size);
                    }
                }
            }
        }

        // ── Poll left SFTP browser ──────────────────────────────────────────
        if let Some(ref mut browser) = self.sftp_browser_left {
            let had_transfer = browser.transfer.is_some();
            let was_download = browser.transfer.as_ref().map_or(false, |t| !t.is_upload);
            browser.poll();
            // Auto-refresh local browser after download completes
            if had_transfer && browser.transfer.is_none() && was_download {
                self.local_browser_left.refresh();
            }
            // Handle async file read results for the editor
            if let Some((path, data)) = browser.pending_file_content.take() {
                if let Some(ref mut dialog) = self.sftp_editor_dialog {
                    if dialog.loading && dialog.panel == ui::types::SftpPanel::LeftRemote {
                        match String::from_utf8(data) {
                            Ok(text) => {
                                dialog.content = text.clone();
                                dialog.original_content = text;
                                dialog.loading = false;
                                dialog.file_path = path;
                            }
                            Err(_) => {
                                dialog.error = "Not valid UTF-8".to_string();
                                dialog.loading = false;
                            }
                        }
                    }
                }
            }
            if let Some((_path, size)) = browser.pending_file_too_large.take() {
                if let Some(ref mut dialog) = self.sftp_editor_dialog {
                    if dialog.loading && dialog.panel == ui::types::SftpPanel::LeftRemote {
                        dialog.loading = false;
                        dialog.error = format!("File too large ({} bytes)", size);
                    }
                }
            }
        }

    }
}

fn load_app_icon() -> Option<egui::IconData> {
    let png_bytes = include_bytes!("../assets/portal-icon-1024.png");
    let decoder = eframe::icon_data::from_png_bytes(png_bytes);
    decoder.ok()
}

fn main() -> eframe::Result<()> {
    env_logger::init();

    // Set up signal handler for clean PTY cleanup on exit
    // This prevents PTY resource leaks when the app is terminated
    #[cfg(unix)]
    {
        use std::sync::atomic::{AtomicBool, Ordering};

        // Flag to track if we've already cleaned up
        static CLEANUP_DONE: AtomicBool = AtomicBool::new(false);

        // Spawn a dedicated cleanup thread that waits for signals
        std::thread::spawn(|| {
            use signal_hook::consts::signal::*;
            use signal_hook::iterator::Signals;

            let signals = Signals::new([SIGTERM, SIGINT, SIGHUP]).ok();

            if let Some(mut sig) = signals {
                for _ in sig.forever() {
                    if !CLEANUP_DONE.swap(true, Ordering::SeqCst) {
                        // Kill all zsh -l processes spawned by portal
                        unsafe {
                            let cmd = std::ffi::CString::new("pkill").unwrap();
                            let arg1 = std::ffi::CString::new("-9").unwrap();
                            let arg2 = std::ffi::CString::new("-f").unwrap();
                            let arg3 = std::ffi::CString::new("/bin/zsh -l").unwrap();
                            libc::execvp(cmd.as_ptr(), [cmd.as_ptr(), arg1.as_ptr(), arg2.as_ptr(), arg3.as_ptr(), std::ptr::null()].as_ptr());
                        }
                    }
                    // Exit after cleanup
                    std::process::exit(0);
                }
            }
        });
    }

    // Set macOS activation policy to Regular so the app appears in Dock
    #[cfg(target_os = "macos")]
    {
        use cocoa::appkit::{NSApp, NSApplication, NSApplicationActivationPolicy};
        use cocoa::base::{id, YES};

        unsafe {
            let app: id = NSApp();
            app.setActivationPolicy_(NSApplicationActivationPolicy::NSApplicationActivationPolicyRegular);
            app.activateIgnoringOtherApps_(YES);
        }
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1400.0, 900.0])
        .with_min_inner_size([800.0, 600.0])
        .with_title("Portal");

    if let Some(icon) = load_app_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }

    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "Portal",
        options,
        Box::new(|cc| Ok(Box::new(PortalApp::new(cc)))),
    )
}
