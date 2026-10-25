//! # Application Module
//!
//! This module contains the main application structure and core logic.

#![allow(unexpected_cfgs)]

mod session_restore;
mod tab_management;
mod window_content;

use crate::config::{HostEntry, Credential, ConnectionRecord, ShortcutAction, Snippet};
use crate::ui::types::session::TerminalSession;
use crate::ui::pane::{Tab, AppWindow};
use crate::ui::input::ShortcutResolver;
use crate::ui::{ThemeColors, ThemePreset, Language, fonts};
use std::path::PathBuf;

#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl};

/// The main application structure for Portal terminal emulator
pub struct PortalApp {
    // Unified window management - all windows are equal
    pub windows: Vec<AppWindow>,
    pub next_viewport_id: u32,
    // Shared data (used by all windows)
    pub hosts: Vec<HostEntry>,
    pub hosts_file: PathBuf,
    pub credentials: Vec<Credential>,
    pub credentials_file: PathBuf,
    pub runtime: tokio::runtime::Runtime,
    pub snippets: Vec<Snippet>,

    // Settings
    pub theme: ThemeColors,
    pub theme_preset: ThemePreset, // For UI selection only, not persisted
    pub language: Language,
    pub font_size: f32,
    pub custom_font_path: String,
    pub scrollback_limit_mb: u64,
    pub ssh_keepalive_interval: u32,
    pub fonts_dirty: bool,
    pub visuals_dirty: bool,
    pub connection_history: Vec<ConnectionRecord>,
    pub shortcut_resolver: ShortcutResolver,
    pub recording_shortcut: Option<ShortcutAction>,
    // Update checking
    pub update_state: std::sync::Arc<std::sync::Mutex<crate::update::UpdateState>>,
    pub next_update_check: std::time::Instant,
    pub check_for_updates: bool,
    pub last_dismissed_update_version: Option<String>,
    /// Mirror of PortalSettings.auto_record_sessions (Settings → Terminal).
    pub auto_record_sessions: bool,
    /// Mirror of PortalSettings.recordings_retention_days (Settings → Terminal).
    pub recordings_retention_days: u32,
    /// Mirror of PortalSettings.recordings_max_total_mb (Settings → Terminal).
    pub recordings_max_total_mb: u32,
    /// Mirror of PortalSettings.restore_sessions (Settings → General).
    pub restore_sessions: bool,
    /// Mirror of PortalSettings.zmodem_download_dir (Settings → General).
    pub zmodem_download_dir: String,
    /// Next periodic retention sweep (hourly; startup already swept).
    pub next_retention_check: std::time::Instant,
    // LLM integration
    pub llm_config: crate::llm::types::LlmConfig,
    /// Draft API key text in the settings input (never shown back once stored).
    pub llm_key_draft: String,
    /// Active settings page section (left-hand nav, non-persisted).
    pub settings_section: SettingsSection,
    // Session restore
    /// Tab/session structure changed since the last snapshot save.
    pub sessions_dirty: bool,
    /// Probed SFTP support per host ("host:port" → capability), harvested
    /// from live SSH sessions each frame. Not persisted — a fresh connect
    /// re-probes, so restarts just show no badge until then.
    pub host_sftp_support:
        std::collections::HashMap<String, crate::ssh::capability::HostCapability>,
    /// When the debounced snapshot saver last wrote (or app start).
    pub last_session_save: std::time::Instant,
}

/// Categories shown in the Settings page's left-hand navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Appearance,
    Terminal,
    Ai,
    Shortcuts,
    General,
}

/// Minimum interval between session-state writes. SIGTERM cannot run
/// snapshot code (the process replaces itself), so the debounce bounds
/// crash/TERM loss to this window.
const SESSION_SAVE_DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(20);

