# Changelog

All notable changes to Portal will be documented in this file.

## [0.11.0] - 2026-03-24

### Added
- **Light Color Themes** - Added Solarized Light, GitHub Light, and One Light themes
- **SSH Tunnels** - Local (-L) and remote (-R) port forwarding with start/stop controls
- **Command Snippets** - Save and run frequently used commands (Cmd+Shift+S)
- **Tunnels View** - Dedicated view for managing SSH tunnel configurations
- **Snippets Drawer** - Quick snippet selector from terminal view
- **Tab Bar Menu** - "..." menu with Snippets and Broadcast toggle options
- **More Language Support** - Added French, Spanish, and Russian translations

### Fixed
- **Terminal Layout** - Tab bar now at top, status bar at bottom, terminal in middle
- **Split Divider Clipping** - Divider lines no longer appear in snippets drawer
- **Tunnel Form Inputs** - Input fields now use proper width instead of infinite
- **Dropdown Colors** - All dropdown menus now use theme-configurable background colors
- **Theme Button Contrast** - Light theme buttons now use medium-gray backgrounds for better text visibility

### Improved
- **Snippets Drawer Scope** - Snippets drawer now only appears in Terminal view
- **i18n Coverage** - All tunnel form labels and placeholders now properly translated
- **Theme System** - Added menu_bg field for consistent dropdown styling across all themes

## [0.10.5] - 2026-03-21

### Fixed
- **Detached Window Layout** - Fixed + button position in detached window tab bar (now inside scroll area like main window)
- **Tab Drag to Detach** - Ensured broadcast and new tab features work consistently in detached windows

### Improved
- **Code Organization** - Refactored main.rs into app module (app/mod.rs, app/tab_management.rs) for better maintainability

## [0.10.0] - 2026-03-03

### Added
- **Built-in File Editor** - Double-click files to open in embedded editor with save functionality
- **Hidden Files Toggle** - Show/hide hidden files with Cmd+Shift+H
- **File Details Display** - Show file size and modification time in SFTP browser
- **Column Alignment** - Aligned display for permissions, size, and date columns
- **Error Display** - SFTP operation errors shown in status bar with red color
- **Delete Confirmation** - Interactive dialog with progress tracking for deletions
- **Parent Directory Button** - ".." button in breadcrumb navigation
- **Terminal Click to Focus** - Click on terminal pane to focus for typing

### Fixed
- **CJK Text Selection** - Fixed double-click and triple-click selection for Chinese/Japanese/Korean characters
- **Double-click File Operations** - Fixed index mismatch when hidden files are filtered
- **Delete Confirmation with Hidden Files** - Delete dialog now correctly shows only visible file names
- **Context Menu with Hidden Files** - Context menu operations now work correctly with filtered entries
- **Terminal Mouse Events** - Improved click, double-click, triple-click, and drag detection

### Improved
- **Performance Optimizations** - Reduced memory allocations and CPU usage in SFTP view rendering
- **Mouse Drag Detection** - Better handling of drag vs click interactions in terminal
- **File Selection UI** - More intuitive keyboard navigation and selection management

## [0.9.0] - Previous Release

### Features
- Terminal emulation with ANSI color support
- SSH connections with password and key authentication
- SFTP file browser with dual-panel layout
- Drag and drop file transfers
- Split panes (horizontal/vertical)
- Multiple tabs
- System keychain integration
- Multi-language support (EN/ZH/JA/KO)
