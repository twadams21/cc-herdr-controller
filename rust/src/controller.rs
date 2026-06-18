//! SDL joystick layer: enumerate, open, calibrate, discover. Uses the raw
//! Joystick API (by index), matching mapping.json's `profile`, so a calibration
//! made here interops with the Python daemon's `profile`.

use crate::config;
use sdl2::event::Event;
use sdl2::joystick::{HatState, Joystick};
use sdl2::{EventPump, Sdl};
use serde_json::Value;
use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

/// Order controls are requested during --calibrate (mirrors the Python tool).
const CALIBRATION_ORDER: &[&str] = &[
    "A",
    "B",
    "X",
    "Y",
    "L",
    "R",
    "ZL",
    "ZR",
    "minus",
    "plus",
    "home",
    "capture",
    "lstick",
    "rstick",
    "dpad_up",
    "dpad_down",
    "dpad_left",
    "dpad_right",
];

/// SDL i16 axis value -> -1.0..1.0 (matches pygame's normalization).
pub fn norm(v: i16) -> f32 {
    v as f32 / 32767.0
}

/// HatState -> the 4 cardinal direction names (diagonals ignored, like Python).
pub fn hat_dir(state: HatState) -> Option<&'static str> {
    match state {
        HatState::Up => Some("up"),
        HatState::Down => Some("down"),
        HatState::Left => Some("left"),
        HatState::Right => Some("right"),
        _ => None,
    }
}

pub fn init_sdl() -> Result<Sdl, String> {
    sdl2::init()
}

pub fn open_first(sdl: &Sdl) -> Result<Joystick, String> {
    let subsystem = sdl.joystick().map_err(|e| e.to_string())?;
    let n = subsystem.num_joysticks().map_err(|e| e.to_string())?;
    if n == 0 {
        return Err(
            "No controller detected. Paired / plugged in? Another process \
                    holding it? Only one program can own a controller at a time."
                .into(),
        );
    }
    let joy = subsystem.open(0).map_err(|e| e.to_string())?;
    println!(
        "Controller: {}  (buttons={} axes={} hats={})",
        joy.name(),
        joy.num_buttons(),
        joy.num_axes(),
        joy.num_hats()
    );
    Ok(joy)
}

pub fn run_list() -> Result<(), String> {
    let sdl = init_sdl()?;
    let subsystem = sdl.joystick().map_err(|e| e.to_string())?;
    let n = subsystem.num_joysticks().map_err(|e| e.to_string())?;
    if n == 0 {
        println!("No controllers detected.");
        return Ok(());
    }
    for i in 0..n {
        match subsystem.open(i) {
            Ok(j) => println!(
                "[{i}] {}  buttons={} axes={} hats={}",
                j.name(),
                j.num_buttons(),
                j.num_axes(),
                j.num_hats()
            ),
            Err(e) => println!("[{i}] <open failed: {e}>"),
        }
    }
    Ok(())
}

