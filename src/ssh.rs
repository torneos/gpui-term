//! SSH terminal entity for GPUI.
//!
//! [`SshTerminal`] connects to a remote host via SSH, pipes output through
//! alacritty's ANSI state machine in [`TerminalCore`](crate::core::TerminalCore),
//! and exposes [`Content`] for rendering.

use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant};

use alacritty_terminal::term::TermMode;
use gpui::{Context, Window};
use russh::client;
use russh::ChannelMsg;

use crate::content::TerminalBounds;
use crate::core::TerminalCore;

const DEFAULT_COLS: usize = 80;
const DEFAULT_ROWS: usize = 24;

// =========================================================================
// Tokio runtime
// =========================================================================

fn tokio_rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime for SSH")
    })
}

// =========================================================================
// Control messages
// =========================================================================

enum CtrlMsg {
    WindowChange { cols: u32, rows: u32, pix_w: u32, pix_h: u32 },
}

// =========================================================================
// SshTerminal
// =========================================================================

/// A GPUI entity managing an SSH terminal session.
///
/// Wraps [`TerminalCore`](crate::core::TerminalCore) with SSH I/O via
/// [`russh`]: authentication, channel management, and async data transfer.
pub struct SshTerminal {
    core: TerminalCore,
    input_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    ctrl_tx: tokio::sync::mpsc::UnboundedSender<CtrlMsg>,
    _session: Option<client::Handle<SshClientHandler>>,
    pub state: ConnectionState,
    pub title_text: String,
    last_window_change: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Disconnected,
    Error,
}

