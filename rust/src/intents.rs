//! Map herdr *actions* (bindings) to wire *intents* the Mac relay understands,
//! plus the analog-scroll acceleration curve (ported from the Python daemon).

use base64::Engine;

/// Lines/tick from an analog deflection, with a clear zone + two-stage
/// acceleration. Pure.
///
/// - `|value| <= deadzone`: clear zone, returns 0.
/// - just past the deadzone: a *soft* start (rounds to 0), so **stick drift**
///   sitting near the deadzone never scrolls. This is the key difference from a
///   naive `max(1, …)` floor, which scrolled forever the instant drift crept
///   past the deadzone.
/// - lower ~75% of travel: speed ramps slowly.
/// - top ~25%: speed ramps quickly to `max_lines`.
pub fn scroll_lines(value: f32, deadzone: f32, max_lines: i32) -> i32 {
    let a = value.abs();
    if a <= deadzone {
        return 0;
    }
    let t = ((a - deadzone) / (1.0 - deadzone)).clamp(0.0, 1.0);
    let speed = if t < 0.75 {
        (t / 0.75) * 0.35
    } else {
        0.35 + ((t - 0.75) / 0.25) * 0.65
    };
    (speed * max_lines as f32).round() as i32
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
        "arrow_up" => "keys up",
        "arrow_down" => "keys down",
        "arrow_left" => "keys left",
        "arrow_right" => "keys right",
        _ => return None,
    })
}

/// `scroll <signed>` — positive = up (older content).
pub fn scroll_intent(lines: i32) -> String {
    format!("scroll {lines}")
}

/// `hscroll <signed>` — positive = right.
pub fn hscroll_intent(lines: i32) -> String {
    format!("hscroll {lines}")
}

/// `keys <name>` — e.g. an arrow direction.
pub fn keys_intent(name: &str) -> String {
    format!("keys {name}")
}

/// `text <base64>` — base64 so spaces/newlines survive the line protocol.
pub fn text_intent(s: &str) -> String {
    format!(
        "text {}",
        base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_clear_zone_and_drift_are_silent() {
        // Inside the deadzone, and drift sitting just past it, must not scroll.
        assert_eq!(scroll_lines(0.0, 0.2, 6), 0);
        assert_eq!(scroll_lines(0.18, 0.2, 6), 0);
        assert_eq!(scroll_lines(0.21, 0.2, 6), 0); // drift just past deadzone
        assert_eq!(scroll_lines(0.30, 0.2, 6), 0); // typical drift magnitude
    }

    #[test]
    fn scroll_ramps_slow_then_fast() {
        let mid = scroll_lines(0.5, 0.2, 6); // slow region
        let near = scroll_lines(0.8, 0.2, 6); // end of slow region
        let full = scroll_lines(1.0, 0.2, 6); // top of fast region
        assert!(mid >= 1 && mid <= near, "mid={mid} near={near}");
        assert!(full > near, "full={full} near={near}");
        assert_eq!(full, 6); // reaches max at full deflection
    }

    #[test]
    fn arrow_actions_map_to_key_intents() {
        assert_eq!(simple_intent("arrow_up"), Some("keys up"));
        assert_eq!(simple_intent("arrow_down"), Some("keys down"));
        assert_eq!(simple_intent("arrow_left"), Some("keys left"));
        assert_eq!(simple_intent("arrow_right"), Some("keys right"));
    }
}
