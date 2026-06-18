//! The run loop: read controller events/state, map to herdr intents, and push
//! them through the transport. Ports the Python daemon's timing behaviour.

use crate::transport::Transport;
use crate::{config, controller, intents};
use sdl2::event::Event;
use sdl2::joystick::HatState;
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const SCROLL_BUTTON_NOTCH: i32 = 3;

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

    let sc = &settings["scroll"];
    let scroll_axis_name = sc["axis"].as_str().unwrap_or("right_y").to_string();
    let scroll_axis = axis_index.get(&scroll_axis_name).copied();
    let scroll_invert = sc["invert"].as_bool().unwrap_or(false);
    let scroll_dead = sc["deadzone"].as_f64().unwrap_or(0.18) as f32;
    let scroll_tick = Duration::from_millis(sc["tick_ms"].as_u64().unwrap_or(30));
    let scroll_max = sc["max_lines"].as_i64().unwrap_or(6) as i32;

    let arrows_on = settings
        .get("arrows")
        .map(|v| v.is_object())
        .unwrap_or(false);
    let ar = &settings["arrows"];
    let arrows_ax = if arrows_on {
        axis_index
            .get(ar["axis_x"].as_str().unwrap_or("left_x"))
            .copied()
    } else {
        None
    };
    let arrows_ay = if arrows_on {
        axis_index
            .get(ar["axis_y"].as_str().unwrap_or("left_y"))
            .copied()
    } else {
        None
    };
    let arrows_dead = ar["deadzone"].as_f64().unwrap_or(0.5) as f32;
    let arrows_repeat = Duration::from_millis(ar["repeat_ms"].as_u64().unwrap_or(150));

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

    // Triggers: binding names that resolve to an axis (e.g. ZL/ZR), not the scroll axis.
    let mut trigger_down: HashMap<String, bool> = HashMap::new();
    let triggers: Vec<(String, u32)> = bindings
        .as_object()
        .map(|b| {
            b.keys()
                .filter(|n| *n != "_comment" && **n != scroll_axis_name)
                .filter_map(|n| axis_index.get(n).map(|i| (n.clone(), *i)))
                .collect()
        })
        .unwrap_or_default();
    for (n, _) in &triggers {
        trigger_down.insert(n.clone(), false);
    }

    let stale = Instant::now() - Duration::from_secs(1);
    let mut v_pressing = false;
    let mut v_latched = false;
    let mut v_down = Instant::now();
    let mut v_last = stale;
    let mut scroll_last = stale;
    let mut arrows_last_dir: Option<&'static str> = None;
    let mut arrows_last_fire = stale;
    let mut seen: u64 = 0;
    let mut last_beat = Instant::now();

    let mode = if perform {
        "driving herdr"
    } else {
        "OBSERVING ONLY - not performing"
    };
    println!("\n{label} - {mode}. Ctrl-C to quit.\n");

    loop {
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
                Event::JoyButtonUp { button_idx, .. } => {
                    if Some(button_idx as u32) == voice_idx && v_pressing {
                        v_pressing = false;
                        let held = now.duration_since(v_down);
                        v_latched = match voice_mode.as_str() {
                            "hold" => false,
                            "toggle" => !v_latched,
                            _ => held <= voice_tap_max && !v_latched,
                        };
                    }
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

        // Analog scroll with acceleration.
        if let Some(sa) = scroll_axis {
            if sa < num_axes {
                let val = controller::norm(joy.axis(sa).unwrap_or(0));
                let lines = intents::scroll_lines(val, scroll_dead, scroll_max);
                if lines > 0 && now.duration_since(scroll_last) >= scroll_tick {
                    scroll_last = now;
                    let up = val < 0.0; // stick up = scroll up (older content)
                    let mut delta = if up { lines } else { -lines };
                    if scroll_invert {
                        delta = -delta;
                    }
                    if perform {
                        transport.send(&intents::scroll_intent(delta));
                    } else {
                        println!(
                            "scroll {} {}  [skipped]",
                            if delta > 0 { "up" } else { "down" },
                            delta.abs()
                        );
                    }
                }
            }
        }

        // Left stick -> arrow keys (4-way dominant axis, repeats while held).
        if let (Some(ax), Some(ay)) = (arrows_ax, arrows_ay) {
            if ax < num_axes && ay < num_axes {
                let vx = controller::norm(joy.axis(ax).unwrap_or(0));
                let vy = controller::norm(joy.axis(ay).unwrap_or(0));
                if vx.abs().max(vy.abs()) < arrows_dead {
                    arrows_last_dir = None;
                } else {
                    let direction = if vx.abs() >= vy.abs() {
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
                    if Some(direction) != arrows_last_dir
                        || now.duration_since(arrows_last_fire) >= arrows_repeat
                    {
                        arrows_last_fire = now;
                        arrows_last_dir = Some(direction);
                        if perform {
                            transport.send(&intents::keys_intent(direction));
                        } else {
                            println!("arrow {direction}  [skipped]");
                        }
                    }
                }
            }
        }

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