impl SshTerminal {
    pub fn connect(
        user: &str, host: &str, port: u16,
        password: Option<&str>, identity_file: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let (input_tx, input_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let (ctrl_tx, ctrl_rx) = tokio::sync::mpsc::unbounded_channel::<CtrlMsg>();

        let initial_bounds = TerminalBounds::new(
            gpui::px(16.0), gpui::px(8.0),
            gpui::Bounds::new(
                gpui::point(gpui::px(0.), gpui::px(0.)),
                gpui::size(
                    gpui::px(DEFAULT_COLS as f32 * 8.0),
                    gpui::px(DEFAULT_ROWS as f32 * 16.0),
                ),
            ),
        );
        let core = TerminalCore::new(initial_bounds, rx);

        let user_owned = user.to_string();
        let host_owned = host.to_string();
        let password = password.map(|s| s.to_string());
        let identity_file = identity_file.map(|s| s.to_string());

        cx.spawn(move |this: gpui::WeakEntity<SshTerminal>, async_cx: &mut gpui::AsyncApp| {
            let async_cx = async_cx.clone();
            async move {
                let result = tokio_rt().block_on(async {
                    connect_ssh(
                        &user_owned, &host_owned, port,
                        password.as_deref(), identity_file.as_deref(),
                        tx, input_rx, ctrl_rx,
                    ).await
                });
                async_cx.update(|cx| match result {
                    Ok(handle) => {
                        let _ = this.update(cx, |this, _cx| {
                            this._session = Some(handle);
                            this.state = ConnectionState::Connected;
                        });
                    }
                    Err(e) => {
                        log::error!("SSH connection failed: {e}");
                        let _ = this.update(cx, |this, _cx| {
                            this.state = ConnectionState::Error;
                        });
                    }
                });
            }
        }).detach();

        Self {
            core, input_tx, ctrl_tx,
            _session: None, state: ConnectionState::Connecting,
            title_text: format!("{user}@{host}"),
            last_window_change: Instant::now(),
        }
    }

    // ── I/O ──

    pub fn input_bytes(&self, bytes: Vec<u8>) {
        let _ = self.input_tx.send(bytes);
    }

    // ── Sync ──

    /// Process pending SSH output and side-band events.
    pub fn sync(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.core.sync() {
            cx.notify();
        }
        // Send pty_write output (DA responses, DSR, color query replies)
        for text in self.core.drain_pty_writes() {
            self.input_bytes(text.as_bytes().to_vec());
        }
        // Handle clipboard events
        for event in self.core.take_pending_events() {
            match event {
                alacritty_terminal::event::Event::ClipboardStore(_, text) => {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                }
                alacritty_terminal::event::Event::ClipboardLoad(_, formatter) => {
                    if let Some(item) = cx.read_from_clipboard() {
                        if let Some(text) = item.text() {
                            let s = formatter(&text);
                            self.input_bytes(s.as_bytes().to_vec());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // ── Resize ──

    pub fn set_size(&mut self, bounds: TerminalBounds) {
        // Always resize the grid immediately.
        self.core.set_size(bounds);
        // Debounce WindowChange to avoid flooding the SSH connection.
        let now = Instant::now();
        if now.duration_since(self.last_window_change) < Duration::from_millis(500) {
            return;
        }
        self.last_window_change = now;
        let cols = bounds.num_columns().max(1) as u32;
        let rows = bounds.num_lines().max(1) as u32;
        let cell_w: f32 = bounds.cell_width.into();
        let line_h: f32 = bounds.line_height.into();
        let _ = self.ctrl_tx.send(CtrlMsg::WindowChange {
            cols, rows,
            pix_w: (cols as f32 * cell_w) as u32,
            pix_h: (rows as f32 * line_h) as u32,
        });
    }

    // ── Keyboard ──

    pub fn try_keystroke(&mut self, keystroke: &gpui::Keystroke) -> bool {
        if let Some(bytes) = self.core.try_keystroke(keystroke) {
            self.input_bytes(bytes);
            true
        } else {
            false
        }
    }

    // ── Mouse ──

    pub fn mouse_down(
        &mut self,
        event: &gpui::MouseDownEvent,
        bounds: gpui::Bounds<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let result = self.core.mouse_down(event, bounds);
        if let Some(bytes) = result.report_bytes {
            self.input_bytes(bytes);
        }
        if result.needs_paint {
            cx.notify();
        }
    }

    pub fn mouse_up(&mut self, event: &gpui::MouseUpEvent, cx: &mut Context<Self>) {
        let result = self.core.mouse_up(event);
        if let Some(bytes) = result.report_bytes {
            self.input_bytes(bytes);
        }
        if result.needs_paint {
            cx.notify();
        }
    }

    pub fn mouse_drag(
        &mut self,
        event: &gpui::MouseMoveEvent,
        bounds: gpui::Bounds<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let result = self.core.mouse_drag(event, bounds);
        if let Some(bytes) = result.report_bytes {
            self.input_bytes(bytes);
        }
        if result.needs_paint {
            cx.notify();
        }
    }

    pub fn mouse_move(
        &mut self,
        event: &gpui::MouseMoveEvent,
        bounds: gpui::Bounds<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let result = self.core.mouse_move(event, bounds);
        if let Some(bytes) = result.report_bytes {
            self.input_bytes(bytes);
        }
        if result.needs_paint {
            cx.notify();
        }
    }

    pub fn scroll_wheel(
        &mut self,
        event: &gpui::ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) {
        let result = self.core.scroll_wheel(event);
        if let Some(bytes) = result.report_bytes {
            self.input_bytes(bytes);
        }
        if result.needs_paint {
            cx.notify();
        }
    }

    // ── Clipboard ──

    pub fn copy(&self) -> Option<String> {
        self.core.copy()
    }

    pub fn paste(&mut self, text: &str) {
        self.input_bytes(self.core.paste(text));
    }

    // ── Focus ──

    pub fn focus_in(&mut self) {
        if let Some(bytes) = self.core.focus_in() {
            self.input_bytes(bytes);
        }
    }
    pub fn focus_out(&mut self) {
        if let Some(bytes) = self.core.focus_out() {
            self.input_bytes(bytes);
        }
    }

    // ── Simple accessors ──

    pub fn last_content(&self) -> &crate::content::Content { self.core.last_content() }
    pub fn total_lines(&self) -> usize { self.core.total_lines().max(DEFAULT_ROWS) }
    pub fn mode(&self) -> TermMode { self.core.mode() }
    pub fn selection_started(&self) -> bool { self.core.selection_started() }
    pub fn matches_count(&self) -> usize { self.core.matches_count() }
    pub fn matches_clone(&self) -> Vec<alacritty_terminal::selection::SelectionRange> {
        self.core.matches_clone()
    }
    pub fn title(&self) -> &str { &self.title_text }

    pub fn had_sync_output(&self) -> bool { self.core.had_output }

    pub fn scroll_line_up(&mut self) { self.core.scroll_line_up(); }
    pub fn scroll_line_down(&mut self) { self.core.scroll_line_down(); }
}

// =========================================================================
// SSH connection
// =========================================================================

struct SshClientHandler;

impl client::Handler for SshClientHandler {
    type Error = anyhow::Error;
    async fn check_server_key(&mut self, _key: &russh::keys::ssh_key::PublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

#[allow(clippy::too_many_arguments)]
async fn connect_ssh(
    user: &str, host: &str, port: u16,
    password: Option<&str>, identity_file: Option<&str>,
    tx: mpsc::Sender<Vec<u8>>,
    mut input_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    mut ctrl_rx: tokio::sync::mpsc::UnboundedReceiver<CtrlMsg>,
) -> anyhow::Result<client::Handle<SshClientHandler>> {
    let addr = format!("{host}:{port}");
    let config = Arc::new(client::Config::default());
    let mut session = client::connect(config, &addr, SshClientHandler).await?;

    let mut authenticated = false;

    // None auth
    match session.authenticate_none(user).await {
        Ok(r) if r.success() => authenticated = true,
        _ => {}
    }

    // SSH agent
    if !authenticated {
        if let Ok(mut agent) = russh::keys::agent::client::AgentClient::connect_env().await {
            if let Ok(identities) = agent.request_identities().await {
                for id in identities {
                    let key = match id {
                        russh::keys::agent::AgentIdentity::PublicKey { key, .. } => key,
                        _ => continue,
                    };
                    match session.authenticate_publickey_with(user, key, None, &mut agent).await {
                        Ok(r) if r.success() => { authenticated = true; break; }
                        _ => {}
                    }
                }
            }
        }
    }

    // Default SSH keys
    if !authenticated {
        let default_keys = ["id_ed25519", "id_ecdsa", "id_rsa"];
        let home = std::env::var("HOME").unwrap_or_default();
        for key_name in &default_keys {
            let key_path = format!("{home}/.ssh/{key_name}");
            if std::path::Path::new(&key_path).exists() {
                if let Ok(key) = russh::keys::load_secret_key(&key_path, None) {
                    let alg = key.algorithm();
                    let hash = if alg.is_rsa() {
                        match session.best_supported_rsa_hash().await {
                            Ok(h) => h.flatten(),
                            Err(_) => None,
                        }
                    } else { None };
                    let key = russh::keys::key::PrivateKeyWithHashAlg::new(Arc::new(key), hash);
                    match session.authenticate_publickey(user, key).await {
                        Ok(r) if r.success() => { authenticated = true; break; }
                        _ => {}
                    }
                }
            }
        }
    }

    // Explicit identity
    if !authenticated {
        if let Some(path) = identity_file {
            if let Ok(key) = russh::keys::load_secret_key(path, None) {
                let key = russh::keys::key::PrivateKeyWithHashAlg::new(Arc::new(key), None);
                match session.authenticate_publickey(user, key).await {
                    Ok(r) if r.success() => authenticated = true,
                    _ => {}
                }
            }
        }
    }

    // Password
    if !authenticated {
        if let Some(pw) = password {
            let result = session.authenticate_password(user, pw).await?;
            if !result.success() { anyhow::bail!("SSH authentication failed"); }
            authenticated = true;
        }
    }

    if !authenticated {
        anyhow::bail!("No authentication method succeeded for {user}@{host}:{port}");
    }

    let channel = session.channel_open_session().await?;
    channel.request_pty(false, "xterm-256color", DEFAULT_COLS as u32, DEFAULT_ROWS as u32, 0, 0, &[]).await?;
    channel.request_shell(false).await?;

    let handle = session;
    tokio::spawn(async move {
        let mut channel = channel;
        loop {
            tokio::select! {
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { ref data }) => {
                            if tx.send(data.to_vec()).is_err() { break; }
                        }
                        Some(ChannelMsg::Eof) | None => break,
                        _ => {}
                    }
                }
                input = input_rx.recv() => {
                    match input {
                        Some(bytes) => {
                            if channel.data(bytes.as_slice()).await.is_err() { break; }
                        }
                        None => break,
                    }
                }
                ctrl = ctrl_rx.recv() => {
                    match ctrl {
                        Some(CtrlMsg::WindowChange { cols, rows, pix_w, pix_h }) => {
                            let _ = channel.window_change(cols, rows, pix_w, pix_h).await;
                        }
                        None => break,
                    }
                }
            }
        }
    });

    Ok(handle)
}
