//! # Protected-host owner gate (app layer)
//!
//! Central choke point for "verify the local computer owner before touching
//! a protected host". Views never talk to `local_auth` directly — they route
//! actions through `ViewActions`, and the gate helpers here decide whether to
//! run the action now (already verified), or park it in the requesting
//! window's `owner_gate` and start verification (macOS system sheet, or the
//! app-drawn password dialog on Linux/Windows).
//!
//! Flow: gate helper → [`Self::owner_gate_or_request`] parks the action →
//! [`Self::poll_owner_gate`] (every frame, from `eframe::App::update` so the
//! hidden root window is covered too) consumes the result →
//! [`Self::dispatch_owner_action`] re-runs the action, re-checking identity
//! (hosts-vec indices can shift during the async wait).

use crate::config::{self, HostEntry};
use crate::ssh::AppNotification;
use crate::ui::state::owner_gate::{GatePrompt, PendingOwnerAction};
use crate::ui::types::session::{SessionKind, TerminalSession};

impl super::PortalApp {
    /// Gate an SSH connect (hosts view, history rows). Checks the target and
    /// — since connecting through it uses its credentials — the jump host.
    pub(crate) fn gate_connect_ssh(&mut self, window_idx: usize, host: &HostEntry) {
        let gated = host.protected || self.jump_chain_protected(host);
        if !gated || self.owner_gate_or_request(
            window_idx,
            PendingOwnerAction::ConnectSsh { host: host.clone() },
            "owner_verify_reason_connect",
        ) {
            self.add_tab_ssh(host);
        }
    }

    /// Gate opening the edit drawer (= viewing details + the modify entry).
    pub(crate) fn gate_open_host_edit(&mut self, window_idx: usize, index: usize) {
        let (protected, name) = match self.hosts.get(index) {
            Some(h) => (h.protected, h.name.clone()),
            None => return,
        };
        if !protected
            || self.owner_gate_or_request(
                window_idx,
                PendingOwnerAction::OpenHostEdit { index, name },
                "owner_verify_reason_edit",
            )
        {
            let host = self.hosts[index].clone();
            if let Some(w) = self.windows.get_mut(window_idx) {
                w.add_host_dialog.open_edit(index, &host);
            }
        }
    }

    /// Gate duplicating a host (duplicates stay protected, so this is a
    /// credential-bearing copy even though it doesn't connect).
    pub(crate) fn gate_duplicate_host(&mut self, window_idx: usize, index: usize) {
        let (protected, name) = match self.hosts.get(index) {
            Some(h) => (h.protected, h.name.clone()),
            None => return,
        };
        if !protected
            || self.owner_gate_or_request(
                window_idx,
                PendingOwnerAction::DuplicateHost { index, name },
                "owner_verify_reason_edit",
            )
        {
            let host = self.hosts[index].clone();
            let suffix = self.language.t("duplicate_suffix");
            if let Some(w) = self.windows.get_mut(window_idx) {
                w.add_host_dialog.open_duplicate(&host, suffix);
            }
        }
    }

    /// Gate deleting a host, then run the deletion body. Load-bearing even
    /// though the drawer open is gated: the drawer can stay open past the
    /// grace window.
    pub(crate) fn gate_delete_host(&mut self, window_idx: usize, index: usize) {
        let (protected, name, is_local) = match self.hosts.get(index) {
            Some(h) => (h.protected, h.name.clone(), h.is_local),
            None => return,
        };
        if is_local {
            return;
        }
        if !protected
            || self.owner_gate_or_request(
                window_idx,
                PendingOwnerAction::DeleteHost { index, name },
                "owner_verify_reason_delete",
            )
        {
            let host = self.hosts.remove(index);
            config::delete_host_credentials(&host);
            self.save_hosts();
        }
    }

    /// Gate an SFTP panel connect (`left` = left browser, else right).
    pub(crate) fn gate_connect_sftp(&mut self, window_idx: usize, left: bool, index: usize) {
        let host = match self.hosts.get(index) {
            Some(h) => h.clone(),
            None => return,
        };
        if !host.protected
            || self.owner_gate_or_request(
                window_idx,
                PendingOwnerAction::ConnectSftp { left, host: host.clone() },
                "owner_verify_reason_sftp",
            )
        {
            self.connect_sftp_browser(window_idx, left, &host);
        }
    }

