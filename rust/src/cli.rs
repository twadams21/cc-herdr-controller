//! Command-line surface (clap). One binary, three run modes (local / remote /
//! host) plus controller setup and config editing.

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "cc-controller",
    version,
    about = "Drive herdr or tmux from a game controller",
    long_about = "Map a game controller (Switch Pro, Xbox, DualSense, …) to herdr or \
tmux: switch windows/tabs and sessions/workspaces, move between panes, scroll, \
send keys, and trigger voice — all over the multiplexer's own socket, so it \
works headless / over SSH."
)]
pub struct Cli {
    /// Path to mapping.json (default: nearest one above the cwd).
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Controller + multiplexer on THIS machine (dispatch in-process, no SSH).
    Local(LocalArgs),
    /// Controller here; stream intents over SSH to the multiplexer machine.
    Remote(RemoteArgs),
    /// Multiplexer-side dispatcher: read intents on stdin, drive herdr/tmux.
    /// Normally spawned over SSH by `remote`; runnable by hand for debugging.
    Host(HostArgs),
    /// List detected controllers.
    List,
    /// Print raw controller input + the name each maps to.
    Discover,
    /// Press each control to (re)build the name→index profile in mapping.json.
    Calibrate,
    /// Read or edit mapping.json.
    Config(ConfigArgs),
}

/// run (default) | start | stop | status — lifecycle for a controller mode.
#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum Lifecycle {
    /// Run in the foreground until Ctrl-C.
    Run,
    /// Start detached in the background (logs to a file).
    Start,
    /// Stop the background instance.
    Stop,
    /// Report whether a background instance is running.
    Status,
}

#[derive(Args)]
pub struct LocalArgs {
    #[arg(value_enum, default_value_t = Lifecycle::Run)]
    pub action: Lifecycle,
    /// Observe only: print intents, dispatch nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Multiplexer to drive: herdr (default) or tmux. Overrides mapping.json.
    #[arg(long, value_name = "NAME")]
    pub backend: Option<String>,
}

#[derive(Args)]
pub struct RemoteArgs {
    #[arg(value_enum, default_value_t = Lifecycle::Run)]
    pub action: Lifecycle,
    /// Observe only: print intents, dispatch nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// SSH target for the host (overrides mapping.json remote.ssh_host).
    #[arg(long, value_name = "TARGET")]
    pub host: Option<String>,
    /// Backend to run on the host: herdr (default) or tmux.
    #[arg(long, value_name = "NAME")]
    pub backend: Option<String>,
}

#[derive(Args)]
pub struct HostArgs {
    /// Multiplexer to drive: herdr (default) or tmux. Overrides mapping.json.
    #[arg(long, value_name = "NAME")]
    pub backend: Option<String>,
}

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: ConfigAction,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show the current bindings and key settings.
    Show,
    /// Read a value by dotted path (e.g. `backend`, `bindings.A`).
    Get {
        #[arg(value_name = "PATH")]
        path: String,
    },
    /// Write a value by dotted path (e.g. `backend tmux`, `bindings.A enter`).
    Set {
        #[arg(value_name = "PATH")]
        path: String,
        #[arg(value_name = "VALUE")]
        value: String,
    },
    /// Interactively bind a control to an action.
    Bind,
    /// Interactively edit the common settings (backend, scroll, voice).
    Edit,
    /// Print the resolved mapping.json path.
    Path,
}
