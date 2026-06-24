//! Where intents go.
//!
//! - [`LocalTransport`] dispatches in-process (local mode — no child, no pipe).
//! - [`ChildTransport`] (via [`ssh_transport`]) streams newline-delimited
//!   intents over one persistent SSH connection to `cc-controller host` on the
//!   multiplexer machine.
//! - [`StdoutTransport`] is the no-op used by `--dry-run`.

use crate::dispatch::{self, Backend};
use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};

pub trait Transport {
    fn send(&mut self, intent: &str);
}

/// Prints intents instead of sending them. Used when not performing for real.
pub struct StdoutTransport;

impl Transport for StdoutTransport {
    fn send(&mut self, intent: &str) {
        println!("    -> intent: {intent}");
    }
}

/// Dispatches intents in-process against a local herdr/tmux — `local` mode.
/// No subprocess, no pipe: the run loop's intent goes straight to `dispatch`.
pub struct LocalTransport {
    backend: Backend,
}

impl LocalTransport {
    pub fn new(backend: Backend) -> Self {
        LocalTransport { backend }
    }
}

impl Transport for LocalTransport {
    fn send(&mut self, intent: &str) {
        if let Err(e) = dispatch::handle_intent(intent, self.backend) {
            eprintln!("[local] {intent:?}: {e}");
        }
    }
}

/// A long-lived child process fed newline-delimited intents on its stdin. The
/// command is rebuilt by `spawn` on each (re)connect, so a broken pipe is
/// respawned transparently. Backs both the SSH (remote herdr) and local
/// (herdr on this machine) transports — they differ only in how the child is
/// launched.
pub struct ChildTransport {
    label: String,
    spawn: Box<dyn Fn() -> Command>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
}

impl ChildTransport {
    fn new(label: String, spawn: impl Fn() -> Command + 'static) -> Result<Self, String> {
        let mut t = ChildTransport {
            label,
            spawn: Box::new(spawn),
            child: None,
            stdin: None,
        };
        t.connect()?;
        Ok(t)
    }

    fn connect(&mut self) -> Result<(), String> {
        let mut cmd = (self.spawn)();
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to start relay ({}): {e}", self.label))?;
        self.stdin = child.stdin.take();
        self.child = Some(child);
        eprintln!("[transport] connected: {}", self.label);
        Ok(())
    }

    fn reconnect(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.stdin = None;
        eprintln!("[transport] reconnecting: {} ...", self.label);
        if let Err(e) = self.connect() {
            eprintln!("[transport] reconnect failed: {e}");
        }
    }
}

impl Drop for ChildTransport {
    fn drop(&mut self) {
        // Kill the SSH child on a clean exit so it doesn't linger reparented.
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Transport for ChildTransport {
    fn send(&mut self, intent: &str) {
        for _ in 0..2 {
            if self.stdin.is_none() {
                self.reconnect();
            }
            if let Some(stdin) = self.stdin.as_mut() {
                let line = format!("{intent}\n");
                if stdin
                    .write_all(line.as_bytes())
                    .and_then(|_| stdin.flush())
                    .is_ok()
                {
                    return;
                }
                eprintln!("[transport] write failed; resetting connection");
                self.reconnect();
            }
        }
        eprintln!("[transport] dropped intent (no connection): {intent}");
    }
}

/// `ssh <host> <remote_cmd>` — the multiplexer on another machine. One
/// persistent connection; `remote_cmd` runs `cc-controller host` next to
/// herdr/tmux, executing intents against the *local* socket, so only the
/// compact intent crosses the network.
pub fn ssh_transport(
    host: String,
    extra_args: Vec<String>,
    remote_cmd: String,
) -> Result<ChildTransport, String> {
    let label = format!("ssh {host} {remote_cmd}");
    ChildTransport::new(label, move || {
        let mut c = Command::new("ssh");
        c.args(&extra_args).arg(&host).arg(&remote_cmd);
        c
    })
}
