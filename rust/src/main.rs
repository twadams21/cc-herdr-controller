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
use transport::{SshConfig, SshTransport, StdoutTransport, Transport};

const HELP: &str = "\
cc-herdr-controller - drive herdr from a game controller (Rust V2)

USAGE:
    cc-herdr-controller [MODE] [OPTIONS]

MODES:
    (default)        run and drive herdr (sends intents to the Mac relay)
    --monitor        watch inputs -> actions, do NOTHING (add --perform to act)
    --dry-run        run, but print intents instead of sending them
    --list           list detected controllers
    --discover       print raw input events + the name each maps to
    --calibrate      press each control to (re)build the profile in mapping.json

OPTIONS:
    --perform        with --monitor, actually send intents
    --host <target>  ssh target for the Mac relay (overrides mapping.json remote.ssh_host)
    --config <path>  path to mapping.json (default: nearest one above the cwd)
    -h, --help       show this help

The Mac side runs `relay.py` next to herdr. Configure the connection under a
`remote` block in mapping.json, or pass --host. Use --dry-run to test the
controller mapping without contacting the Mac.";

#[derive(PartialEq)]
enum Mode {
    Run,
    Monitor,
    List,
    Discover,
    Calibrate,
}

fn build_transport(cfg: &Value, host_override: Option<String>) -> Result<Box<dyn Transport>, String> {
    let remote = &cfg["remote"];
    let host = host_override
        .or_else(|| remote["ssh_host"].as_str().map(str::to_string))
        .filter(|h| !h.is_empty())
        .ok_or_else(|| {
            "no Mac SSH host. Set remote.ssh_host in mapping.json or pass --host user@mac. \
             (Use --dry-run to test the mapping without the Mac.)"
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
    Ok(Box::new(SshTransport::new(SshConfig { host, extra_args, relay_cmd })?))
}

fn real_main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let mut mode = Mode::Run;
    let mut perform = false;
    let mut dry = false;
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
                let mut t = build_transport(&cfg, host_override)?;
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
                let mut t = build_transport(&cfg, host_override)?;
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
