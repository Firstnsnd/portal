# Portal

A modern GUI terminal emulator built with Rust and egui, inspired by Termius.

![Portal](https://img.shields.io/badge/Portal-0.10.0-blue)
![Rust](https://img.shields.io/badge/Rust-1.70+-orange)

## Features

### Terminal
- **Native Terminal Input** - Direct keyboard input with IME (CJK) support
- **Multiple Tabs** - Create and manage multiple terminal sessions
- **Split Panes** - Horizontal/vertical splits (Cmd+D / Cmd+Shift+D)
- **ANSI Support** - Full 256 color + Truecolor, SGR attributes, alternate screen
- **Scrollback** - Mouse wheel scrollback history
- **CJK Characters** - Unicode double-width rendering with font fallback
- **Text Selection** - Mouse drag select, double-click word, triple-click line selection
- **Click to Focus** - Click terminal pane to focus and type

### SSH
- **SSH Connections** - Password and SSH key authentication (russh)
- **Connection State UI** - Connecting/Authenticating/Connected/Error overlays
- **Connection Timeout** - 15-second timeout with manual cancel
- **Auto-reconnect** - Click tab to reconnect after disconnect
- **Test Connection** - Verify connectivity before saving
- **Secure Credentials** - Passwords and private keys stored in system keychain (macOS Keychain)
- **Keychain Management** - View, inspect and delete stored credentials from the Keychain page

### SFTP File Browser
- **Dual-panel Layout** - Local and remote file browsers side by side
- **Drag & Drop** - Transfer files by dragging between panels
- **Built-in Editor** - Double-click files to open in embedded editor
- **File Management** - Right-click context menu with Rename, Delete, New Folder
- **Breadcrumb Navigation** - Clickable path segments, parent directory button
- **Hidden Files** - Toggle visibility with Cmd+Shift+H
- **File Permissions** - Unix permission display (rwxrwxrwx)
- **File Details** - Size, modification time display with aligned columns
- **Error Display** - Operation errors shown in status bar
- **Delete Confirmation** - Interactive dialog with progress feedback

### Host Management
- **Host List** - Grouped SSH hosts with search
- **Edit Drawer** - Side panel for host configuration
- **Click to Edit** - Click host row to open editor, "Connect" button for SSH
- **Delete from Drawer** - Trash icon in edit drawer header

### UI / Theme
- **Tokyo Night** dark color scheme
- **Navigation Bar** - Hosts / Terminal / SFTP / Keychain / Settings view switching
- **Tab Status Indicators** - Green=connected, Blue=connecting, Red=error
- **Bottom Status Bar** - Connection type, shell, encoding
- **Multi-language** - English, Chinese, Japanese, Korean

## Quick Start

```bash
# Build and run
cargo build --release
./target/release/portal

# Or build macOS .dmg installer
./scripts/build-dmg.sh
```

## Architecture

```
src/
├── main.rs              # Entry point, eframe::App update logic
├── app/
│   ├── mod.rs           # PortalApp struct and initialization
│   └── tab_management.rs # Tab operations (add, split, close, detach)
├── ui/
│   ├── hosts_view.rs    # Host list page, nav panel
│   ├── settings_view.rs # Settings page
│   ├── keychain_view.rs # Keychain management page
│   ├── sftp_view.rs     # SFTP file browser UI
│   ├── types.rs         # Shared UI types (AppView, sessions, dialogs)
│   ├── theme.rs         # Color themes
│   ├── i18n.rs          # Internationalization (EN/ZH/JA/KO)
│   ├── pane.rs          # Split pane layout
│   ├── terminal_render.rs # Terminal pane rendering
│   └── input.rs         # Input handling
├── terminal/
│   ├── session.rs       # TerminalGrid, TerminalCell, PTY management
│   └── mod.rs
├── ssh/
│   ├── session.rs       # SSH session (russh), async I/O
│   └── mod.rs
├── sftp/
│   ├── session.rs       # SFTP browser, local browser, async transfers
│   └── mod.rs
└── config/
    └── mod.rs           # Host configuration, JSON persistence, keychain integration
```

## Dependencies

- **egui / eframe** - Immediate mode GUI framework
- **tokio** - Async runtime
- **vte** - Terminal escape sequence parsing
- **russh / russh-sftp** - SSH and SFTP protocol
- **pty** - Unix PTY support
- **unicode-width** - CJK double-width detection
- **arboard** - Clipboard access
- **keyring** - System keychain credential storage

## Building

```bash
cargo build --release
```

### macOS .dmg Installer

The build script automatically completes three steps: build, sign, and package DMG. There are three methods depending on whether you have an Apple Developer account:

#### 1. Personal Use (No Apple Developer Account Required)

```bash
# Install cargo-bundle (first time only)
cargo install cargo-bundle

# Build package (ad-hoc signing)
./scripts/build-dmg.sh
```

Produces `target/release/Portal-0.9.1-{arch}.dmg`.

> Ad-hoc signed applications will be blocked by Gatekeeper on other computers. Users need to **right-click > Open** to bypass.

#### 2. Distribution to Others (Apple Developer Account Required)

First, confirm you have a `Developer ID Application: ...` certificate in Keychain Access, then:

```bash
SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)" \
./scripts/build-dmg.sh
```

After signing, the app can be opened directly on other computers without the "damaged" error. However, it will still show a warning about being unable to check for malicious software.

#### 3. Official Distribution (Signing + Notarization, No Warnings)

Additional requirements:
1. Log in to [appleid.apple.com](https://appleid.apple.com/account/manage) to generate an **App-specific password**
2. Check your **Team ID** (10 characters) at [developer.apple.com](https://developer.apple.com)

```bash
SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)" \
APPLE_ID="you@example.com" \
APPLE_TEAM_ID="XXXXXXXXXX" \
APPLE_PASSWORD="xxxx-xxxx-xxxx-xxxx" \
./scripts/build-dmg.sh
```

The script will automatically complete signing → submit for notarization → wait for approval → staple ticket. Users can then install by double-clicking without any security warnings.

## Configuration

Host connections are stored in `~/Library/Application Support/portal/hosts.json`. Passwords, key passphrases, and private key contents are stored securely in the macOS Keychain. Each host's credentials appear under `Portal: <host name>` in Keychain Access. The JSON file never contains plaintext secrets.

## Security Features

- **SSH Host Key Verification** - Automatic detection of man-in-the-middle attacks (host key changes)
- **known_hosts Support** - Auto-learns new hosts, detects key changes
- **System Keychain** - Credentials stored securely in macOS Keychain
- **Key Authentication** - Support for RSA/ED25519 and other key types

## License

MIT License
