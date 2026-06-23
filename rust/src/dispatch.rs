//! The multiplexer dispatch layer — the all-Rust replacement for the old
//! `herdr.py` + `tmux.py` + `backend.py` + `relay.py::_dispatch`.
//!
//! A [`Backend`] turns the abstract actions (tab / workspace / pane / keys / …)
//! into herdr or tmux CLI calls, talking to the multiplexer over its local
//! socket. [`handle_intent`] parses one wire intent and runs it. Used both
//! in-process by `local` mode and line-by-line by the `host` subcommand.

use base64::Engine;
use serde_json::Value;
use std::fmt;

#[derive(Debug)]
pub struct DispatchError(pub String);

impl fmt::Display for DispatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DispatchError {}

/// SGR mouse-wheel escape sequences. A mouse-tracking program (Claude Code,
/// `less --mouse`, htop, …) reads these as wheel notches. Sent over the socket,
/// so it scrolls the focused *program* (not the multiplexer's own scrollback).
const WHEEL_UP: &str = "\x1b[<64;1;1M";
const WHEEL_DOWN: &str = "\x1b[<65;1;1M";

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Backend {
    Herdr,
    Tmux,
}

impl Backend {
    pub fn name(self) -> &'static str {
        match self {
            Backend::Herdr => "herdr",
            Backend::Tmux => "tmux",
        }
    }

    /// Pick a backend from an override (CLI flag), else mapping.json's `backend`,
    /// else the default (herdr).
    pub fn resolve(override_name: Option<&str>, cfg: &Value) -> Result<Backend, String> {
        let name = override_name
            .map(str::to_string)
            .or_else(|| cfg["backend"].as_str().map(str::to_string))
            .unwrap_or_else(|| "herdr".to_string());
        match name.as_str() {
            "herdr" => Ok(Backend::Herdr),
            "tmux" => Ok(Backend::Tmux),
            other => Err(format!("unknown backend {other:?}; choose herdr or tmux")),
        }
    }

    pub fn tab_step(self, step: i64) -> Result<(), DispatchError> {
        match self {
            Backend::Herdr => herdr::tab_step(step),
            Backend::Tmux => tmux::tab_step(step),
        }
    }

    pub fn workspace_step(self, step: i64) -> Result<(), DispatchError> {
        match self {
            Backend::Herdr => herdr::workspace_step(step),
            Backend::Tmux => tmux::workspace_step(step),
        }
    }

    pub fn pane_focus(self, direction: &str) -> Result<(), DispatchError> {
        match self {
            Backend::Herdr => herdr::pane_focus(direction),
            Backend::Tmux => tmux::pane_focus(direction),
        }
    }

    pub fn zoom(self) -> Result<(), DispatchError> {
        match self {
            Backend::Herdr => herdr::zoom(),
            Backend::Tmux => tmux::zoom(),
        }
    }

    pub fn send_keys(self, keys: &[&str]) -> Result<(), DispatchError> {
        match self {
            Backend::Herdr => herdr::send_keys(keys),
            Backend::Tmux => tmux::send_keys(keys),
        }
    }

    pub fn send_text(self, text: &str) -> Result<(), DispatchError> {
        match self {
            Backend::Herdr => herdr::send_text(text),
            Backend::Tmux => tmux::send_text(text),
        }
    }

    pub fn scroll(self, lines: i64) -> Result<(), DispatchError> {
        match self {
            Backend::Herdr => herdr::scroll(lines),
            Backend::Tmux => tmux::scroll(lines),
        }
    }
}

/// Parse and run one wire intent (the protocol the run loop emits). Mirrors the
/// old `relay.py::_dispatch`. Unknown intents are logged, not fatal.
pub fn handle_intent(line: &str, backend: Backend) -> Result<(), DispatchError> {
    let mut parts = line.split_whitespace();
    let Some(cmd) = parts.next() else {
        return Ok(());
    };
    let args: Vec<&str> = parts.collect();

    match cmd {
        "ping" => Ok(()),
        "tab_next" => backend.tab_step(1),
        "tab_prev" => backend.tab_step(-1),
        "workspace_next" => backend.workspace_step(1),
        "workspace_prev" => backend.workspace_step(-1),
        "pane" if !args.is_empty() => backend.pane_focus(args[0]),
        "zoom" => backend.zoom(),
        "scroll" if !args.is_empty() => {
            let n = args[0]
                .parse::<i64>()
                .map_err(|_| DispatchError(format!("bad scroll amount: {:?}", args[0])))?;
            backend.scroll(n)
        }
        "keys" if !args.is_empty() => backend.send_keys(&args),
        "text" if !args.is_empty() => {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(args[0])
                .map_err(|e| DispatchError(format!("bad base64 text: {e}")))?;
            backend.send_text(&String::from_utf8_lossy(&decoded))
        }
        _ => {
            eprintln!("dispatch: ignoring unknown intent: {line:?}");
            Ok(())
        }
    }
}

