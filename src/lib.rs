//! # Portal - Modern Terminal Emulator
//!
//! Portal is a cross-platform terminal emulator built with Rust and egui, inspired by Termius.
//! It provides SSH, SFTP, and local terminal emulation with a modern GUI.
//!
//! ## Features
//!
//! - **Multiple Tabs & Panes**: Split panes horizontally/vertically with drag-to-resize
//! - **SSH Connections**: Password and SSH key authentication with secure credential storage
//! - **SFTP File Browser**: Dual-panel local/remote file browser with drag-and-drop transfers
//! - **System Keychain Integration**: Secure password and key storage (macOS Keychain)
//! - **Multi-language Support**: English, Chinese, Japanese, Korean
//! - **Broadcast Mode**: Type into multiple terminals simultaneously
//! - **Detached Windows**: Open terminals in separate windows
//!
//! ## Architecture
//!
//! The application is structured into several modules:
//!
//! - `terminal`: Terminal emulation, PTY handling, VTE parsing
//! - `ssh`: SSH client with key-based and password authentication
//! - `sftp`: SFTP file browser and transfer
//! - `config`: Configuration management and keychain integration
//! - `ui`: egui-based user interface
//!
//! ## Dependencies
//!
//! - **egui / eframe**: Immediate mode GUI framework
//! - **tokio**: Async runtime
//! - **russh / russh-sftp**: SSH and SFTP protocols
//! - **vte**: Terminal escape sequence parsing
//! - **pty**: Unix PTY support
//! - **keyring**: System keychain credential storage

pub mod terminal;
pub mod config;
pub mod ssh;
