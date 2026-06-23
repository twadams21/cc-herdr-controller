//! `cc-controller config …` — read and edit mapping.json from the CLI.
//!
//! `show` / `get` / `set` / `path` are non-interactive; `bind` / `edit` use
//! cliclack prompts (and require a TTY). All writes go through `config::save`,
//! which now preserves key order and the `_comment` keys (preserve_order).

use crate::cli::ConfigAction;
use crate::config;
use console::style;
use serde_json::Value;
use std::path::Path;

/// Actions a control can be bound to (value, one-line hint).
const ACTIONS: &[(&str, &str)] = &[
    ("tab_next", "next tab / window"),
    ("tab_prev", "previous tab / window"),
    ("workspace_next", "next workspace / session"),
    ("workspace_prev", "previous workspace / session"),
    ("pane_left", "focus pane left"),
    ("pane_right", "focus pane right"),
    ("pane_up", "focus pane up"),
    ("pane_down", "focus pane down"),
    ("pane_zoom", "toggle pane zoom"),
    ("scroll_up", "scroll up"),
    ("scroll_down", "scroll down"),
    ("enter", "send Enter"),
    ("escape", "send Escape"),
    ("voice", "hold-space voice mode"),
    ("dictation", "run the OS dictation command"),
    ("noop", "do nothing (unbind)"),
];

pub fn run(action: ConfigAction, cfg_path: &Path) -> Result<(), String> {
    match action {
        ConfigAction::Path => {
            println!("{}", cfg_path.display());
            Ok(())
        }
        ConfigAction::Show => show(cfg_path),
        ConfigAction::Get { path } => get(cfg_path, &path),
        ConfigAction::Set { path, value } => set(cfg_path, &path, &value),
        ConfigAction::Bind => bind(cfg_path),
        ConfigAction::Edit => edit(cfg_path),
    }
}

/// String form of a value: bare for strings, JSON for everything else.
fn value_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn show(cfg_path: &Path) -> Result<(), String> {
    let cfg = config::load(cfg_path)?;

    println!(
        "{}  {}",
        style("backend").bold(),
        style(cfg["backend"].as_str().unwrap_or("herdr")).cyan()
    );

    println!("\n{}", style("bindings").bold().underlined());
    if let Some(b) = cfg["bindings"].as_object() {
        let width = b
            .keys()
            .filter(|k| *k != "_comment")
            .map(String::len)
            .max()
            .unwrap_or(0);
        for (ctrl, act) in b {
            if ctrl == "_comment" {
                continue;
            }
            println!(
                "  {:<width$}  {}",
                style(ctrl).green(),
                value_str(act),
                width = width
            );
        }
    }

    println!("\n{}", style("settings").bold().underlined());
    for key in ["trigger_threshold", "dictation_command"] {
        if let Some(v) = config::get_path(&cfg, &format!("settings.{key}")) {
            println!("  {:<18}  {}", style(key).dim(), value_str(v));
        }
    }
    for group in ["scroll", "arrows", "voice"] {
        if let Some(obj) =
            config::get_path(&cfg, &format!("settings.{group}")).and_then(Value::as_object)
        {
            let inline: Vec<String> = obj
                .iter()
                .filter(|(k, _)| *k != "_comment")
                .map(|(k, v)| format!("{k}={}", value_str(v)))
                .collect();
            println!("  {:<18}  {}", style(group).dim(), inline.join("  "));
        }
    }

    println!("\n{} {}", style("file:").dim(), cfg_path.display());
    Ok(())
}

fn get(cfg_path: &Path, path: &str) -> Result<(), String> {
    let cfg = config::load(cfg_path)?;
    match config::get_path(&cfg, path) {
        Some(v) => {
            println!("{}", value_str(v));
            Ok(())
        }
        None => Err(format!("no such key: {path}")),
    }
}

