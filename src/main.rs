//! gray-achieve — achievements unlocked from the live event stream.
//!
//! Port of the concept behind hermes' `hermes-achievements` plugin (the
//! dashboard, scanning and share-cards stay upstream): this sidecar watches
//! `event/notify` broadcasts, keeps a persistent tool-call counter, and
//! appends unlocks to `~/.gray/achievements.json`. Each unlock also fires
//! `host/say "🏆 <name>"` — the `host.say` capability needs consent, and the
//! plugin degrades silently without it (the unlock still lands on disk).
//!
//! `/achievements` lists unlocked (with timestamps) and still-locked badges.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};

/// Single-turn token count that earns `marathon`.
const MARATHON_TOKENS: u64 = 100_000;
/// Local-hour window (00:00–06:00) that earns `night_owl`.
const NIGHT_OWL_LAST_HOUR: u32 = 5;

static SAY_SEQ: AtomicU64 = AtomicU64::new(1);

struct Achievement {
    id: &'static str,
    name: &'static str,
    desc: &'static str,
}

const ACHIEVEMENTS: &[Achievement] = &[
    Achievement { id: "first_tool", name: "First Contact", desc: "run a tool call" },
    Achievement { id: "tool_10", name: "Toolchain Ten", desc: "10 tool calls" },
    Achievement { id: "tool_100", name: "Centurion", desc: "100 tool calls" },
    Achievement { id: "first_bash", name: "Shell Shocked", desc: "run a bash tool call" },
    Achievement { id: "first_write", name: "Pen to Paper", desc: "write or edit a file" },
    Achievement { id: "first_error_seen", name: "Red Text", desc: "see a tool call fail" },
    Achievement { id: "night_owl", name: "Night Owl", desc: "end a turn between 00:00 and 06:00 local" },
    Achievement { id: "marathon", name: "Marathon", desc: "a single turn over 100k tokens" },
];

fn manifest() -> Value {
    json!({
        "name": "achieve",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": "1.1",
        "tools": [],
        "commands": ["/achievements"],
        "capabilities": ["host.say"],
    })
}

fn gray_home() -> PathBuf {
    std::env::var_os("GRAY_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".gray")))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn state_dir() -> PathBuf {
    gray_home().join("achieve")
}

fn achievements_file() -> PathBuf {
    gray_home().join("achievements.json")
}

// ---------- state ----------

#[derive(Default)]
struct State {
    tool_calls: u64,
}

fn load_state(dir: &Path) -> State {
    let v: Value = std::fs::read_to_string(dir.join("state.json"))
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .unwrap_or(Value::Null);
    State {
        tool_calls: v.get("tool_calls").and_then(Value::as_u64).unwrap_or(0),
    }
}

fn save_state(dir: &Path, s: &State) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(
        dir.join("state.json"),
        json!({"tool_calls": s.tool_calls}).to_string(),
    );
}

fn load_unlocked(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
        .and_then(|v: Value| v.as_array().cloned())
        .unwrap_or_default()
}

fn save_unlocked(path: &Path, unlocked: &[Value]) {
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
    let _ = std::fs::write(path, json!(unlocked).to_string());
}

// ---------- conditions ----------

