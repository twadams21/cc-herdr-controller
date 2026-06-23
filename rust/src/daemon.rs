//! Background lifecycle for the controller modes (`start` / `stop` / `status`).
//!
//! Ports `controller_daemon.py`'s `--bg` / `--stop` / `--status`: a PID file
//! next to mapping.json, a detached child whose output goes to a log file, and
//! signal-based stop. Only `local` / `remote` use this (they own the
//! controller; one at a time). `host` lives/dies with its SSH pipe.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

fn sibling(cfg_path: &Path, name: &str) -> PathBuf {
    match cfg_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(dir) => dir.join(name),
        None => PathBuf::from(name),
    }
}

pub fn pidfile(cfg_path: &Path) -> PathBuf {
    sibling(cfg_path, ".cc-controller.pid")
}

pub fn logfile(cfg_path: &Path) -> PathBuf {
    sibling(cfg_path, ".cc-controller.log")
}

fn read_pid(p: &Path) -> Option<u32> {
    fs::read_to_string(p).ok()?.trim().parse().ok()
}

/// Re-launch this binary with `argv` (the foreground form, e.g. `local run …`)
/// detached in the background, writing the PID file.
pub fn start(argv: &[String], pidfile: &Path, logfile: &Path) -> Result<(), String> {
    if let Some(pid) = read_pid(pidfile) {
        if alive(pid) {
            return Err(format!("already running (pid {pid}). Use `stop` first."));
        }
    }
    let exe = std::env::current_exe().map_err(|e| format!("cannot find own exe: {e}"))?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(logfile)
        .map_err(|e| format!("cannot open log {}: {e}", logfile.display()))?;
    let log_err = log.try_clone().map_err(|e| e.to_string())?;

    let mut cmd = Command::new(exe);
    cmd.args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    detach(&mut cmd);

    let child = cmd.spawn().map_err(|e| format!("failed to start: {e}"))?;
    let pid = child.id();
    fs::write(pidfile, pid.to_string())
        .map_err(|e| format!("cannot write {}: {e}", pidfile.display()))?;
    println!(
        "started in background (pid {pid}); logs -> {}",
        logfile.display()
    );
    Ok(())
}

pub fn stop(pidfile: &Path) -> Result<(), String> {
    let Some(pid) = read_pid(pidfile) else {
        println!("not running");
        return Ok(());
    };
    if !alive(pid) {
        let _ = fs::remove_file(pidfile);
        println!("not running");
        return Ok(());
    }
    terminate(pid);
    for _ in 0..50 {
        if !alive(pid) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = fs::remove_file(pidfile);
    println!("stopped (pid {pid})");
    Ok(())
}

pub fn status(pidfile: &Path, logfile: &Path) -> Result<(), String> {
    match read_pid(pidfile) {
        Some(pid) if alive(pid) => {
            println!("running (pid {pid}); logs -> {}", logfile.display());
        }
        _ => {
            let _ = fs::remove_file(pidfile);
            println!("not running");
        }
    }
    Ok(())
}

// ---- platform: liveness, terminate, detach ---------------------------------

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // kill(pid, 0): 0 = exists; EPERM also means it exists (just not ours).
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 || std_errno() == libc::EPERM }
}

#[cfg(unix)]
fn std_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

#[cfg(unix)]
fn terminate(pid: u32) {
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
}

#[cfg(unix)]
fn detach(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: setsid() in the forked child detaches it from the controlling
    // terminal (new session) so it survives the parent shell exiting. No
    // allocation or non-async-signal-safe work is done here.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
}

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

#[cfg(windows)]
fn terminate(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
}

#[cfg(windows)]
fn detach(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}
