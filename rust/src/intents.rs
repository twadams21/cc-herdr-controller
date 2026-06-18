//! Map herdr *actions* (bindings) to wire *intents* the Mac relay understands,
//! plus the analog-scroll acceleration curve (ported from the Python daemon).

use base64::Engine;

/// Lines/tick from an analog deflection, with acceleration. Pure; mirrors
/// `scroll_lines` in the Python daemon.
pub fn scroll_lines(value: f32, deadzone: f32, max_lines: i32) -> i32 {
    let a = value.abs();
    if a < deadzone {
        return 0;
    }
    let t = ((a - deadzone) / (1.0 - deadzone)).clamp(0.0, 1.0);
    std::cmp::max(1, (t * max_lines as f32).round() as i32)
}

/// Intent for the 1:1 actions (navigation, zoom, enter/esc). Returns `None` for
/// actions handled specially in the loop (scroll/voice/dictation/noop).
pub fn simple_intent(action: &str) -> Option<&'static str> {
    Some(match action {
        "tab_next" => "tab_next",
        "tab_prev" => "tab_prev",
        "workspace_next" => "workspace_next",
        "workspace_prev" => "workspace_prev",
        "pane_left" => "pane left",
        "pane_right" => "pane right",
        "pane_up" => "pane up",
        "pane_down" => "pane down",
        "pane_zoom" => "zoom",
        "enter" => "keys enter",
        "escape" => "keys esc",
        _ => return None,
    })
}

/// `scroll <signed>` — positive = up (older content).
pub fn scroll_intent(lines: i32) -> String {
    format!("scroll {lines}")
}

/// `keys <name>` — e.g. an arrow direction.
pub fn keys_intent(name: &str) -> String {
    format!("keys {name}")
}

/// `text <base64>` — base64 so spaces/newlines survive the line protocol.
pub fn text_intent(s: &str) -> String {
    format!("text {}", base64::engine::general_purpose::STANDARD.encode(s.as_bytes()))
}
