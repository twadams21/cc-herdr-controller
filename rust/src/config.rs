//! Load/save mapping.json — the SAME file the Python daemon uses, so a profile
//! calibrated by either tool works in the other.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// Resolve the config path: an explicit `--config`, else the nearest
/// `mapping.json` walking up from the cwd, else next to the exe.
pub fn find_config(explicit: Option<&str>) -> PathBuf {
    if let Some(p) = explicit {
        return PathBuf::from(p);
    }
    if let Ok(mut dir) = std::env::current_dir() {
        loop {
            let candidate = dir.join("mapping.json");
            if candidate.is_file() {
                return candidate;
            }
            if !dir.pop() {
                break;
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("mapping.json");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from("mapping.json")
}

pub fn load(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("invalid JSON in {}: {e}", path.display()))
}

pub fn save(path: &Path, cfg: &Value) -> Result<(), String> {
    let mut text =
        serde_json::to_string_pretty(cfg).map_err(|e| format!("cannot serialize config: {e}"))?;
    text.push('\n');
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Follow a dotted path (e.g. `backend`, `bindings.A`, `settings.scroll.invert`)
/// and return the value if present.
pub fn get_path<'a>(cfg: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = cfg;
    for key in path.split('.') {
        cur = cur.get(key)?;
    }
    Some(cur)
}

/// Set a dotted path, creating intermediate objects as needed. The raw string
/// is coerced to bool / integer / float / else string. Returns the value set.
pub fn set_path(cfg: &mut Value, path: &str, raw: &str) -> Result<Value, String> {
    let keys: Vec<&str> = path.split('.').collect();
    if keys.iter().any(|k| k.is_empty()) {
        return Err(format!("invalid path: {path:?}"));
    }
    let value = coerce(raw);
    let mut cur = cfg;
    for key in &keys[..keys.len() - 1] {
        if !cur.is_object() {
            *cur = Value::Object(serde_json::Map::new());
        }
        cur = cur
            .as_object_mut()
            .unwrap()
            .entry((*key).to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
    }
    if !cur.is_object() {
        *cur = Value::Object(serde_json::Map::new());
    }
    let last = keys[keys.len() - 1].to_string();
    cur.as_object_mut().unwrap().insert(last, value.clone());
    Ok(value)
}

/// Coerce a CLI string into the most natural JSON scalar.
fn coerce(raw: &str) -> Value {
    match raw {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if let Ok(i) = raw.parse::<i64>() {
        return Value::from(i);
    }
    if let Ok(f) = raw.parse::<f64>() {
        return Value::from(f);
    }
    Value::String(raw.to_string())
}

/// `(button_index -> name, axis_name -> index)` from the `profile` block,
/// skipping any `_comment` keys.
pub fn name_maps(
    cfg: &Value,
) -> (
    std::collections::HashMap<u32, String>,
    std::collections::HashMap<String, u32>,
) {
    let mut btn_name = std::collections::HashMap::new();
    let mut axis_index = std::collections::HashMap::new();
    let profile = &cfg["profile"];
    if let Some(buttons) = profile.get("buttons").and_then(|v| v.as_object()) {
        for (name, idx) in buttons {
            if name == "_comment" {
                continue;
            }
            if let Some(i) = idx.as_u64() {
                btn_name.insert(i as u32, name.clone());
            }
        }
    }
    if let Some(axes) = profile.get("axes").and_then(|v| v.as_object()) {
        for (name, idx) in axes {
            if name == "_comment" {
                continue;
            }
            if let Some(i) = idx.as_u64() {
                axis_index.insert(name.clone(), i as u32);
            }
        }
    }
    (btn_name, axis_index)
}
