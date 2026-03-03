# Portal

A modern GUI terminal emulator built with Rust and egui, inspired by Termius.

![Portal](https://img.shields.io/badge/Portal-0.9.0-blue)
![Rust](https://img.shields.io/badge/Rust-1.70+-orange)

## Features

### Terminal
- **Native Terminal Input** - Direct keyboard input with IME (CJK) support
- **Multiple Tabs** - Create and manage multiple terminal sessions
- **Split Panes** - Horizontal/vertical splits (Cmd+D / Cmd+Shift+D)
- **ANSI Support** - Full 256 color + Truecolor, SGR attributes, alternate screen
- **Scrollback** - Mouse wheel scrollback history
- **CJK Characters** - Unicode double-width rendering with font fallback
- **Text Selection** - Mouse drag select, Cmd+C/V, right-click context menu

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
- **File Management** - Right-click context menu with Rename, Delete, New Folder
- **Breadcrumb Navigation** - Clickable path segments
- **File Permissions** - Unix permission display (rwxrwxrwx)
- **Status Bar** - File/folder count and total size per directory
- **Transfer Progress** - Real-time speed, progress bar, cancel support
- **Refresh Animation** - Spinner feedback on refresh button click

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
├── main.rs              # GUI application (egui)
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

打包脚本会自动完成构建、签名、打 DMG 三个步骤。根据是否有 Apple Developer 账号，分三种方式：

#### 1. 本机使用（无需 Apple Developer 账号）

```bash
# 安装 cargo-bundle（仅首次）
cargo install cargo-bundle

# 打包（ad-hoc 签名）
./scripts/build-dmg.sh
```

产出 `target/release/Portal-0.9.1-{arch}.dmg`。

> ad-hoc 签名的应用在其他电脑上会被 Gatekeeper 拦截，用户需要 **右键 > 打开** 绕过。

#### 2. 分发给他人（需要 Apple Developer 账号）

先在 Keychain Access 中确认你有 `Developer ID Application: ...` 证书，然后：

```bash
SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)" \
./scripts/build-dmg.sh
```

签名后其他电脑可以直接打开，不会提示"已损坏"。但仍会显示"无法检查是否包含恶意软件"的警告。

#### 3. 正式分发（签名 + 公证，无任何警告）

需要额外准备：
1. 登录 [appleid.apple.com](https://appleid.apple.com/account/manage) 生成 **App 专用密码**
2. 在 [developer.apple.com](https://developer.apple.com) 查看你的 **Team ID**（10 位字符）

```bash
SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)" \
APPLE_ID="you@example.com" \
APPLE_TEAM_ID="XXXXXXXXXX" \
APPLE_PASSWORD="xxxx-xxxx-xxxx-xxxx" \
./scripts/build-dmg.sh
```

脚本会自动完成签名 → 提交公证 → 等待审核 → Staple 票据，完成后用户双击即可安装，无任何安全警告。

## Configuration

Host connections are stored in `~/Library/Application Support/portal/hosts.json`. Passwords, key passphrases, and private key contents are stored securely in the macOS Keychain. Each host's credentials appear under `Portal: <host name>` in Keychain Access. The JSON file never contains plaintext secrets.

## License

MIT License