impl PortalApp {
    /// Create a new PortalApp instance
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // On macOS, ensure windows appear in Dock menu
        #[cfg(target_os = "macos")]
        {
            use cocoa::appkit::NSApp;
            use cocoa::base::{id, YES};

            #[allow(unexpected_cfgs)]  // Suppress warning from objc crate's msg_send! macro
            unsafe {
                let app: id = NSApp();
                // Force the app to be active and show in Dock
                let _: id = msg_send![app, activateIgnoringOtherApps: YES];
            }
        }

        // Load settings
        let settings = crate::config::load_settings();
        let language = Language::from_id(&settings.language);
        let font_size = settings.font_size;
        let custom_font_path = settings.custom_font_path.clone().unwrap_or_default();
        let scrollback_limit_mb = settings.scrollback_limit_mb;
        let ssh_keepalive_interval = settings.ssh_keepalive_interval;
        let shortcut_resolver = ShortcutResolver::new(settings.keyboard_shortcuts.clone());
        let check_for_updates = settings.check_for_updates;
        let last_dismissed_update_version = settings.last_dismissed_update_version.clone();
        let auto_record_sessions = settings.auto_record_sessions;
        let recordings_retention_days = settings.recordings_retention_days;
        let recordings_max_total_mb = settings.recordings_max_total_mb;
        let restore_sessions = settings.restore_sessions;
        let zmodem_download_dir = settings
            .zmodem_download_dir
            .clone()
            .unwrap_or_default();

        // Retention sweep at startup: with auto-record on, recordings would
        // otherwise accumulate forever. Cheap no-op when both limits are 0.
        let removed = crate::terminal::cast::enforce_retention(
            &crate::terminal::cast::recordings_dir(),
            recordings_retention_days,
            recordings_max_total_mb,
        );
        if !removed.is_empty() {
            log::info!("retention: removed {} recording(s) at startup", removed.len());
        }
        let llm_config = settings.llm.clone();
        // Use default theme preset (Tokyo Night)
        let theme_preset = ThemePreset::TokyoNight;
        let theme = theme_preset.colors();

        // Load fonts using shared utility
        let fonts = fonts::load_fonts(&custom_font_path);
        cc.egui_ctx.set_fonts(fonts);

        // Install the process-wide repaint notifier so reader threads (PTY/SSH)
        // can wake the UI when terminal output arrives — the app no longer
        // repaints unconditionally at 60 FPS.
        let repaint_ctx = cc.egui_ctx.clone();
        crate::repaint::set_global_repaint_notifier(std::sync::Arc::new(move || {
            repaint_ctx.request_repaint();
        }));

        // Visuals will be applied on the first frame via visuals_dirty flag,
        // because eframe may override visuals set during new().

        let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

        // Hosts/credentials must load BEFORE tab creation: session restore
        // resolves SSH sessions against them.
        let connection_history = crate::config::load_history();
        let snippets = crate::config::load_snippets(&crate::config::snippets_file_path());

        let hosts_file = crate::config::hosts_file_path();
        let mut hosts = crate::config::load_hosts(&hosts_file);

        // Load credentials and run migration
        let credentials_file = crate::config::credentials_file_path();
        let mut credentials = crate::config::load_credentials(&credentials_file);
        crate::config::migrate_hosts_to_credentials(&mut hosts, &mut credentials);
        crate::config::save_credentials(&credentials_file, &credentials);
        crate::config::save_hosts(&hosts_file, &hosts);

        // Restore the previous session's tabs, or start with one local shell.
        let (first_tabs, restored_active_tab, restored_next_id) =
            match session_restore::restore_root_tabs(&hosts, &credentials, &runtime) {
                Some((tabs, active_tab, next_id)) => (tabs, Some(active_tab), next_id),
                None => {
                    let default_shell = std::env::var("SHELL")
                        .unwrap_or_else(|_| "/bin/zsh".to_string());
                    let first_tab = Tab {
                        title: "Terminal 1".to_owned(),
                        sessions: vec![TerminalSession::new_local(0, &default_shell)],
                        layout: crate::ui::pane::PaneNode::Terminal(0),
                        focused_session: 0,
                        broadcast_enabled: false,
                        tools_drawer_open: false,
                        tools_tab: 0,
                        pending_snippet: None,
                        ai_chat: std::sync::Arc::new(std::sync::Mutex::new(
                            crate::ui::state::ai_chat::AiChatState::default(),
                        )),
                        pending_ai_command: None,
                    };
                    (vec![first_tab], None, 1)
                }
            };

