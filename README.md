# Portal

A modern GUI terminal emulator built with Rust and egui, inspired by Termius.

![Portal](https://img.shields.io/badge/Portal-0.16.2-blue)
![Rust](https://img.shields.io/badge/Rust-1.70+-orange)

## Features

### AI Assistant
- **In-terminal chat** - A draggable orb opens a chat drawer beside the terminal, backed by an LLM (OpenAI-compatible or Anthropic), with streaming replies.
- **Markdown rendering** - Headings, bold/italic, inline code, links, lists, block quotes, and fenced shell commands rendered as runnable cards.
- **Environment-aware prompts** - The system prompt reflects the target session (local OS/shell/cwd, or the remote host's probed OS/arch/shell for SSH), so suggested commands match where they run.
- **Command safety** - AI-suggested commands never run on their own: every command requires two explicit approvals (Run button → confirm dialog) before reaching the terminal.

### Terminal
- **Native Terminal Input** - Direct keyboard input with IME (CJK) support
- **Multiple Tabs** - Create and manage multiple terminal sessions
- **Split Panes** - Horizontal/vertical splits (Cmd+D / Cmd+Shift+D)
- **ANSI Support** - Full 256 color + Truecolor, SGR attributes, alternate screen
- **Scrollback** - Mouse wheel scrollback history
- **CJK Characters** - Unicode double-width rendering with font fallback
- **Text Selection** - Mouse drag select, double-click word, triple-click line selection
- **Clickable URLs** - Auto-detect URLs in terminal output, hover to highlight, click to open in browser
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

### SSH Tunnels (Port Forwarding)
- **Local Forwarding** - Map local port to remote host ( `-L` )
- **Remote Forwarding** - Map remote port to local host ( `-R` )
- **Tunnel Management** - Start/stop tunnels from host detail view
- **Status Indicators** - Active, Starting, Stopped, Error states

### Snippets
- **Command Snippets** - Save and reuse frequently used commands
- **Group Organization** - Organize snippets by group
- **Quick Run** - Run snippets in active session with Cmd+Shift+S
- **Multi-language Support** - English, Chinese, Japanese, Korean, French, Spanish, Russian

### UI / Theme
- **Dark Themes** - Tokyo Night, Dracula, One Dark, Solarized Dark, Nord
- **Light Themes** - Solarized Light, GitHub Light, One Light
- **Navigation Bar** - Hosts / Terminal / SFTP / Tunnels / Snippets / Keychain / Settings
- **Tab Status Indicators** - Green=connected, Blue=connecting, Red=error
- **Bottom Status Bar** - Connection type, shell, encoding
- **Multi-language** - English, Chinese, Japanese, Korean, French, Spanish, Russian

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
│   ├── views/           # View pages
│   │   ├── hosts_view.rs    # Host list page
│   │   ├── tunnel_view.rs   # SSH tunnels page
│   │   ├── snippets_view.rs # Snippets page
│   │   ├── keychain_view.rs # Keychain management page
│   │   ├── sftp_view.rs     # SFTP file browser UI
│   │   ├── settings_view.rs # Settings page
│   │   ├── tab_view.rs      # Tab bar component
│   │   ├── metrics_drawer.rs # Tools drawer (metrics/snippets/tunnels)
│   │   ├── ai_drawer.rs     # AI chat drawer + draggable orb
│   │   └── ai_markdown.rs   # Lightweight Markdown renderer for AI replies
│   ├── state/           # UI state
│   │   └── ai_chat.rs   # AI chat state, streaming buffer, system prompt
│   ├── types/           # Shared UI types (AppView, sessions, dialogs)
│   ├── theme.rs         # Color themes (dark and light)
│   ├── i18n/            # Internationalization (EN/ZH/JA/KO/FR/ES/RU)
│   ├── pane.rs          # Split pane layout
│   ├── terminal/        # Terminal grid rendering
│   ├── widgets.rs       # Reusable UI components
│   └── tokens.rs        # Design tokens (spacing, sizing)
├── terminal/
│   ├── session.rs       # TerminalGrid, TerminalCell, PTY management
│   └── mod.rs
├── ssh/
│   ├── session.rs       # SSH session (russh), async I/O
│   └── mod.rs
├── sftp/
│   ├── session.rs       # SFTP browser, local browser, async transfers
│   └── mod.rs
├── llm/                 # Chat backends (OpenAI-compatible + Anthropic), SSE
│   ├── stream.rs        # Streaming request dispatch and SSE parsing
│   ├── openai.rs        # OpenAI-compatible chat-completion body building
│   ├── anthropic.rs     # Anthropic Messages body building
│   └── types.rs         # LlmConfig, LlmBackend, ChatMessage
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
- **ureq** - HTTP client for the AI assistant (rustls TLS) and update checks
- **serde / serde_json** - Configuration persistence and LLM JSON bodies
- **cocoa** - macOS specific APIs (app activation, NSPanel)

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

### AI Assistant

Configure the AI assistant in **Settings → LLM**:

- **Provider** - OpenAI-compatible (e.g. OpenAI, local Ollama, any `/v1` server) or Anthropic.
- **Base URL** - Required for OpenAI-compatible endpoints (default `https://api.openai.com/v1`); blank falls back to `https://api.anthropic.com` for Anthropic.
- **Model** - e.g. `gpt-4o`, `claude-sonnet`, or your local model.
- **API key** - Stored for the configured provider.

Once configured, drag the assistant orb in any terminal tab to open the chat drawer. The system prompt is built from the target session's environment (local OS/shell/cwd, or the probed remote environment for SSH sessions), so suggested commands match the host they run on.

### Hosts & Credentials

Host connections are stored in `~/Library/Application Support/portal/hosts.json`. Passwords, key passphrases, and private key contents are stored securely in the macOS Keychain. Each host's credentials appear under `Portal: <host name>` in Keychain Access. The JSON file never contains plaintext secrets.

## Security Features

- **SSH Host Key Verification** - Automatic detection of man-in-the-middle attacks (host key changes)
- **known_hosts Support** - Auto-learns new hosts, detects key changes
- **System Keychain** - Credentials stored securely in macOS Keychain
- **Key Authentication** - Support for RSA/ED25519 and other key types

## License

MIT License
