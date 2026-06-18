//! Where intents go. A long-lived child process consumes newline-delimited
//! intents on its stdin and runs `relay.py` next to herdr — either over SSH to
//! another machine (remote herdr) or as a local subprocess (herdr on *this*
//! machine). `StdoutTransport` is the no-op used by --dry-run / --monitor.

use std::io::Write;
use std::path::PathBuf;
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

/// `ssh <host> <relay_cmd>` — herdr on another machine. One persistent
/// connection; the relay runs herdr against its *local* socket so only the
/// compact intent crosses the network.
pub fn ssh_transport(
    host: String,
    extra_args: Vec<String>,
    relay_cmd: String,
) -> Result<ChildTransport, String> {
    let label = format!("ssh {host} {relay_cmd}");
    ChildTransport::new(label, move || {
        let mut c = Command::new("ssh");
        c.args(&extra_args).arg(&host).arg(&relay_cmd);
        c
    })
}

/// Run the relay as a local subprocess — herdr on *this* machine, no SSH. The
/// relay is launched through the shell from `dir` (the repo root, so it can
/// find relay.py / herdr.py) and inherits our environment, including whatever
/// the running herdr session exposes for its socket.
pub fn local_transport(relay_cmd: String, dir: Option<PathBuf>) -> Result<ChildTransport, String> {
    let label = format!("local: {relay_cmd}");
    ChildTransport::new(label, move || {
        let mut c = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&relay_cmd);
            c
        } else {
            let mut c = Command::new("sh");
            c.arg("-c").arg(&relay_cmd);
            c
        };
        if let Some(d) = &dir {
            c.current_dir(d);
        }
        c
    })
}
