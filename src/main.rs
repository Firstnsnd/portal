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
use sftp::LocalBrowser;
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

        // ── Window close handling ─────────────────────────────────────────────────────────────
        // IMPORTANT: eframe/egui architecture limitation
        //
        // The first window (index 0) is the ROOT viewport created by eframe::run_native().
        // - eframe's `update()` method is bound to the root viewport
        // - When root viewport closes, eframe STOPS calling `update()`
        // - Other windows (created via show_viewport_immediate) depend on `update()` for rendering
        //
        // Therefore, we CANNOT simply remove the first window when others exist:
        // - No root viewport → no `update()` calls → child windows can't render → app exits
        //
        // Solution: Keep first window alive but HIDDEN when other windows exist.
        // This is the standard approach for multi-window apps in eframe.
        //
        // Alternatives (not implemented):
        // - Use native window APIs directly: complex, cross-platform issues
        // - Switch to wry+tauri: more control but different architecture
        //
        let has_multiple_windows = self.windows.len() > 1;
        let first_window = self.windows.first();
        let first_window_hidden = first_window.map_or(false, |w| w.close_requested);

        // Handle first window close request
        if ctx.input(|i| i.viewport().close_requested()) {
            if has_multiple_windows {
                // Cancel close, hide first window, keep it alive
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                if let Some(w) = &mut self.windows.first_mut() {
                    w.close_requested = true;
                }
            } else {
                // Last window - let it close
                self.cleanup_sessions();
            }
        }

        // Keep first window alive (but hidden) while other windows exist
        if first_window_hidden && !self.windows.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }

        // ── Render windows ───────────────────────────────────────────────────────────────
        let mut pending_detach: Vec<(usize, usize)> = Vec::new();

        // Render first window content if visible
        if !first_window_hidden {
            if let Some(w) = self.windows.first() {
                // Update title
                let title = match w.current_view {
                    AppView::Terminal => {
                        w.tabs.get(w.active_tab).map(|t| t.title.clone()).unwrap_or_else(|| "Terminal".to_string())
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
            let result = self.render_window_content(ctx, 0, false);
            pending_detach.extend(result.pending_detach);
        }

        // Render additional windows (using show_viewport_immediate)
        for i in 1..self.windows.len() {
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

        // Remove closed windows (except first window which is kept alive but hidden)
        let mut child_windows_closed = false;
        for i in (1..self.windows.len()).rev() {
            if self.windows[i].close_requested {
                self.windows.remove(i);
                child_windows_closed = true;
            }
        }

        // If all child windows closed and first window is hidden, show it again
        if child_windows_closed && self.windows.len() == 1 && self.windows[0].close_requested {
            self.windows[0].close_requested = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }

        // If all windows are closed, exit app
        if self.windows.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Process deferred tab detaches
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
                        // SFTP state (new window starts with fresh SFTP state)
                        sftp_browser_left: None,
                        sftp_browser: None,
                        local_browser_left: LocalBrowser::new(),
                        local_browser_right: LocalBrowser::new(),
                        left_panel_is_local: true,
                        right_panel_is_local: false,
                        sftp_context_menu: None,
                        sftp_rename_dialog: None,
                        sftp_new_folder_dialog: None,
                        sftp_new_file_dialog: None,
                        sftp_confirm_delete: None,
                        sftp_editor_dialog: None,
                        sftp_error_dialog: None,
                        sftp_local_left_refresh_start: None,
                        sftp_local_right_refresh_start: None,
                        sftp_remote_refresh_start: None,
                        sftp_left_remote_refresh_start: None,
                        sftp_active_panel_is_local: true,
                        // Page-related dialogs (new window starts with fresh state)
                        add_host_dialog: AddHostDialog::default(),
                        credential_dialog: CredentialDialog::default(),
                        snippet_view_state: SnippetViewState::default(),
                        host_filter: HostFilter::default(),
                        confirm_delete_host: None,
                        add_tunnel_dialog: AddTunnelDialog::default(),
                    });
                }
            }
        }

        // ── Poll SFTP browsers (per-window) ──────────────────────────────────────────────
        for window_idx in 0..self.windows.len() {
            let window = &mut self.windows[window_idx];

            // Poll right panel SFTP browser
            if let Some(ref mut browser) = window.sftp_browser {
                let had_transfer = browser.transfer.is_some();
                let was_download = browser.transfer.as_ref().map_or(false, |t| !t.is_upload);
                browser.poll();
                // Auto-refresh local browser after download completes
                if had_transfer && browser.transfer.is_none() && was_download {
                    window.local_browser_right.refresh();
                }
                // Handle async file read results for the editor
                if let Some((path, data)) = browser.pending_file_content.take() {
                    if let Some(ref mut dialog) = window.sftp_editor_dialog {
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
                    if let Some(ref mut dialog) = window.sftp_editor_dialog {
                        if dialog.loading && dialog.panel == ui::types::SftpPanel::RightRemote {
                            dialog.loading = false;
                            dialog.error = format!("File too large ({} bytes)", size);
                        }
                    }
                }
            }

            // Poll left panel SFTP browser
            if let Some(ref mut browser) = window.sftp_browser_left {
                let had_transfer = browser.transfer.is_some();
                let was_download = browser.transfer.as_ref().map_or(false, |t| !t.is_upload);
                browser.poll();
                // Auto-refresh local browser after download completes
                if had_transfer && browser.transfer.is_none() && was_download {
                    window.local_browser_left.refresh();
                }
                // Handle async file read results for the editor
                if let Some((path, data)) = browser.pending_file_content.take() {
                    if let Some(ref mut dialog) = window.sftp_editor_dialog {
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
                    if let Some(ref mut dialog) = window.sftp_editor_dialog {
                        if dialog.loading && dialog.panel == ui::types::SftpPanel::LeftRemote {
                            dialog.loading = false;
                            dialog.error = format!("File too large ({} bytes)", size);
                        }
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
