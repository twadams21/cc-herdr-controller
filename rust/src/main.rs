//! cc-controller — drive herdr or tmux from a game controller.
//!
//! One binary, three run modes:
//!
//!   - `local`  — controller + multiplexer on this machine; dispatch in-process.
//!   - `remote` — controller here; intents stream over SSH to `cc-controller host`.
//!   - `host`   — multiplexer-side dispatcher: read intents on stdin, drive the mux.
//!
//! Plus controller setup (`list`/`discover`/`calibrate`) and `config` editing.

mod cli;
mod config;
mod configcmd;
mod controller;
mod daemon;
mod dispatch;
mod intents;
mod run;
mod transport;
mod ui;

use clap::Parser;
use cli::{Cli, Command, Lifecycle, LocalArgs, RemoteArgs};
use dispatch::Backend;
use serde_json::Value;
use std::path::Path;
use transport::{ssh_transport, LocalTransport, StdoutTransport, Transport};

/// Build the transport for a performing (non-dry-run) controller loop.
fn build_transport(
    cfg: &Value,
    backend: Backend,
    host_override: Option<String>,
    local: bool,
) -> Result<Box<dyn Transport>, String> {
    if local {
        return Ok(Box::new(LocalTransport::new(backend)));
    }
    // Remote: ssh to the host and run `cc-controller host --backend <name>`.
    let remote = &cfg["remote"];
    let host = host_override
        .or_else(|| remote["ssh_host"].as_str().map(str::to_string))
        .filter(|h| !h.is_empty())
        .ok_or_else(|| {
            "no host. Set remote.ssh_host in mapping.json or pass --host user@mac. \
             (Use --dry-run to test the mapping without touching the multiplexer.)"
                .to_string()
        })?;
    let host_bin = remote["host_bin"].as_str().unwrap_or("cc-controller");
    let remote_cmd = format!("{host_bin} host --backend {}", backend.name());
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
    Ok(Box::new(ssh_transport(host, extra_args, remote_cmd)?))
}

/// The `host` subcommand: read newline-delimited intents on stdin and run each
/// against the local multiplexer. Normally fed by `remote` over SSH.
fn run_host(backend: Backend) -> Result<(), String> {
    use std::io::BufRead;
    eprintln!("host: ready (backend={})", backend.name());
    let stdin = std::io::stdin();
    for raw in stdin.lock().lines() {
        let raw = raw.map_err(|e| e.to_string())?;
        let line = raw.trim_start_matches('\u{feff}').trim(); // tolerate a stray BOM
        if line.is_empty() {
            continue;
        }
        if let Err(e) = dispatch::handle_intent(line, backend) {
            eprintln!("host: {line:?}: {e}");
        }
    }
    eprintln!("host: stdin closed, exiting");
    Ok(())
}

/// Foreground argv (`<mode> run …`) to relaunch detached for `start`.
fn forwarded_config(cfg_path: &Path) -> [String; 2] {
    ["--config".into(), cfg_path.display().to_string()]
}

fn run_local(args: LocalArgs, cfg_path: &Path) -> Result<(), String> {
    match args.action {
        Lifecycle::Run => {
            let cfg = config::load(cfg_path)?;
            let backend = Backend::resolve(args.backend.as_deref(), &cfg)?;
            if args.dry_run {
                let mut t = StdoutTransport;
                run::run_loop(&cfg, false, "LOCAL (dry-run)", &mut t)
            } else {
                let mut t = build_transport(&cfg, backend, None, true)?;
                run::run_loop(&cfg, true, "LOCAL", t.as_mut())
            }
        }
        Lifecycle::Start => {
            let mut argv = vec!["local".to_string(), "run".to_string()];
            if args.dry_run {
                argv.push("--dry-run".into());
            }
            if let Some(b) = &args.backend {
                argv.push("--backend".into());
                argv.push(b.clone());
            }
            argv.extend(forwarded_config(cfg_path));
            daemon::start(
                &argv,
                &daemon::pidfile(cfg_path),
                &daemon::logfile(cfg_path),
            )
        }
        Lifecycle::Stop => daemon::stop(&daemon::pidfile(cfg_path)),
        Lifecycle::Status => daemon::status(&daemon::pidfile(cfg_path), &daemon::logfile(cfg_path)),
    }
}

fn run_remote(args: RemoteArgs, cfg_path: &Path) -> Result<(), String> {
    match args.action {
        Lifecycle::Run => {
            let cfg = config::load(cfg_path)?;
            let backend = Backend::resolve(args.backend.as_deref(), &cfg)?;
            if args.dry_run {
                let mut t = StdoutTransport;
                run::run_loop(&cfg, false, "REMOTE (dry-run)", &mut t)
            } else {
                let mut t = build_transport(&cfg, backend, args.host, false)?;
                run::run_loop(&cfg, true, "REMOTE", t.as_mut())
            }
        }
        Lifecycle::Start => {
            let mut argv = vec!["remote".to_string(), "run".to_string()];
            if args.dry_run {
                argv.push("--dry-run".into());
            }
            if let Some(h) = &args.host {
                argv.push("--host".into());
                argv.push(h.clone());
            }
            if let Some(b) = &args.backend {
                argv.push("--backend".into());
                argv.push(b.clone());
            }
            argv.extend(forwarded_config(cfg_path));
            daemon::start(
                &argv,
                &daemon::pidfile(cfg_path),
                &daemon::logfile(cfg_path),
            )
        }
        Lifecycle::Stop => daemon::stop(&daemon::pidfile(cfg_path)),
        Lifecycle::Status => daemon::status(&daemon::pidfile(cfg_path), &daemon::logfile(cfg_path)),
    }
}

fn real_main() -> Result<(), String> {
    let cli = Cli::parse();
    let cfg_path = config::find_config(cli.config.as_deref());

    match cli.command {
        Command::List => controller::run_list(),
        Command::Calibrate => controller::run_calibrate(&cfg_path),
        Command::Discover => {
            let cfg = config::load(&cfg_path)?;
            controller::run_discover(&cfg)
        }
        Command::Local(args) => run_local(args, &cfg_path),
        Command::Remote(args) => run_remote(args, &cfg_path),
        Command::Host(args) => {
            let cfg = config::load(&cfg_path).unwrap_or(Value::Null);
            let backend = Backend::resolve(args.backend.as_deref(), &cfg)?;
            run_host(backend)
        }
        Command::Config(args) => configcmd::run(args.action, &cfg_path),
    }
}

fn main() {
    if let Err(e) = real_main() {
        ui::fail(&e);
        std::process::exit(1);
    }
}