// ---- herdr backend ---------------------------------------------------------

mod herdr {
    use super::{neighbor, DispatchError, WHEEL_DOWN, WHEEL_UP};
    use serde_json::Value;
    use std::process::Command;

    fn bin() -> String {
        std::env::var("CC_HERDR_BIN").unwrap_or_else(|_| "herdr".to_string())
    }

    /// Run `herdr <args>` and return the parsed `result` object (or Null).
    fn run(args: &[&str]) -> Result<Value, DispatchError> {
        let out = Command::new(bin())
            .args(args)
            .output()
            .map_err(|e| DispatchError(format!("herdr {}: {e}", args.join(" "))))?;
        if !out.status.success() {
            return Err(DispatchError(format!(
                "herdr {} exited {}: {}",
                args.join(" "),
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let trimmed = stdout.trim();
        if trimmed.is_empty() {
            return Ok(Value::Null);
        }
        // herdr returns {"result": {...}}; some commands print plain text.
        match serde_json::from_str::<Value>(trimmed) {
            Ok(v) => Ok(v.get("result").cloned().unwrap_or(v)),
            Err(_) => Ok(Value::Null),
        }
    }

    fn list(args: &[&str], key: &str) -> Result<Vec<Value>, DispatchError> {
        Ok(run(args)?
            .get(key)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default())
    }

    fn focused_id<'a>(items: &'a [Value], id_key: &str) -> Option<&'a str> {
        items
            .iter()
            .find(|it| it.get("focused").and_then(Value::as_bool).unwrap_or(false))
            .and_then(|it| it.get(id_key))
            .and_then(Value::as_str)
    }

    fn focused_workspace_id() -> Result<Option<String>, DispatchError> {
        let wss = list(&["workspace", "list"], "workspaces")?;
        Ok(focused_id(&wss, "workspace_id").map(str::to_string))
    }

    fn focused_pane() -> Result<Option<String>, DispatchError> {
        let panes = list(&["pane", "list"], "panes")?;
        Ok(focused_id(&panes, "pane_id").map(str::to_string))
    }

    pub fn tab_step(step: i64) -> Result<(), DispatchError> {
        let ws_id = focused_workspace_id()?;
        let mut tabs = list(&["tab", "list"], "tabs")?;
        let ws = ws_id.as_deref();
        tabs.retain(|t| t.get("workspace_id").and_then(Value::as_str) == ws);
        tabs.sort_by_key(|t| t.get("number").and_then(Value::as_i64).unwrap_or(0));
        if let Some(target) = neighbor(&tabs, "tab_id", step) {
            run(&["tab", "focus", target])?;
        }
        Ok(())
    }

    pub fn workspace_step(step: i64) -> Result<(), DispatchError> {
        let mut wss = list(&["workspace", "list"], "workspaces")?;
        wss.sort_by_key(|w| w.get("number").and_then(Value::as_i64).unwrap_or(0));
        if let Some(target) = neighbor(&wss, "workspace_id", step) {
            run(&["workspace", "focus", target])?;
        }
        Ok(())
    }

    pub fn pane_focus(direction: &str) -> Result<(), DispatchError> {
        let pane = focused_pane()?;
        let mut args = vec!["pane", "focus", "--direction", direction];
        match &pane {
            Some(p) => {
                args.push("--pane");
                args.push(p);
            }
            None => args.push("--current"),
        }
        run(&args).map(|_| ())
    }

    pub fn zoom() -> Result<(), DispatchError> {
        let pane = focused_pane()?;
        let mut args = vec!["pane", "zoom", "--toggle"];
        match &pane {
            Some(p) => {
                args.push("--pane");
                args.push(p);
            }
            None => args.push("--current"),
        }
        run(&args).map(|_| ())
    }

    pub fn send_keys(keys: &[&str]) -> Result<(), DispatchError> {
        let Some(pane) = focused_pane()? else {
            return Ok(());
        };
        let mut args = vec!["pane", "send-keys", pane.as_str()];
        args.extend_from_slice(keys);
        run(&args).map(|_| ())
    }

    pub fn send_text(text: &str) -> Result<(), DispatchError> {
        let Some(pane) = focused_pane()? else {
            return Ok(());
        };
        run(&["pane", "send-text", pane.as_str(), text]).map(|_| ())
    }

    pub fn scroll(lines: i64) -> Result<(), DispatchError> {
        if lines == 0 {
            return Ok(());
        }
        let Some(pane) = focused_pane()? else {
            return Ok(());
        };
        let seq = if lines > 0 { WHEEL_UP } else { WHEEL_DOWN };
        let payload = seq.repeat(lines.unsigned_abs() as usize);
        run(&["pane", "send-text", pane.as_str(), &payload]).map(|_| ())
    }
}

// ---- tmux backend ----------------------------------------------------------

mod tmux {
    use super::{DispatchError, WHEEL_DOWN, WHEEL_UP};
    use std::process::Command;

    fn bin() -> String {
        std::env::var("CC_TMUX_BIN").unwrap_or_else(|_| "tmux".to_string())
    }

    fn run(args: &[&str]) -> Result<(), DispatchError> {
        let out = Command::new(bin())
            .args(args)
            .output()
            .map_err(|e| DispatchError(format!("tmux {}: {e}", args.join(" "))))?;
        if !out.status.success() {
            return Err(DispatchError(format!(
                "tmux {} exited {}: {}",
                args.join(" "),
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(())
    }

    /// herdr key names (lowercase) → tmux key names; unknown names pass through.
    fn keyname(k: &str) -> String {
        match k.to_ascii_lowercase().as_str() {
            "enter" => "Enter",
            "esc" | "escape" => "Escape",
            "up" => "Up",
            "down" => "Down",
            "left" => "Left",
            "right" => "Right",
            "pageup" => "PageUp",
            "pagedown" => "PageDown",
            "home" => "Home",
            "end" => "End",
            "space" => "Space",
            "tab" => "Tab",
            "backspace" => "BSpace",
            _ => return k.to_string(),
        }
        .to_string()
    }

    pub fn tab_step(step: i64) -> Result<(), DispatchError> {
        run(&[if step > 0 {
            "next-window"
        } else {
            "previous-window"
        }])
    }

    pub fn workspace_step(step: i64) -> Result<(), DispatchError> {
        run(&["switch-client", if step > 0 { "-n" } else { "-p" }])
    }

    pub fn pane_focus(direction: &str) -> Result<(), DispatchError> {
        let flag = match direction {
            "left" => "-L",
            "right" => "-R",
            "up" => "-U",
            "down" => "-D",
            other => return Err(DispatchError(format!("unknown pane direction: {other:?}"))),
        };
        run(&["select-pane", flag])
    }

    pub fn zoom() -> Result<(), DispatchError> {
        run(&["resize-pane", "-Z"])
    }

    pub fn send_keys(keys: &[&str]) -> Result<(), DispatchError> {
        if keys.is_empty() {
            return Ok(());
        }
        let names: Vec<String> = keys.iter().map(|k| keyname(k)).collect();
        let mut args = vec!["send-keys"];
        args.extend(names.iter().map(String::as_str));
        run(&args)
    }

    pub fn send_text(text: &str) -> Result<(), DispatchError> {
        if text.is_empty() {
            return Ok(());
        }
        // `-l` = literal; `--` guards text starting with '-'.
        run(&["send-keys", "-l", "--", text])
    }

    pub fn scroll(lines: i64) -> Result<(), DispatchError> {
        if lines == 0 {
            return Ok(());
        }
        let seq = if lines > 0 { WHEEL_UP } else { WHEEL_DOWN };
        let payload = seq.repeat(lines.unsigned_abs() as usize);
        run(&["send-keys", "-l", "--", &payload])
    }
}

/// The id `step` positions from the focused item (wraps). If none is focused,
/// the first item. Mirrors `herdr.py::_neighbor`.
fn neighbor<'a>(items: &'a [Value], id_key: &str, step: i64) -> Option<&'a str> {
    if items.is_empty() {
        return None;
    }
    let len = items.len() as i64;
    let idx = match items
        .iter()
        .position(|it| it.get("focused").and_then(Value::as_bool).unwrap_or(false))
    {
        Some(i) => (((i as i64 + step) % len + len) % len) as usize,
        None => 0,
    };
    items[idx].get(id_key).and_then(Value::as_str)
}