pub fn run_discover(cfg: &Value) -> Result<(), String> {
    let sdl = init_sdl()?;
    let _joy = open_first(&sdl)?; // keep it open for the event stream
    let (btn_name, axis_index) = config::name_maps(cfg);
    let axis_name: std::collections::HashMap<u32, String> =
        axis_index.iter().map(|(n, i)| (*i, n.clone())).collect();
    let mut pump = sdl.event_pump()?;

    println!("\nDISCOVER - press each control; Ctrl-C to quit.");
    println!("Shows raw index and the name your profile assigns it.\n");
    let mut seen: u64 = 0;
    let mut last_beat = Instant::now();
    loop {
        for ev in pump.poll_iter() {
            match ev {
                Event::JoyButtonDown { button_idx, .. } => {
                    seen += 1;
                    let bi = button_idx as u32;
                    let name = btn_name.get(&bi).map(String::as_str).unwrap_or("unnamed");
                    println!("button {bi}  DOWN   ({name})");
                }
                Event::JoyHatMotion { hat_idx, state, .. } if state != HatState::Centered => {
                    seen += 1;
                    let d = hat_dir(state).unwrap_or("?");
                    println!("hat {hat_idx}  {state:?}   (dpad_{d})");
                }
                Event::JoyAxisMotion {
                    axis_idx, value, ..
                } => {
                    let v = norm(value);
                    if v.abs() > 0.5 {
                        seen += 1;
                        let ai = axis_idx as u32;
                        let name = axis_name.get(&ai).map(String::as_str).unwrap_or("unnamed");
                        println!("axis {ai}  {v:+.2}   ({name})");
                    }
                }
                _ => {}
            }
        }
        if last_beat.elapsed() >= Duration::from_secs(2) {
            last_beat = Instant::now();
            if seen == 0 {
                println!(
                    "... listening, 0 events. If pressing does nothing, is another \
                          app holding the controller?"
                );
            } else {
                println!("... listening ({seen} events so far)");
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

enum Cal {
    Button(u32),
    Axis(u32),
    Hat,
    Skip,
    Quit,
}

fn spawn_stdin_reader() -> Receiver<String> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        loop {
            let mut line = String::new();
            if stdin.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            if tx.send(line.trim().to_string()).is_err() {
                break;
            }
        }
    });
    rx
}

fn await_input(pump: &mut EventPump, rx: &Receiver<String>) -> Cal {
    for _ in pump.poll_iter() {} // drain stale events
    loop {
        for ev in pump.poll_iter() {
            match ev {
                Event::JoyButtonDown { button_idx, .. } => return Cal::Button(button_idx as u32),
                Event::JoyAxisMotion {
                    axis_idx, value, ..
                } if norm(value).abs() > 0.7 => return Cal::Axis(axis_idx as u32),
                Event::JoyHatMotion { state, .. } if state != HatState::Centered => {
                    return Cal::Hat
                }
                _ => {}
            }
        }
        match rx.try_recv() {
            Ok(s) if s == "q" => return Cal::Quit,
            Ok(_) => return Cal::Skip,
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {}
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn run_calibrate(cfg_path: &Path) -> Result<(), String> {
    let sdl = init_sdl()?;
    let _joy = open_first(&sdl)?;
    let mut cfg = config::load(cfg_path)?;
    let mut pump = sdl.event_pump()?;
    let rx = spawn_stdin_reader();

    // Buttons rebuilt fresh; axes preserved and updated (mirrors Python).
    let mut buttons = serde_json::Map::new();
    let mut axes = serde_json::Map::new();
    if let Some(obj) = cfg["profile"]["axes"].as_object() {
        for (k, v) in obj {
            if k != "_comment" {
                axes.insert(k.clone(), v.clone());
            }
        }
    }

    println!("\nCALIBRATE - press each control when prompted.");
    println!("Press Enter (in this terminal) to skip a control; type q + Enter to stop.\n");
    for name in CALIBRATION_ORDER {
        print!("  press [{name}] (Enter=skip, q=quit): ");
        std::io::stdout().flush().ok();
        match await_input(&mut pump, &rx) {
            Cal::Button(i) => {
                buttons.insert((*name).to_string(), Value::from(i));
                println!("button {i}");
            }
            Cal::Axis(i) => {
                axes.insert((*name).to_string(), Value::from(i));
                println!("axis {i}");
            }
            Cal::Hat => println!("hat - D-pad is a hat, handled automatically (skipping)"),
            Cal::Skip => println!("skipped"),
            Cal::Quit => {
                println!("(stopped early)");
                break;
            }
        }
    }

    cfg["profile"]["buttons"] = Value::Object(buttons);
    cfg["profile"]["axes"] = Value::Object(axes);
    config::save(cfg_path, &cfg)?;
    println!(
        "\nSaved profile to {}. Set `bindings`, then --monitor.",
        cfg_path.display()
    );
    Ok(())
}
