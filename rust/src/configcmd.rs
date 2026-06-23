//! `cc-controller config …` — read and edit mapping.json from the CLI.
//!
//! `show` / `get` / `set` / `path` are non-interactive. `bind` is a quick
//! two-prompt binder (cliclack). `edit` is a full-screen arrow-key grid: ↑/↓
//! moves between buttons (and the cyclable settings), ←/→ changes each one's
//! value, Enter saves, Esc cancels. Interactive commands require a TTY. All
//! writes go through `config::save`, which preserves key order and the
//! `_comment` keys (preserve_order).

use crate::cli::ConfigAction;
use crate::config;
use console::{style, Key, Term};
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
    let mut lines: Vec<String> = Vec::new();

    lines.push(format!(
        "{}   {}",
        style("backend").bold(),
        style(cfg["backend"].as_str().unwrap_or("herdr")).cyan()
    ));

    lines.push(String::new());
    lines.push(style("bindings").bold().to_string());
    if let Some(b) = cfg["bindings"].as_object() {
        // Pad the plain control name to width, THEN colour it, so the ANSI
        // codes don't throw the column alignment off.
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
            lines.push(format!(
                "  {}  {}",
                style(format!("{ctrl:<width$}")).green(),
                value_str(act)
            ));
        }
    }

    lines.push(String::new());
    lines.push(style("settings").bold().to_string());
    for key in ["trigger_threshold", "dictation_command"] {
        if let Some(v) = config::get_path(&cfg, &format!("settings.{key}")) {
            lines.push(format!(
                "  {}  {}",
                style(format!("{key:<18}")).dim(),
                value_str(v)
            ));
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
            lines.push(format!(
                "  {}  {}",
                style(format!("{group:<18}")).dim(),
                inline.join("  ")
            ));
        }
    }

    crate::ui::boxed("config", &lines);
    println!("{} {}", style("file:").dim(), cfg_path.display());
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
    cliclack::note(
        "Saved",
        format!("{} → {}", style(&control).green(), style(&action).cyan()),
    )
    .map_err(ui_err)?;
    cliclack::outro(style("binding updated").green()).map_err(ui_err)?;
    Ok(())
}

// ---- interactive grid editor (`config edit`) -------------------------------

const UNBOUND: &str = "—";

/// One editable row: a button binding or a settings choice, with a cyclable
/// set of options. For binding rows, option 0 (`—`) means "unbound".
struct Row {
    label: String,
    path: String,
    options: Vec<String>,
    idx: usize,
    binding: bool,
}

fn choice_row(label: &str, path: &str, opts: &[&str], current: &str) -> Row {
    let options: Vec<String> = opts.iter().map(|s| s.to_string()).collect();
    let idx = options.iter().position(|o| o == current).unwrap_or(0);
    Row {
        label: label.into(),
        path: path.into(),
        options,
        idx,
        binding: false,
    }
}

/// Build the editable rows: one per bindable control (button), then the handful
/// of cyclable settings. Returns `(rows, number_of_binding_rows)`.
fn build_rows(cfg: &Value) -> (Vec<Row>, usize) {
    let mut rows = Vec::new();
    let bindings = cfg["bindings"].as_object();
    for ctrl in bindable_controls(cfg) {
        let mut options = vec![UNBOUND.to_string()];
        options.extend(ACTIONS.iter().map(|(v, _)| v.to_string()));
        let cur = bindings.and_then(|b| b.get(&ctrl)).and_then(Value::as_str);
        if let Some(a) = cur {
            if !options.iter().any(|o| o == a) {
                options.push(a.to_string()); // preserve an unknown/custom action
            }
        }
        let idx = cur
            .and_then(|a| options.iter().position(|o| o == a))
            .unwrap_or(0);
        rows.push(Row {
            label: ctrl.clone(),
            path: format!("bindings.{ctrl}"),
            options,
            idx,
            binding: true,
        });
    }
    let n_bindings = rows.len();

    let backend = cfg["backend"].as_str().unwrap_or("herdr");
    rows.push(choice_row(
        "backend",
        "backend",
        &["herdr", "tmux"],
        backend,
    ));
    let invert = config::get_path(cfg, "settings.scroll.invert")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    rows.push(choice_row(
        "scroll invert",
        "settings.scroll.invert",
        &["false", "true"],
        if invert { "true" } else { "false" },
    ));
    let voice = config::get_path(cfg, "settings.voice.mode")
        .and_then(Value::as_str)
        .unwrap_or("both");
    rows.push(choice_row(
        "voice mode",
        "settings.voice.mode",
        &["both", "hold", "toggle"],
        voice,
    ));

    (rows, n_bindings)
}

