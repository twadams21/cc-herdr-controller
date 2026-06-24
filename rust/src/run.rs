//! The run loop: read controller events/state, map to herdr intents, and push
//! them through the transport. Ports the Python daemon's timing behaviour.

use crate::transport::Transport;
use crate::{config, controller, intents};
use sdl2::event::Event;
use sdl2::joystick::{HatState, Joystick};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const SCROLL_BUTTON_NOTCH: i32 = 3;

/// Raw stick axes — handled by the per-stick behaviour, never as on/off
/// triggers, so they're excluded from the trigger set.
const STICK_AXES: [&str; 4] = ["left_x", "left_y", "right_x", "right_y"];

/// Set by SIGINT/SIGTERM so the loop returns cleanly (dropping the transport,
/// which kills any SSH child). On non-unix we rely on default Ctrl-C handling.
static STOP: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
fn install_signal_handlers() {
    extern "C" fn handle(_sig: libc::c_int) {
        STOP.store(true, Ordering::SeqCst);
    }
    // SAFETY: the handler only stores into an AtomicBool (async-signal-safe).
    unsafe {
        libc::signal(libc::SIGINT, handle as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, handle as *const () as libc::sighandler_t);
    }
}

#[cfg(not(unix))]
fn install_signal_handlers() {}

fn run_dictation(cmd: &Option<String>) {
    if let Some(c) = cmd {
        let mut command = if cfg!(windows) {
            let mut x = std::process::Command::new("cmd");
            x.args(["/C", c]);
            x
        } else {
            let mut x = std::process::Command::new("sh");
            x.args(["-c", c]);
            x
        };
        let _ = command.spawn();
    }
}

/// Print a discrete action and, if performing, send its intent(s).
fn dispatch_action(
    action: &str,
    src: &str,
    perform: bool,
    scroll_invert: bool,
    dictation_cmd: &Option<String>,
    transport: &mut dyn Transport,
) {
    println!(
        "{src} -> {action}  [{}]",
        if perform { "performed" } else { "skipped" }
    );
    if !perform {
        return;
    }
    if let Some(intent) = intents::simple_intent(action) {
        transport.send(intent);
        return;
    }
    match action {
        "scroll_up" => {
            let d = if scroll_invert {
                -SCROLL_BUTTON_NOTCH
            } else {
                SCROLL_BUTTON_NOTCH
            };
            transport.send(&intents::scroll_intent(d));
        }
        "scroll_down" => {
            let d = if scroll_invert {
                SCROLL_BUTTON_NOTCH
            } else {
                -SCROLL_BUTTON_NOTCH
            };
            transport.send(&intents::scroll_intent(d));
        }
        "dictation" => run_dictation(dictation_cmd),
        "noop" | "voice" => {}
        other => eprintln!("  ! unknown action: {other}"),
    }
}

// ---- analog sticks ---------------------------------------------------------

/// What an analog stick does. `keys`/`panes` are 4-way; `scroll` is 2D.
#[derive(Clone, Copy, PartialEq)]
enum StickBehavior {
    Off,
    Keys,
    Panes,
    Scroll,
}

impl StickBehavior {
    fn parse(s: &str) -> StickBehavior {
        match s {
            "panes" => StickBehavior::Panes,
            "scroll" => StickBehavior::Scroll,
            "off" => StickBehavior::Off,
            _ => StickBehavior::Keys,
        }
    }
}

struct Stick {
    ax: Option<u32>,
    ay: Option<u32>,
    behavior: StickBehavior,
    deadzone: f32,
    repeat: Duration, // keys: repeat-while-held interval
    invert: bool,     // scroll: flip vertical direction
    tick: Duration,   // scroll: min interval between wheel bursts
    max_lines: i32,   // scroll: acceleration ceiling
    last_dir: Option<&'static str>,
    last_fire: Instant,
    last_scroll: Instant,
}

/// Build a stick from `settings.sticks.<side>`, falling back to the legacy
/// `arrows`/`scroll` block for params when the stick block is absent.
fn build_stick(
    s: &Value,
    x_name: &str,
    y_name: &str,
    axis_index: &HashMap<String, u32>,
    default_behavior: StickBehavior,
    legacy: &Value,
    stale: Instant,
) -> Stick {
    let behavior = s["behavior"]
        .as_str()
        .map(StickBehavior::parse)
        .unwrap_or(default_behavior);
    let num = |k: &str, d: f64| s[k].as_f64().or_else(|| legacy[k].as_f64()).unwrap_or(d);
    let ms = |k: &str, d: u64| s[k].as_u64().or_else(|| legacy[k].as_u64()).unwrap_or(d);
    let default_dead = if default_behavior == StickBehavior::Scroll {
        0.18
    } else {
        0.5
    };
    Stick {
        ax: axis_index.get(x_name).copied(),
        ay: axis_index.get(y_name).copied(),
        behavior,
        deadzone: num("deadzone", default_dead) as f32,
        repeat: Duration::from_millis(ms("repeat_ms", 150)),
        invert: s["invert"]
            .as_bool()
            .or_else(|| legacy["invert"].as_bool())
            .unwrap_or(false),
        tick: Duration::from_millis(ms("tick_ms", 30)),
        max_lines: s["max_lines"]
            .as_i64()
            .or_else(|| legacy["max_lines"].as_i64())
            .unwrap_or(6) as i32,
        last_dir: None,
        last_fire: stale,
        last_scroll: stale,
    }
}

