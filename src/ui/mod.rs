pub mod theme;
pub mod i18n;
pub mod input;
pub mod types;
pub mod pane;
pub mod terminal_render;
pub mod sftp_view;
pub mod hosts_view;
pub mod settings_view;
pub mod keychain_view;

// Re-export all public types for convenient access via `use ui::*`
pub use theme::{ThemeColors, ThemePreset};
pub use i18n::Language;
pub use types::{SessionBackend, TerminalSession, AddHostDialog, AppView, KeychainDeleteRequest, load_available_shells, SftpContextMenu, SftpRenameDialog, SftpNewFolderDialog, SftpNewFileDialog, SftpConfirmDelete, SftpEditorDialog};
pub use pane::{SplitDirection, PaneNode, PaneAction, Tab, DetachedWindow};
pub use terminal_render::render_pane_tree;