fn render_row(r: &Row, selected: bool, label_w: usize) -> String {
    let val = &r.options[r.idx];
    if selected {
        format!(
            "{} {}  {}",
            style("❯").cyan().bold(),
            style(format!("{:<label_w$}", r.label)).bold(),
            style(format!(" ‹ {val} › ")).black().on_cyan()
        )
    } else {
        let label = format!("{:<label_w$}", r.label);
        let shown = if val == UNBOUND {
            style(UNBOUND).dim().to_string()
        } else {
            val.clone()
        };
        format!("  {label}  {shown}")
    }
}

fn build_frame(rows: &[Row], sel: usize, n_bindings: usize, label_w: usize) -> Vec<String> {
    let mut out = vec![
        style("edit mapping.json").bold().to_string(),
        style("buttons").cyan().bold().to_string(),
    ];
    for (i, r) in rows.iter().enumerate() {
        if i == n_bindings {
            out.push(style("settings").cyan().bold().to_string());
        }
        out.push(render_row(r, i == sel, label_w));
    }
    out.push(String::new());
    out.push(
        style("↑/↓ button   ←/→ action   enter save   esc cancel")
            .dim()
            .to_string(),
    );
    out
}

fn edit(cfg_path: &Path) -> Result<(), String> {
    require_tty()?;
    let mut cfg = config::load(cfg_path)?;
    let (mut rows, n_bindings) = build_rows(&cfg);
    if rows.is_empty() {
        return Err("nothing to edit (run `cc-controller calibrate` first)".into());
    }
    let label_w = rows.iter().map(|r| r.label.len()).max().unwrap_or(0);

    let term = Term::stderr();
    let _ = term.hide_cursor();
    let mut sel = 0usize;
    let mut prev_lines: Option<usize> = None;

    let saved = loop {
        let frame = build_frame(&rows, sel, n_bindings, label_w);
        if let Some(n) = prev_lines {
            let _ = term.clear_last_lines(n);
        }
        for line in &frame {
            let _ = term.write_line(line);
        }
        prev_lines = Some(frame.len());

        match term.read_key() {
            Ok(Key::ArrowUp) => sel = (sel + rows.len() - 1) % rows.len(),
            Ok(Key::ArrowDown) => sel = (sel + 1) % rows.len(),
            Ok(Key::ArrowLeft) => {
                let r = &mut rows[sel];
                r.idx = (r.idx + r.options.len() - 1) % r.options.len();
            }
            Ok(Key::ArrowRight) => {
                let r = &mut rows[sel];
                r.idx = (r.idx + 1) % r.options.len();
            }
            Ok(Key::Enter) => break true,
            Ok(Key::Escape) => break false,
            Ok(Key::Char(c)) if c == 'q' || c == '\u{3}' => break false,
            Ok(_) => {}            // ignore unmapped keys
            Err(_) => break false, // EOF / read error -> cancel (never spin)
        }
    };

    let _ = term.show_cursor();
    if !saved {
        let _ = term.write_line(&style("cancelled — no changes saved").dim().to_string());
        return Ok(());
    }

    for r in &rows {
        if r.binding && r.idx == 0 {
            // unbound: drop the binding key if present
            if let Some(b) = cfg["bindings"].as_object_mut() {
                b.remove(&r.label);
            }
        } else {
            config::set_path(&mut cfg, &r.path, &r.options[r.idx])?;
        }
    }
    config::save(cfg_path, &cfg)?;
    let _ = term.write_line(&format!(
        "{} {}",
        style("✓").green().bold(),
        style("saved").green()
    ));
    Ok(())
}