fn set(cfg_path: &Path, path: &str, value: &str) -> Result<(), String> {
    let mut cfg = config::load(cfg_path)?;
    let set = config::set_path(&mut cfg, path, value)?;
    config::save(cfg_path, &cfg)?;
    println!("{} {path} = {}", style("set").green(), value_str(&set));
    Ok(())
}

fn require_tty() -> Result<(), String> {
    if console::user_attended() {
        Ok(())
    } else {
        Err("this command needs an interactive terminal; use `config set <path> <value>`".into())
    }
}

/// Map a cliclack interaction error (incl. Ctrl-C) to a friendly message.
fn ui_err(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::Interrupted {
        "cancelled".into()
    } else {
        e.to_string()
    }
}

/// Control names that can be bound — profile buttons + axes (e.g. ZL/ZR).
fn bindable_controls(cfg: &Value) -> Vec<String> {
    let mut names = Vec::new();
    for group in ["buttons", "axes"] {
        if let Some(obj) =
            config::get_path(cfg, &format!("profile.{group}")).and_then(Value::as_object)
        {
            names.extend(obj.keys().filter(|k| *k != "_comment").cloned());
        }
    }
    names.sort();
    names.dedup();
    names
}

fn bind(cfg_path: &Path) -> Result<(), String> {
    require_tty()?;
    let mut cfg = config::load(cfg_path)?;

    cliclack::intro(style(" bind a control ").on_cyan().black()).map_err(ui_err)?;

    let controls = bindable_controls(&cfg);
    if controls.is_empty() {
        return Err("no controls in the profile yet — run `cc-controller calibrate` first".into());
    }
    let current = cfg["bindings"].as_object();
    let mut control_select = cliclack::select("Control");
    for name in &controls {
        let bound = current
            .and_then(|b| b.get(name))
            .and_then(Value::as_str)
            .unwrap_or("(unbound)");
        control_select = control_select.item(name.clone(), name, bound);
    }
    let control: String = control_select.interact().map_err(ui_err)?;

    let mut action_select = cliclack::select(format!("Action for {control}"));
    for (val, hint) in ACTIONS {
        action_select = action_select.item((*val).to_string(), val, hint);
    }
    let action: String = action_select.interact().map_err(ui_err)?;

    config::set_path(&mut cfg, &format!("bindings.{control}"), &action)?;
    config::save(cfg_path, &cfg)?;
    cliclack::outro(format!("{control} → {action}")).map_err(ui_err)?;
    Ok(())
}

fn edit(cfg_path: &Path) -> Result<(), String> {
    require_tty()?;
    let mut cfg = config::load(cfg_path)?;

    cliclack::intro(style(" edit settings ").on_cyan().black()).map_err(ui_err)?;

    let backend: String = cliclack::select("Backend")
        .initial_value(cfg["backend"].as_str().unwrap_or("herdr").to_string())
        .item("herdr".to_string(), "herdr", "the herdr multiplexer")
        .item("tmux".to_string(), "tmux", "tmux (window/session/pane)")
        .interact()
        .map_err(ui_err)?;
    config::set_path(&mut cfg, "backend", &backend)?;

    let invert = cliclack::confirm("Invert scroll direction?")
        .initial_value(
            config::get_path(&cfg, "settings.scroll.invert")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        )
        .interact()
        .map_err(ui_err)?;
    config::set_path(&mut cfg, "settings.scroll.invert", &invert.to_string())?;

    let voice: String = cliclack::select("Voice button mode")
        .initial_value(
            config::get_path(&cfg, "settings.voice.mode")
                .and_then(Value::as_str)
                .unwrap_or("both")
                .to_string(),
        )
        .item("both".to_string(), "both", "tap = toggle, hold = momentary")
        .item("hold".to_string(), "hold", "momentary only")
        .item("toggle".to_string(), "toggle", "tap on/off only")
        .interact()
        .map_err(ui_err)?;
    config::set_path(&mut cfg, "settings.voice.mode", &voice)?;

    config::save(cfg_path, &cfg)?;
    cliclack::outro("saved").map_err(ui_err)?;
    Ok(())
}
