//! cc-herdr-controller (Rust V2) — drive herdr from a game controller.
//!
//! The controller is read locally (SDL). Actions become compact *intents* that
//! are streamed over one persistent SSH connection to a relay running next to
//! herdr on the Mac, which executes them against herdr's local socket.

mod config;
mod controller;
mod intents;
mod run;
mod transport;

use serde_json::Value;
use std::path::PathBuf;
use transport::{local_transport, ssh_transport, StdoutTransport, Transport};

const HELP: &str = "\
cc-herdr-controller - drive herdr from a game controller (Rust V2)

USAGE:
    cc-herdr-controller [MODE] [OPTIONS]

MODES:
    (default)        run and drive herdr (over SSH, or locally with --local)
    --monitor        watch inputs -> actions, do NOTHING (add --perform to act)
    --dry-run        run, but print intents instead of sending them
    --list           list detected controllers
    --discover       print raw input events + the name each maps to
    --calibrate      press each control to (re)build the profile in mapping.json

OPTIONS:
    --local          run herdr on THIS machine (local relay, no SSH)
    --perform        with --monitor, actually send intents
    --host <target>  ssh target for the relay (overrides mapping.json remote.ssh_host)
    --config <path>  path to mapping.json (default: nearest one above the cwd)
    -h, --help       show this help

By default intents go over SSH to `relay.py` running next to herdr on another
machine (configure the `remote` block in mapping.json, or pass --host). With
--local, the relay runs as a subprocess on this machine instead (the `local`
block). Use --dry-run to test the controller mapping without touching herdr.";

#[derive(PartialEq)]
enum Mode {
    Run,
    Monitor,
    List,
    Discover,
    Calibrate,
}

fn build_transport(
    cfg: &Value,
    host_override: Option<String>,
    local: bool,
    cfg_dir: Option<PathBuf>,
) -> Result<Box<dyn Transport>, String> {
    if local {
        let relay_cmd = cfg["local"]["relay_cmd"]
            .as_str()
            .unwrap_or("python3 relay.py")
            .to_string();
        return Ok(Box::new(local_transport(relay_cmd, cfg_dir)?));
    }
    let remote = &cfg["remote"];
    let host = host_override
        .or_else(|| remote["ssh_host"].as_str().map(str::to_string))
        .filter(|h| !h.is_empty())
        .ok_or_else(|| {
            "no relay host. Set remote.ssh_host in mapping.json, pass --host user@mac, \
             or use --local to run herdr on this machine. \
             (Use --dry-run to test the mapping without touching herdr.)"
                .to_string()
        })?;
    let relay_cmd = remote["relay_cmd"]
        .as_str()
        .unwrap_or("cd ~/cc-herdr-controller && python3 relay.py")
        .to_string();
    let mut extra_args = vec![
        "-o".into(),
        "ServerAliveInterval=15".into(),
        "-o".into(),
        "ServerAliveCountMax=3".into(),
    ];
    if let Some(arr) = remote["ssh_args"].as_array() {
        for a in arr {
            if let Some(s) = a.as_str() {
                extra_args.push(s.to_string());
            }
        }
    }
    Ok(Box::new(ssh_transport(host, extra_args, relay_cmd)?))
}

fn real_main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let mut mode = Mode::Run;
    let mut perform = false;
    let mut dry = false;
    let mut local = false;
    let mut host_override: Option<String> = None;
    let mut config_path: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--list" => mode = Mode::List,
            "--discover" => mode = Mode::Discover,
            "--calibrate" => mode = Mode::Calibrate,
            "--monitor" => mode = Mode::Monitor,
            "--perform" => perform = true,
            "--dry-run" => dry = true,
            "--local" => local = true,
            "--host" => {
                i += 1;
                host_override = args.get(i).cloned();
            }
            "--config" => {
                i += 1;
                config_path = args.get(i).cloned();
            }
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            other => return Err(format!("unknown argument: {other}\n\n{HELP}")),
        }
        i += 1;
    }

    let cfg_path = config::find_config(config_path.as_deref());
    // Directory holding mapping.json — also where relay.py / herdr.py live, so
    // --local runs the relay from there.
    let cfg_dir = cfg_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf());

    match mode {
        Mode::List => controller::run_list(),
        Mode::Calibrate => controller::run_calibrate(&cfg_path),
        Mode::Discover => {
            let cfg = config::load(&cfg_path)?;
            controller::run_discover(&cfg)
        }
        Mode::Monitor => {
            let cfg = config::load(&cfg_path)?;
            if perform {
                let mut t = build_transport(&cfg, host_override, local, cfg_dir)?;
                run::run_loop(&cfg, true, "MONITOR", t.as_mut())
            } else {
                let mut t = StdoutTransport;
                run::run_loop(&cfg, false, "MONITOR", &mut t)
            }
        }
        Mode::Run => {
            let cfg = config::load(&cfg_path)?;
            if dry {
                let mut t = StdoutTransport;
                run::run_loop(&cfg, false, "DRY-RUN", &mut t)
            } else {
                let mut t = build_transport(&cfg, host_override, local, cfg_dir)?;
                run::run_loop(&cfg, true, "RUNNING", t.as_mut())
            }
        }
    }
}

fn main() {
    if let Err(e) = real_main() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
