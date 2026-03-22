//! # Application Module
//!
//! This module contains the main application structure and core logic.

mod tab_management;

use crate::config::{HostEntry, Credential, ConnectionRecord, ShortcutAction, Snippet};
use crate::sftp::{LocalBrowser, SftpBrowser};
use crate::ui::types::{
    BatchExecutionState, HostFilter, CredentialDialog, AddHostDialog, AddTunnelDialog,
    AppView, SftpContextMenu, SftpRenameDialog, SftpNewFolderDialog,
    SftpNewFileDialog, SftpConfirmDelete, SftpEditorDialog, SftpErrorDialog,
    KeychainDeleteRequest, TerminalSession, SnippetViewState,
};
use crate::ui::pane::{Tab, DetachedWindow, TabDragState};
use crate::ui::input::ShortcutResolver;
use crate::ui::{ThemeColors, ThemePreset, Language};
use eframe::egui;
use std::path::PathBuf;

#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl};

/// The main application structure for Portal terminal emulator
pub struct PortalApp {
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    pub current_view: AppView,
    pub hosts: Vec<HostEntry>,
    pub hosts_file: PathBuf,
    pub credentials: Vec<Credential>,
    pub credentials_file: PathBuf,
    pub next_id: usize,
    pub add_host_dialog: AddHostDialog,
    pub host_filter: HostFilter,
    pub host_to_delete: Option<usize>,
    pub confirm_delete_host: Option<usize>,
    pub ime_composing: bool,
    pub ime_preedit: String,
    pub runtime: tokio::runtime::Runtime,
    // SFTP browser
    pub sftp_browser_left: Option<SftpBrowser>,  // Left panel SFTP connection
    pub sftp_browser: Option<SftpBrowser>,       // Right panel SFTP connection
    pub local_browser_left: LocalBrowser,        // Left panel local browser
    pub local_browser_right: LocalBrowser,       // Right panel local browser
    pub left_panel_is_local: bool,               // true = local, false = remote (left panel)
    pub right_panel_is_local: bool,              // true = local, false = remote (right panel)
    pub sftp_context_menu: Option<SftpContextMenu>,
    pub sftp_rename_dialog: Option<SftpRenameDialog>,
    pub sftp_new_folder_dialog: Option<SftpNewFolderDialog>,
    pub sftp_new_file_dialog: Option<SftpNewFileDialog>,
    pub sftp_confirm_delete: Option<SftpConfirmDelete>,
    pub sftp_editor_dialog: Option<SftpEditorDialog>,
    pub sftp_error_dialog: Option<SftpErrorDialog>,
    pub sftp_local_left_refresh_start: Option<std::time::Instant>,
    pub sftp_local_right_refresh_start: Option<std::time::Instant>,
    pub sftp_remote_refresh_start: Option<std::time::Instant>,
    pub sftp_left_remote_refresh_start: Option<std::time::Instant>,
    pub sftp_active_panel_is_local: bool,
    // Tab drag state
    pub tab_drag: TabDragState,
    // Status bar pickers
    pub selected_shell: String,
    pub selected_encoding: String,
    // Detached tab windows
    pub detached_windows: Vec<DetachedWindow>,
    pub next_viewport_id: u32,
    // Main window hidden (still running for detached windows)
    pub main_window_hidden: bool,
    // Broadcast state
    #[allow(dead_code)]
    pub broadcast_state: crate::ui::types::BroadcastState,
    // Batch execution state
    pub batch_execution: BatchExecutionState,
    // Keychain
    pub keychain_confirm_delete: Option<KeychainDeleteRequest>,
    pub credential_dialog: CredentialDialog,
    // Tunnels
    pub add_tunnel_dialog: AddTunnelDialog,
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
    // Command Snippets
    pub snippets: Vec<Snippet>,
    pub snippet_view_state: SnippetViewState,
}

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
        // Use default theme preset (Tokyo Night)
        let theme_preset = ThemePreset::TokyoNight;
        let theme = theme_preset.colors();

        let mut fonts = egui::FontDefinitions::default();

        // Load custom font if specified
        if !custom_font_path.is_empty() {
            if let Ok(font_data) = std::fs::read(&custom_font_path) {
                fonts.font_data.insert(
                    "CustomFont".to_owned(),
                    egui::FontData::from_owned(font_data),
                );
                fonts.families
                    .entry(egui::FontFamily::Monospace)
                    .or_insert_with(Vec::new)
                    .insert(0, "CustomFont".to_owned());
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Ok(font_data) = std::fs::read("/System/Library/Fonts/Monaco.dfont") {
                fonts.font_data.insert(
                    "Monaco".to_owned(),
                    egui::FontData::from_owned(font_data),
                );
                fonts.families
                    .entry(egui::FontFamily::Monospace)
                    .or_insert_with(Vec::new)
                    .push("Monaco".to_owned());
            }

            // CJK fallback font for Chinese/Japanese/Korean characters
            let cjk_paths = [
                "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
                "/System/Library/Fonts/STHeiti Medium.ttc",
                "/System/Library/Fonts/Hiragino Sans GB.ttc",
            ];
            for path in &cjk_paths {
                if let Ok(font_data) = std::fs::read(path) {
                    fonts.font_data.insert(
                        "CJK".to_owned(),
                        egui::FontData::from_owned(font_data),
                    );
                    fonts.families
                        .entry(egui::FontFamily::Monospace)
                        .or_insert_with(Vec::new)
                        .push("CJK".to_owned());
                    fonts.families
                        .entry(egui::FontFamily::Proportional)
                        .or_insert_with(Vec::new)
                        .push("CJK".to_owned());
                    break;
                }
            }
        }

        cc.egui_ctx.set_fonts(fonts);

        // Visuals will be applied on the first frame via visuals_dirty flag,
        // because eframe may override visuals set during new().

        let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        let selected_shell = std::env::var("SHELL")
            .unwrap_or_else(|_| "/bin/zsh".to_string());
        let first_tab = Tab {
            title: "Terminal 1".to_owned(),
            sessions: vec![TerminalSession::new_local(0, &selected_shell)],
            layout: crate::ui::pane::PaneNode::Terminal(0),
            focused_session: 0,
            broadcast_enabled: false,
        };

        let connection_history = crate::config::load_history();
        let snippets = crate::config::load_snippets();

        let hosts_file = crate::config::hosts_file_path();
        let mut hosts = crate::config::load_hosts(&hosts_file);

        // Load credentials and run migration
        let credentials_file = crate::config::credentials_file_path();
        let mut credentials = crate::config::load_credentials(&credentials_file);
        crate::config::migrate_hosts_to_credentials(&mut hosts, &mut credentials);
        crate::config::save_credentials(&credentials_file, &credentials);
        crate::config::save_hosts(&hosts_file, &hosts);

        Self {
            tabs: vec![first_tab],
            active_tab: 0,
            current_view: AppView::Terminal,
            hosts,
            hosts_file,
            credentials,
            credentials_file,
            next_id: 1,
            add_host_dialog: AddHostDialog::default(),
            host_filter: HostFilter::default(),
            host_to_delete: None,
            confirm_delete_host: None,
            ime_composing: false,
            ime_preedit: String::new(),
            runtime,
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
            sftp_active_panel_is_local: true,  // Track which panel has focus
            tab_drag: TabDragState::default(),

            selected_shell,
            selected_encoding: "UTF-8".to_string(),
            detached_windows: Vec::new(),
            next_viewport_id: 0,
            main_window_hidden: false,
            broadcast_state: crate::ui::types::BroadcastState::default(),
            batch_execution: BatchExecutionState::default(),
            keychain_confirm_delete: None,
            credential_dialog: CredentialDialog::default(),
            add_tunnel_dialog: AddTunnelDialog::default(),
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
            snippet_view_state: SnippetViewState::default(),
        }
    }
}