/// Local hour 0–23 via `date`; falls back to the UTC hour.
fn local_hour() -> u32 {
    if let Ok(o) = std::process::Command::new("date").arg("+%H").output()
        && o.status.success()
        && let Ok(h) = String::from_utf8_lossy(&o.stdout).trim().parse::<u32>()
    {
        return h;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    ((secs / 3600) % 24) as u32
}

/// Best-effort total token count for a `usage` object: `total`, then
/// `total_tokens`, then `input + output`.
fn usage_total(usage: &Value) -> u64 {
    for k in ["total", "total_tokens"] {
        if let Some(n) = usage.get(k).and_then(Value::as_u64) {
            return n;
        }
    }
    usage.get("input").and_then(Value::as_u64).unwrap_or(0)
        + usage.get("output").and_then(Value::as_u64).unwrap_or(0)
}

/// Evaluate one `event/notify` against the current state; returns the ids
/// that unlock *now* (caller filters already-unlocked before counting on
/// this for announcements).
fn eval_event(params: &Value, state: &mut State) -> Vec<&'static str> {
    let ty = params.get("type").and_then(Value::as_str).unwrap_or("");
    let mut out = vec![];
    match ty {
        "pre_tool" => {
            state.tool_calls += 1;
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            out.push("first_tool");
            if state.tool_calls >= 10 {
                out.push("tool_10");
            }
            if state.tool_calls >= 100 {
                out.push("tool_100");
            }
            if name == "bash" {
                out.push("first_bash");
            }
            if matches!(name, "write" | "edit" | "notebook_edit") {
                out.push("first_write");
            }
        }
        "post_tool" => {
            if params.get("is_error").and_then(Value::as_bool).unwrap_or(false) {
                out.push("first_error_seen");
            }
        }
        "turn_end" => {
            if local_hour() <= NIGHT_OWL_LAST_HOUR {
                out.push("night_owl");
            }
            if usage_total(params.get("usage").unwrap_or(&Value::Null)) > MARATHON_TOKENS {
                out.push("marathon");
            }
        }
        _ => {}
    }
    out
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `YYYY-MM-DD HH:MM:SS UTC` for display.
fn fmt_ts(ts: u64) -> String {
    let z = (ts / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (m, y) = if mp < 10 { (mp + 3, y) } else { (mp - 9, y + 1) };
    let d = doy - (153 * mp + 2) / 5 + 1;
    let t = ts % 86_400;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", t / 3600, t / 60 % 60, t % 60)
}

// ---------- wire ----------

/// Frames a `host/say` request (string id; the reply, if any, is ignored).
fn say_frame(text: &str) -> Value {
    let n = SAY_SEQ.fetch_add(1, Ordering::Relaxed);
    json!({"id": format!("say-{n}"), "method": "host/say", "params": {"text": text}})
}

/// Process one `event/notify`: update counters, append fresh unlocks to
/// `achievements.json`, and return a `host/say` frame per new unlock.
fn on_event(params: &Value, dir: &Path, unlocked_path: &Path) -> Vec<Value> {
    let mut state = load_state(dir);
    let mut unlocked = load_unlocked(unlocked_path);
    let have: Vec<String> = unlocked
        .iter()
        .filter_map(|u| u.get("id").and_then(Value::as_str).map(str::to_string))
        .collect();
    let fresh = eval_event(params, &mut state);
    save_state(dir, &state);
    let mut frames = vec![];
    for id in fresh {
        if have.iter().any(|h| h == id) {
            continue;
        }
        let Some(a) = ACHIEVEMENTS.iter().find(|a| a.id == id) else {
            continue;
        };
        unlocked.push(json!({"id": a.id, "name": a.name, "ts": now_ts()}));
        frames.push(say_frame(&format!("🏆 {}", a.name)));
    }
    if !frames.is_empty() {
        save_unlocked(unlocked_path, &unlocked);
    }
    frames
}

/// `/achievements` → unlocked list then locked list.
fn list(unlocked: &[Value]) -> String {
    let have: Vec<String> = unlocked
        .iter()
        .filter_map(|u| u.get("id").and_then(Value::as_str).map(str::to_string))
        .collect();
    let mut out = format!("unlocked ({}/{}):", have.len(), ACHIEVEMENTS.len());
    for a in ACHIEVEMENTS {
        if let Some(u) = unlocked
            .iter()
            .find(|u| u.get("id").and_then(Value::as_str) == Some(a.id))
        {
            let ts = u.get("ts").and_then(Value::as_u64).unwrap_or(0);
            out.push_str(&format!("\n  🏆 {} — {}", a.name, fmt_ts(ts)));
        }
    }
    let locked: Vec<&Achievement> = ACHIEVEMENTS
        .iter()
        .filter(|a| !have.iter().any(|h| h == a.id))
        .collect();
    if !locked.is_empty() {
        out.push_str("\nlocked:");
        for a in locked {
            out.push_str(&format!("\n  · {} — {}", a.name, a.desc));
        }
    }
    out
}

/// One request → frames to write (any `host/say` requests, then the reply).
/// The bool asks the loop to exit after writing.
fn handle(req: &Value, dir: &Path, unlocked_path: &Path) -> (Vec<Value>, bool) {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let Some(id) = id else {
        let frames = if method == "event/notify" {
            on_event(&params, dir, unlocked_path)
        } else {
            vec![]
        };
        // A notification carries no id → never reply, even on shutdown.
        return (frames, method == "plugin/shutdown");
    };
    if method.is_empty() {
        // Reply to one of our host/* requests — nothing to do.
        return (vec![], false);
    }
    let result = match method {
        "plugin/manifest" => manifest(),
        "command/run" => json!({ "text": list(&load_unlocked(unlocked_path)) }),
        "plugin/shutdown" => return (vec![json!({ "id": id, "result": {} })], true),
        _ => {
            let error = json!({ "code": -32601, "message": "method not found" });
            return (vec![json!({ "id": id, "error": error })], false);
        }
    };
    (vec![json!({ "id": id, "result": result })], false)
}

fn main() -> std::io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("manifest") {
        println!("{}", manifest());
        return Ok(());
    }
    let dir = state_dir();
    let unlocked_path = achievements_file();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        let Ok(req) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let (frames, exit) = handle(&req, &dir, &unlocked_path);
        for f in frames {
            writeln!(stdout, "{f}")?;
        }
        stdout.flush()?;
        if exit {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gray-ach-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn event(dir: &Path, upath: &Path, params: Value) -> Vec<Value> {
        handle(&json!({ "method": "event/notify", "params": params }), dir, upath).0
    }

    #[test]
    fn manifest_claims_host_say() {
        let m = manifest();
        assert_eq!(m["capabilities"], json!(["host.say"]));
        assert_eq!(m["commands"], json!(["/achievements"]));
    }

    #[test]
    fn first_tool_unlocks_once_and_says() {
        let dir = tmpdir("first");
        let upath = dir.join("achievements.json");
        let s = json!({"id":"s"});
        let f = event(&dir, &upath, json!({"type":"pre_tool","name":"bash","session":s}));
        let ids: Vec<&str> = f.iter()
            .filter_map(|v| v["params"]["text"].as_str())
            .collect();
        // pre_tool with bash → first_tool + first_bash in one shot
        assert_eq!(ids.len(), 2);
        assert!(f.iter().all(|v| v["method"] == "host/say"));
        // second event: first_tool already unlocked, but state counter moved
        let f = event(&dir, &upath, json!({"type":"pre_tool","name":"read","session":s}));
        assert!(f.is_empty());
        assert_eq!(load_state(&dir).tool_calls, 2);
        let unlocked = load_unlocked(&upath);
        assert_eq!(unlocked.len(), 2);
    }

    #[test]
    fn error_and_counters() {
        let dir = tmpdir("err");
        let upath = dir.join("achievements.json");
        let f = event(&dir, &upath, json!({"type":"post_tool","name":"x","is_error":true,"session":{}}));
        assert_eq!(f.len(), 1);
        let mut st = State::default();
        st.tool_calls = 9;
        let got = eval_event(&json!({"type":"pre_tool","name":"x"}), &mut st);
        assert!(got.contains(&"tool_10"));
        st.tool_calls = 99;
        let got = eval_event(&json!({"type":"pre_tool","name":"x"}), &mut st);
        assert!(got.contains(&"tool_100"));
    }

    #[test]
    fn marathon_threshold() {
        let mut st = State::default();
        assert!(eval_event(&json!({"type":"turn_end","usage":{"total":MARATHON_TOKENS + 1}}), &mut st)
            .contains(&"marathon"));
        let mut st = State::default();
        assert!(!eval_event(&json!({"type":"turn_end","usage":{"total":50}}), &mut st)
            .contains(&"marathon"));
        // input+output fallback
        let mut st = State::default();
        assert!(eval_event(&json!({"type":"turn_end","usage":{"input":90000,"output":20000}}), &mut st)
            .contains(&"marathon"));
    }

    #[test]
    fn list_shows_unlocked_and_locked() {
        let unlocked = vec![json!({"id":"first_tool","name":"First Contact","ts":0})];
        let text = list(&unlocked);
        assert!(text.contains("unlocked (1/8)"));
        assert!(text.contains("First Contact"));
        assert!(text.contains("· Marathon"));
    }

    #[test]
    fn host_replies_are_ignored() {
        let dir = tmpdir("ign");
        let upath = dir.join("a.json");
        let (frames, exit) = handle(
            &json!({"id": "say-1", "error": {"code": -32601, "message": "denied"}}),
            &dir,
            &upath,
        );
        assert!(frames.is_empty() && !exit);
    }
}
