# Changelog

All notable changes to Portal will be documented in this file.

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