/// Read a stick and emit its intent(s) for this tick. `keys`/`panes` fire on
/// direction change (keys also repeats while held); `scroll` sends vertical +
/// horizontal wheel bursts with acceleration.
fn handle_stick(
    st: &mut Stick,
    joy: &Joystick,
    num_axes: u32,
    now: Instant,
    perform: bool,
    transport: &mut dyn Transport,
) {
    if st.behavior == StickBehavior::Off {
        return;
    }
    let (ax, ay) = match (st.ax, st.ay) {
        (Some(x), Some(y)) if x < num_axes && y < num_axes => (x, y),
        _ => return,
    };
    let vx = controller::norm(joy.axis(ax).unwrap_or(0));
    let vy = controller::norm(joy.axis(ay).unwrap_or(0));

    match st.behavior {
        StickBehavior::Off => {}
        StickBehavior::Keys | StickBehavior::Panes => {
            if vx.abs().max(vy.abs()) < st.deadzone {
                st.last_dir = None;
                return;
            }
            let dir = if vx.abs() >= vy.abs() {
                if vx > 0.0 {
                    "right"
                } else {
                    "left"
                }
            } else if vy > 0.0 {
                "down" // SDL y+ = down
            } else {
                "up"
            };
            let repeats = st.behavior == StickBehavior::Keys;
            if Some(dir) != st.last_dir
                || (repeats && now.duration_since(st.last_fire) >= st.repeat)
            {
                st.last_fire = now;
                st.last_dir = Some(dir);
                let intent = if st.behavior == StickBehavior::Panes {
                    format!("pane {dir}")
                } else {
                    intents::keys_intent(dir)
                };
                if perform {
                    transport.send(&intent);
                } else {
                    println!("stick {dir} -> {intent}  [skipped]");
                }
            }
        }
        StickBehavior::Scroll => {
            if now.duration_since(st.last_scroll) < st.tick {
                return;
            }
            let v = intents::scroll_lines(vy, st.deadzone, st.max_lines);
            let h = intents::scroll_lines(vx, st.deadzone, st.max_lines);
            if v == 0 && h == 0 {
                return;
            }
            st.last_scroll = now;
            if v > 0 {
                let up = vy < 0.0; // stick up = scroll up (older content)
                let mut d = if up { v } else { -v };
                if st.invert {
                    d = -d;
                }
                if perform {
                    transport.send(&intents::scroll_intent(d));
                } else {
                    println!(
                        "scroll {} {}  [skipped]",
                        if d > 0 { "up" } else { "down" },
                        d.abs()
                    );
                }
            }
            if h > 0 {
                let d = if vx > 0.0 { h } else { -h };
                if perform {
                    transport.send(&intents::hscroll_intent(d));
                } else {
                    println!(
                        "hscroll {} {}  [skipped]",
                        if d > 0 { "right" } else { "left" },
                        d.abs()
                    );
                }
            }
        }
    }
}

