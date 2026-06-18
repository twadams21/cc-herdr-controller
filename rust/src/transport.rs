//! Where intents go. Either a persistent SSH child to the Mac relay, or stdout
//! (for --dry-run / --monitor, which never contact the Mac).

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

pub struct SshConfig {
    pub host: String,
    pub extra_args: Vec<String>,
    pub relay_cmd: String,
}

/// One long-lived `ssh <host> <relay_cmd>` child. Intents are written, newline
/// terminated, to its stdin; the relay runs herdr locally on the Mac. The
/// connection stays open so each intent is a single network hop, and we
/// transparently respawn the child if the pipe breaks.
pub struct SshTransport {
    cfg: SshConfig,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
}

impl SshTransport {
    pub fn new(cfg: SshConfig) -> Result<Self, String> {
        let mut t = SshTransport { cfg, child: None, stdin: None };
        t.connect()?;
        Ok(t)
    }

    fn connect(&mut self) -> Result<(), String> {
        let mut child = Command::new("ssh")
            .args(&self.cfg.extra_args)
            .arg(&self.cfg.host)
            .arg(&self.cfg.relay_cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("failed to spawn ssh: {e}"))?;
        self.stdin = child.stdin.take();
        self.child = Some(child);
        eprintln!("[transport] connected: ssh {} {}", self.cfg.host, self.cfg.relay_cmd);
        Ok(())
    }

    fn reconnect(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.stdin = None;
        eprintln!("[transport] reconnecting to {} ...", self.cfg.host);
        if let Err(e) = self.connect() {
            eprintln!("[transport] reconnect failed: {e}");
        }
    }
}

impl Transport for SshTransport {
    fn send(&mut self, intent: &str) {
        for _ in 0..2 {
            if self.stdin.is_none() {
                self.reconnect();
            }
            if let Some(stdin) = self.stdin.as_mut() {
                let line = format!("{intent}\n");
                if stdin.write_all(line.as_bytes()).and_then(|_| stdin.flush()).is_ok() {
                    return;
                }
                eprintln!("[transport] write failed; resetting connection");
                self.reconnect();
            }
        }
        eprintln!("[transport] dropped intent (no connection): {intent}");
    }
}