    /// Gate a reconnect (tab bar, pane menu, remove-host-key-and-reconnect).
    pub(crate) fn gate_reconnect(
        &mut self,
        window_idx: usize,
        tab: usize,
        session: usize,
        remove_known_key: bool,
    ) {
        let needs_gate = self
            .windows
            .get(window_idx)
            .and_then(|w| w.tabs.get(tab))
            .and_then(|t| t.sessions.get(session))
            .map(|s| matches!(&s.kind, SessionKind::Ssh(_, h, _) if h.protected))
            .unwrap_or(false);
        if !needs_gate
            || self.owner_gate_or_request(
                window_idx,
                PendingOwnerAction::Reconnect { tab, session, remove_known_key },
                "owner_verify_reason_reconnect",
            )
        {
            self.reconnect_session(window_idx, tab, session, remove_known_key);
        }
    }

    /// True when already inside the grace window. Otherwise parks `action`,
    /// starts verification, and returns false — the caller must NOT act.
    pub(crate) fn owner_gate_or_request(
        &mut self,
        window_idx: usize,
        action: PendingOwnerAction,
        reason_key: &'static str,
    ) -> bool {
        if self.owner_verified.is_verified(std::time::Instant::now()) {
            return true;
        }
        let reason = self.language.t(reason_key).to_string();
        let gate = match self.windows.get_mut(window_idx) {
            Some(w) => &mut w.owner_gate,
            None => return true, // window vanished; don't strand the action
        };
        gate.pending = Some(action);
        gate.reason = reason;
        gate.password.clear();
        gate.error.clear();
        // macOS hands back a receiver for the system sheet; everywhere else
        // we collect a password in the app-drawn dialog instead. If the sheet
        // exists but failed to start (macOS), there is no typed-password
        // path either — drop the action and notify, rather than strand the
        // user in a dialog that can never verify.
        let sheet_failed = match crate::local_auth::begin_system_prompt(&gate.reason) {
            Some(result) => {
                gate.prompt = GatePrompt::InFlight { system_prompt: true, result };
                false
            }
            None if !crate::local_auth::has_system_prompt() => {
                gate.prompt = GatePrompt::Password;
                false
            }
            None => {
                gate.pending = None;
                gate.prompt = GatePrompt::None;
                true
            }
        };
        if sheet_failed {
            let msg = format!(
                "{}: {}",
                self.language.t("owner_verify_failed"),
                self.language.t("owner_verify_unavailable")
            );
            if let Some(w) = self.windows.get_mut(window_idx) {
                w.notifications.push(AppNotification::warning(msg));
            }
        }
        false
    }