pub fn run_loop(
    cfg: &Value,
    perform: bool,
    label: &str,
    transport: &mut dyn Transport,
) -> Result<(), String> {
    let sdl = controller::init_sdl()?;
    let joy = controller::open_first(&sdl)?;
    let mut pump = sdl.event_pump()?;
    let num_axes = joy.num_axes();

    let (btn_name, axis_index) = config::name_maps(cfg);
    let btn_index: HashMap<String, u32> = btn_name.iter().map(|(i, n)| (n.clone(), *i)).collect();

    let bindings = &cfg["bindings"];
    let settings = &cfg["settings"];

    let trigger_threshold = settings["trigger_threshold"].as_f64().unwrap_or(0.5) as f32;

    // Analog sticks: each has a behaviour (keys / panes / scroll / off). Falls
    // back to the legacy `arrows` (left) / `scroll` (right) blocks for params
    // when `settings.sticks` is absent.
    let stale = Instant::now() - Duration::from_secs(1);
    let sticks = &settings["sticks"];
    let mut left_stick = build_stick(
        &sticks["left"],
        "left_x",
        "left_y",
        &axis_index,
        StickBehavior::Keys,
        &settings["arrows"],
        stale,
    );
    let mut right_stick = build_stick(
        &sticks["right"],
        "right_x",
        "right_y",
        &axis_index,
        StickBehavior::Scroll,
        &settings["scroll"],
        stale,
    );
    // Button `scroll_up`/`scroll_down` actions follow the scrolling stick's invert.
    let scroll_invert = right_stick.invert;

    let vc = &settings["voice"];
    let voice_mode = vc["mode"].as_str().unwrap_or("both").to_string();
    let voice_tap_max = Duration::from_millis(vc["tap_max_ms"].as_u64().unwrap_or(300));
    let voice_repeat = Duration::from_millis(vc["repeat_ms"].as_u64().unwrap_or(90));
    let voice_char = vc["char"].as_str().unwrap_or(" ").to_string();
    let voice_idx = bindings
        .as_object()
        .and_then(|b| b.iter().find(|(_, a)| a.as_str() == Some("voice")))
        .and_then(|(n, _)| btn_index.get(n).copied());

    let dictation_cmd = settings["dictation_command"].as_str().map(str::to_string);

    // Triggers: binding names that resolve to an axis (e.g. ZL/ZR), excluding
    // the analog stick axes (those are handled by the per-stick behaviour).
    let mut trigger_down: HashMap<String, bool> = HashMap::new();
    let triggers: Vec<(String, u32)> = bindings
        .as_object()
        .map(|b| {
            b.keys()
                .filter(|n| *n != "_comment" && !STICK_AXES.contains(&n.as_str()))
                .filter_map(|n| axis_index.get(n).map(|i| (n.clone(), *i)))
                .collect()
        })
        .unwrap_or_default();
    for (n, _) in &triggers {
        trigger_down.insert(n.clone(), false);
    }

    let mut v_pressing = false;
    let mut v_latched = false;
    let mut v_down = Instant::now();
    let mut v_last = stale;
    let mut seen: u64 = 0;
    let mut last_beat = Instant::now();

    let mode = if perform {
        "driving the multiplexer — Ctrl-C to quit"
    } else {
        "OBSERVING ONLY, not performing — Ctrl-C to quit"
    };
    crate::ui::banner(label, mode);
    println!();
    install_signal_handlers();

    loop {
        if STOP.load(Ordering::Relaxed) {
            println!("\nbye");
            return Ok(());
        }
        let now = Instant::now();
        for ev in pump.poll_iter() {
            match ev {
                Event::JoyButtonDown { button_idx, .. } => {
                    seen += 1;
                    let bi = button_idx as u32;
                    if Some(bi) == voice_idx {
                        v_pressing = true;
                        v_down = now;
                        continue;
                    }
                    let name = btn_name
                        .get(&bi)
                        .cloned()
                        .unwrap_or_else(|| format!("button{bi}"));
                    match bindings.get(name.as_str()).and_then(|a| a.as_str()) {
                        Some(action) if action != "voice" => dispatch_action(
                            action,
                            &name,
                            perform,
                            scroll_invert,
                            &dictation_cmd,
                            transport,
                        ),
                        Some(_) => {}
                        None => println!("{name} (button {bi})  (unbound)"),
                    }
                }
                Event::JoyButtonUp { button_idx, .. }
                    if Some(button_idx as u32) == voice_idx && v_pressing =>
                {
                    v_pressing = false;
                    let held = now.duration_since(v_down);
                    v_latched = match voice_mode.as_str() {
                        "hold" => false,
                        "toggle" => !v_latched,
                        _ => held <= voice_tap_max && !v_latched,
                    };
                }
                Event::JoyHatMotion { state, .. } if state != HatState::Centered => {
                    seen += 1;
                    if let Some(d) = controller::hat_dir(state) {
                        let name = format!("dpad_{d}");
                        if let Some(action) = bindings.get(name.as_str()).and_then(|a| a.as_str()) {
                            dispatch_action(
                                action,
                                &name,
                                perform,
                                scroll_invert,
                                &dictation_cmd,
                                transport,
                            );
                        }
                    }
                }
                _ => {}
            }
        }

        // Triggers (ZL/ZR): rising edge past threshold.
        for (name, aidx) in &triggers {
            if *aidx >= num_axes {
                continue;
            }
            let pressed = controller::norm(joy.axis(*aidx).unwrap_or(0)) > trigger_threshold;
            if pressed && !trigger_down[name] {
                if let Some(action) = bindings.get(name.as_str()).and_then(|a| a.as_str()) {
                    dispatch_action(
                        action,
                        name,
                        perform,
                        scroll_invert,
                        &dictation_cmd,
                        transport,
                    );
                }
            }
            trigger_down.insert(name.clone(), pressed);
        }

        // Analog sticks (each: keys / panes / scroll / off).
        handle_stick(&mut left_stick, &joy, num_axes, now, perform, transport);
        handle_stick(&mut right_stick, &joy, num_axes, now, perform, transport);

        // Voice: while active, stream the configured char to emulate held-space.
        if (v_latched || v_pressing) && now.duration_since(v_last) >= voice_repeat {
            v_last = now;
            if perform {
                transport.send(&intents::text_intent(&voice_char));
            } else {
                println!("voice -> send {voice_char:?}  [skipped]");
            }
        }

        if now.duration_since(last_beat) >= Duration::from_secs(3) {
            last_beat = now;
            if seen == 0 {
                println!("... listening, 0 events. Controller not registering? Try --discover.");
            }
        }
        std::thread::sleep(Duration::from_millis(8));
    }
}