        // Create the first (main) window
        let mut first_tab_iter = first_tabs.into_iter();
        let first_window_tab = first_tab_iter.next().expect("at least one tab");
        let mut first_window =
            AppWindow::new_window(egui::ViewportId::ROOT, "Portal", first_window_tab, restored_next_id);
        first_window.tabs.extend(first_tab_iter);
        if let Some(active) = restored_active_tab {
            first_window.active_tab = active.min(first_window.tabs.len() - 1);
        }

        Self {
            windows: vec![first_window],
            next_viewport_id: 1,
            hosts,
            hosts_file,
            credentials,
            credentials_file,
            runtime,
            theme,
            theme_preset,
            language,
            font_size,
            custom_font_path,
            scrollback_limit_mb,
            ssh_keepalive_interval,
            fonts_dirty: false,
            visuals_dirty: true,
            connection_history,
            shortcut_resolver,
            recording_shortcut: None,
            snippets,
            update_state: std::sync::Arc::new(std::sync::Mutex::new(crate::update::UpdateState::Idle)),
            next_update_check: std::time::Instant::now(),
            check_for_updates,
            last_dismissed_update_version,
            auto_record_sessions,
            recordings_retention_days,
            recordings_max_total_mb,
            restore_sessions,
            zmodem_download_dir,
            // First periodic sweep one hour after launch (startup already swept).
            next_retention_check: std::time::Instant::now() + std::time::Duration::from_secs(3600),
            llm_config,
            llm_key_draft: String::new(),
            settings_section: SettingsSection::General,
            sessions_dirty: false,
            host_sftp_support: std::collections::HashMap::new(),
            last_session_save: std::time::Instant::now(),
        }
    }

    /// Mark the tab/session structure changed — the debounced saver will
    /// snapshot it within [`SESSION_SAVE_DEBOUNCE`].
    pub fn mark_sessions_dirty(&mut self) {
        self.sessions_dirty = true;
    }

    /// Pull capability probe results from live SSH sessions into the
    /// hosts-list cache (keyed "host:port"). Cheap: a few atomic loads
    /// per frame.
    pub fn harvest_host_capabilities(&mut self) {
        for w in &self.windows {
            for tab in &w.tabs {
                for s in &tab.sessions {
                    if let crate::ui::types::session::SessionKind::Ssh(sess, host, _) = &s.kind {
                        let cap = sess.sftp_support();
                        if cap == crate::ssh::capability::HostCapability::Unknown {
                            continue;
                        }
                        let key = format!("{}:{}", host.host, host.port);
                        if self.host_sftp_support.get(&key) != Some(&cap) {
                            self.host_sftp_support.insert(key, cap);
                        }
                    }
                }
            }
        }
    }

    /// Debounced session-state save: called once per frame from update().
    /// SIGTERM/kill paths can't run snapshot code, so periodic debounced
    /// writes cap the worst-case loss at the debounce window.
    pub fn maybe_save_sessions(&mut self) {
        if !self.sessions_dirty {
            return;
        }
        if self.last_session_save.elapsed() < SESSION_SAVE_DEBOUNCE {
            return;
        }
        self.save_sessions_now();
    }

    /// Snapshot + write immediately (also resets the dirty flag).
    pub fn save_sessions_now(&mut self) {
        let state = session_restore::snapshot(&self.windows);
        crate::config::session_state::save_session_state(
            &crate::config::session_state::session_state_file_path(),
            &state,
        );
        self.sessions_dirty = false;
        self.last_session_save = std::time::Instant::now();
    }

    /// Called once per frame. When the toggle is on and enough time has passed
    /// since the last check, kick off a background GitHub check — but never
    /// while a check/download is in flight or a dialog is showing.
    pub fn maybe_check_for_updates(&mut self) {
        if !self.check_for_updates {
            return;
        }
        if std::time::Instant::now() < self.next_update_check {
            return;
        }
        {
            let guard = self.update_state.lock().expect("update_state lock");
            if !matches!(*guard, crate::update::UpdateState::Idle | crate::update::UpdateState::Error(_)) {
                return;
            }
        }
        self.next_update_check = std::time::Instant::now() + std::time::Duration::from_secs(24 * 3600);
        if let Ok(mut guard) = self.update_state.lock() {
            *guard = crate::update::UpdateState::Checking;
        }
        let arc = std::sync::Arc::clone(&self.update_state);
        std::thread::spawn(move || {
            let next = crate::update::check_latest(env!("CARGO_PKG_VERSION"));
            if let Ok(mut g) = arc.lock() {
                *g = next;
            }
        });
    }

    /// Transition from `Available` to `Downloading` and spawn the download +
    /// reveal on a background thread.
    pub fn begin_update_download(&mut self) {
        let (version, url, asset_name) = {
            let guard = self.update_state.lock().expect("update_state lock");
            match &*guard {
                crate::update::UpdateState::Available { version, url, asset_name } => {
                    (version.clone(), url.clone(), asset_name.clone())
                }
                _ => return,
            }
        };
        if let Ok(mut guard) = self.update_state.lock() {
            *guard = crate::update::UpdateState::Downloading {
                version: version.clone(),
                asset_name: asset_name.clone(),
                received: 0,
                total: None,
            };
        }
        let arc = std::sync::Arc::clone(&self.update_state);
        let v = version.clone();
        let a = asset_name.clone();
        std::thread::spawn(move || {
            let result = crate::update::download_and_reveal(
                &version,
                &asset_name,
                &url,
                |received, total| {
                    if let Ok(mut g) = arc.lock() {
                        *g = crate::update::UpdateState::Downloading {
                            version: v.clone(),
                            asset_name: a.clone(),
                            received,
                            total,
                        };
                    }
                },
            );
            if let Ok(mut g) = arc.lock() {
                *g = match result {
                    Ok(path) => crate::update::UpdateState::Ready { version, path },
                    Err(e) => crate::update::UpdateState::Error(e),
                };
            }
        });
    }

    /// Record a dismissed version and return the update state to `Idle`.
    pub fn dismiss_update(&mut self, version: &str) {
        self.last_dismissed_update_version = Some(version.to_string());
        self.save_settings_to_disk();
        if let Ok(mut guard) = self.update_state.lock() {
            *guard = crate::update::UpdateState::Idle;
        }
    }

    /// Clean up all terminal sessions on exit to prevent PTY leaks
    /// This is critical because PTY devices are limited system resources.
    /// Dropping the sessions runs `RealPtySession`'s `Drop`, which kills the
    /// child and reaps it.
    pub fn cleanup_sessions(&mut self) {
        // Snapshot BEFORE clearing: the whole point of a clean exit is that
        // the next launch puts the tabs back.
        if self.restore_sessions {
            self.save_sessions_now();
        }
        for window in &mut self.windows {
            for tab in &mut window.tabs {
                tab.sessions.clear();
            }
        }
        // Remove the single-instance IPC socket so "open here" forwards fail
        // fast (and the next launch rebinds cleanly) instead of hanging on a
        // dead socket.
        #[cfg(unix)]
        crate::ipc::cleanup();
    }
}

impl Drop for PortalApp {
    fn drop(&mut self) {
        // Ensure all PTY sessions are properly cleaned up
        self.cleanup_sessions();
    }
}