    /// Per-frame poll of an in-flight verification (called from
    /// `eframe::App::update` for every window — the hidden root window never
    /// renders, so this cannot live in `render_window_content`).
    pub(crate) fn poll_owner_gate(&mut self, window_idx: usize) {
        let (outcome, system_prompt) = {
            let Some(gate) = self.windows.get(window_idx).map(|w| &w.owner_gate) else {
                return;
            };
            let GatePrompt::InFlight { system_prompt, result } = &gate.prompt else {
                return;
            };
            match result.try_recv() {
                Ok(res) => (Some(res), *system_prompt),
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                // Callback thread died without answering — surface as failure.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    (Some(Err("verification channel closed".to_string())), *system_prompt)
                }
            }
        };
        let Some(outcome) = outcome else { return };
        let pending = self.windows[window_idx].owner_gate.pending.take();
        self.windows[window_idx].owner_gate.prompt = GatePrompt::None;
        match outcome {
            Ok(true) => {
                self.owner_verified.mark_verified(std::time::Instant::now());
                // A late reply after the user cancelled our dialog still
                // earns the grace window (sudo semantics) — but with nothing
                // parked there is nothing to dispatch.
                if let Some(action) = pending {
                    self.dispatch_owner_action(window_idx, action);
                }
                let gate = &mut self.windows[window_idx].owner_gate;
                gate.password.clear();
                gate.error.clear();
            }
            failure => {
                if system_prompt {
                    // The OS sheet was dismissed (or refused to appear): drop
                    // the parked action and say so.
                    self.windows[window_idx].owner_gate.cancel();
                    let msg = match failure {
                        Err(e) => format!("{}: {e}", self.language.t("owner_verify_failed")),
                        _ => self.language.t("owner_verify_cancelled").to_string(),
                    };
                    if let Some(w) = self.windows.get_mut(window_idx) {
                        w.notifications
                            .push(AppNotification::warning(msg));
                    }
                } else {
                    // Wrong password / backend error: back to the field with
                    // the reason shown, so the user can retry.
                    let gate = &mut self.windows[window_idx].owner_gate;
                    gate.prompt = GatePrompt::Password;
                    gate.password.clear();
                    gate.error = match failure {
                        Err(e) => format!("{}: {e}", self.language.t("owner_verify_failed")),
                        _ => self.language.t("owner_verify_failed").to_string(),
                    };
                }
            }
        }
    }

    /// Re-run an action that survived verification. Identity is re-checked:
    /// the hosts vec may have shifted (delete in another window) while the
    /// system sheet was up.
    pub(crate) fn dispatch_owner_action(
        &mut self,
        window_idx: usize,
        action: PendingOwnerAction,
    ) {
        match action {
            PendingOwnerAction::ConnectSsh { host } => self.add_tab_ssh(&host),
            PendingOwnerAction::OpenHostEdit { index, name } => {
                if self.hosts.get(index).is_some_and(|h| h.name == name) {
                    let host = self.hosts[index].clone();
                    if let Some(w) = self.windows.get_mut(window_idx) {
                        w.add_host_dialog.open_edit(index, &host);
                    }
                }
            }
            PendingOwnerAction::DuplicateHost { index, name } => {
                if self.hosts.get(index).is_some_and(|h| h.name == name) {
                    let host = self.hosts[index].clone();
                    let suffix = self.language.t("duplicate_suffix");
                    if let Some(w) = self.windows.get_mut(window_idx) {
                        w.add_host_dialog.open_duplicate(&host, suffix);
                    }
                }
            }
            PendingOwnerAction::DeleteHost { index, name } => {
                if self.hosts.get(index).is_some_and(|h| h.name == name) && !self.hosts[index].is_local
                {
                    let host = self.hosts.remove(index);
                    config::delete_host_credentials(&host);
                    self.save_hosts();
                }
            }
            PendingOwnerAction::ConnectSftp { left, host } => {
                self.connect_sftp_browser(window_idx, left, &host);
            }
            PendingOwnerAction::Reconnect { tab, session, remove_known_key } => {
                self.reconnect_session(window_idx, tab, session, remove_known_key);
            }
        }
    }

    /// The SFTP panel connect body (mirrors `sftp_view`'s deferred connect).
    fn connect_sftp_browser(&mut self, window_idx: usize, left: bool, host: &HostEntry) {
        let username = TerminalSession::get_effective_username(&host.username);
        let auth = config::resolve_auth(host, &self.credentials);
        let proxy = config::resolve_proxy(host);
        let browser = crate::sftp::SftpBrowser::connect(
            &self.runtime,
            host.host.clone(),
            host.port,
            username,
            auth,
            host.name.clone(),
            proxy,
        );
        if let Some(w) = self.windows.get_mut(window_idx) {
            if left {
                w.sftp_browser_left = Some(browser);
            } else {
                w.sftp_browser = Some(browser);
            }
        }
    }

    /// The reconnect body shared by the three reconnect sites and the gate
    /// dispatch. `remove_known_key` drops the known_hosts entry first — the
    /// RemoveHostKey site must not do that before the gate has passed.
    fn reconnect_session(
        &mut self,
        window_idx: usize,
        tab: usize,
        session: usize,
        remove_known_key: bool,
    ) {
        // Extract-then-mutate: the borrow checker can't hold the window
        // borrow across the runtime reconnect call below.
        let (host_addr, port) = {
            let Some(w) = self.windows.get(window_idx) else { return };
            let Some(s) = w.tabs.get(tab).and_then(|t| t.sessions.get(session)) else {
                return;
            };
            match &s.kind {
                // Only SSH has a connection concept; Local must never reach
                // reconnect_ssh (SessionKind contract).
                SessionKind::Ssh(_, host, _) => (host.host.clone(), host.port),
                _ => return,
            }
        };
        if remove_known_key {
            let _ = crate::ssh::remove_known_hosts_key(&host_addr, port);
        }
        // An active ZMODEM transfer dies with the old connection — say so
        // instead of letting the chip vanish silently.
        #[cfg(unix)]
        {
            let zmodem_active = self
                .windows
                .get(window_idx)
                .and_then(|w| w.tabs.get(tab))
                .and_then(|t| t.sessions.get(session))
                .map(|s| s.kind.zmodem_transfer().is_some_and(|t| t.is_active()))
                .unwrap_or(false);
            if zmodem_active {
                if let Some(w) = self.windows.get_mut(window_idx) {
                    w.recording_error = Some((
                        self.language.t("zmodem_transfer_interrupted").to_string(),
                        std::time::Instant::now(),
                    ));
                }
            }
        }
        let runtime = &self.runtime;
        if let Some(s) = self
            .windows
            .get_mut(window_idx)
            .and_then(|w| w.tabs.get_mut(tab))
            .and_then(|t| t.sessions.get_mut(session))
        {
            s.reconnect_ssh(runtime, None);
        }
    }

    /// Is this host's jump leg itself protected? Connecting through a
    /// protected jump host uses its credentials, so it must be gated too.
    pub(crate) fn jump_chain_protected(&self, host: &HostEntry) -> bool {
        host.jump_host
            .as_ref()
            .and_then(|name| self.hosts.iter().find(|h| &h.name == name))
            .is_some_and(|j| j.protected)
    }

    /// The gate dialog: password field (Linux/Windows) or a passive
    /// "waiting for the system sheet" modal (macOS / background check).
    /// Rendered per window after the central panel, like the other dialogs.
    pub(crate) fn render_owner_gate_dialog(&mut self, ctx: &egui::Context, window_idx: usize) {
        use crate::ui::widgets;

        let is_password = matches!(
            self.windows.get(window_idx).map(|w| &w.owner_gate.prompt),
            Some(GatePrompt::Password)
        );
        let is_in_flight = matches!(
            self.windows.get(window_idx).map(|w| &w.owner_gate.prompt),
            Some(GatePrompt::InFlight { .. })
        );
        if !is_password && !is_in_flight {
            return;
        }

        let t_title = self.language.t("owner_verify_title");
        let t_submit = self.language.t("owner_verify_submit");
        let t_cancel = self.language.t("cancel");
        let t_waiting = self.language.t("owner_verify_waiting");
        let t_password = self.language.t("password");

        let mut submitted = false;
        let mut cancelled = false;
        {
            let theme = &self.theme;
            let gate = &mut self.windows[window_idx].owner_gate;
            egui::Window::new("owner_gate")
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .title_bar(false)
                .resizable(false)
                .collapsible(false)
                .frame(widgets::dialog_frame(theme))
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new(t_title)
                            .size(16.0)
                            .strong()
                            .color(theme.fg_primary),
                    );
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(&gate.reason).size(13.0).color(theme.fg_dim));
                    if !gate.error.is_empty() {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(&gate.error)
                                .size(12.0)
                                .color(theme.red),
                        );
                    }
                    ui.add_space(12.0);
                    match &gate.prompt {
                        GatePrompt::Password => {
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut gate.password)
                                    .password(true)
                                    .font(egui::FontId::monospace(13.0))
                                    .desired_width(220.0)
                                    .hint_text(t_password),
                            );
                            // Auto-focus the field on open; Enter submits.
                            if !resp.has_focus() && !ctx.wants_keyboard_input() {
                                resp.request_focus();
                            }
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                if ui.add(widgets::primary_button(t_submit, theme)).clicked() {
                                    submitted = true;
                                }
                                if ui.add(widgets::secondary_button(t_cancel, theme)).clicked() {
                                    cancelled = true;
                                }
                            });
                        }
                        GatePrompt::InFlight { .. } => {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(
                                    egui::RichText::new(t_waiting)
                                        .size(13.0)
                                        .color(theme.fg_dim),
                                );
                            });
                            ui.add_space(12.0);
                            if ui.add(widgets::secondary_button(t_cancel, theme)).clicked() {
                                cancelled = true;
                            }
                        }
                        GatePrompt::None => {}
                    }
                });
        }

        if cancelled {
            self.windows[window_idx].owner_gate.cancel();
        } else if submitted {
            // Password checks (PAM / LogonUserW) block for seconds — never on
            // the UI thread. Result lands in poll_owner_gate via repaint.
            let gate = &mut self.windows[window_idx].owner_gate;
            if gate.password.is_empty() {
                gate.error = self.language.t("owner_verify_failed").to_string();
                return;
            }
            let (tx, rx) = std::sync::mpsc::channel();
            let password = std::mem::take(&mut gate.password);
            gate.prompt = GatePrompt::InFlight { system_prompt: false, result: rx };
            match std::thread::Builder::new()
                .name("owner-verify".into())
                .spawn(move || {
                    let result = crate::local_auth::verify_owner_password(&password);
                    let _ = tx.send(result);
                    crate::repaint::notify_repaint();
                }) {
                Ok(_) => {}
                Err(e) => {
                    let gate = &mut self.windows[window_idx].owner_gate;
                    gate.prompt = GatePrompt::Password;
                    gate.error = format!("{}: {e}", self.language.t("owner_verify_failed"));
                }
            }
        }
    }
}
