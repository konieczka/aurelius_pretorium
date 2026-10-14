//! Agents saying they ask something, where they can: Claude Code runs a Notification hook when a permission prompt
//! (or an MCP server's question) goes up. Ours touches a file named for the shell, which the status poll picks up;
//! outside a Pretorium shell (no PRETORIUM_ASKS) it does nothing.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::agent_config::{edit_json, read, write_checked, Edit};

const VAR: &str = "PRETORIUM_ASKS";
const COMMAND: &str = r#"[ -z "$PRETORIUM_ASKS" ] || : > "$PRETORIUM_ASKS""#;
const MATCHER: &str = "permission_prompt|elicitation_dialog"; // not idle_prompt: that's a finished turn, a minute on

fn dir() -> PathBuf {
    std::env::temp_dir().join("pretorium-asks")
}

/// The env var a shell gets, naming its file (tab ids are UUIDs).
pub fn env(id: &str) -> (&'static str, PathBuf) {
    let _ = std::fs::create_dir_all(dir());
    (VAR, dir().join(id))
}

/// When the shell's agent last said it asks, if it has since this was last taken.
pub fn take(id: &str) -> Option<SystemTime> {
    let path = dir().join(id);
    let at = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
    let _ = std::fs::remove_file(path);
    Some(at)
}

/// Add our hook to Claude Code's user settings unless it's there. Left alone if they don't parse: Claude's to report.
pub fn install() {
    let dir = crate::workdir::claude_dir();
    if dir.is_dir() {
        let _ = install_in(&dir.join("settings.json"));
    }
}

fn install_in(path: &Path) -> crate::Res<bool> {
    let text = read(path)?;
    let settings: Value = if text.trim().is_empty() { json!({}) } else { serde_json::from_str(&text).map_err(crate::err)? };
    let mut entries = match &settings["hooks"]["Notification"] {
        Value::Array(a) => a.clone(),
        Value::Null => Vec::new(),
        _ => return Ok(false),
    };
    let ours = |e: &Value| e["hooks"].as_array().is_some_and(|h| h.iter().any(|h| h["command"].as_str().is_some_and(|c| c.contains(VAR))));
    if entries.iter().any(ours) {
        return Ok(false);
    }
    entries.push(json!({ "matcher": MATCHER, "hooks": [{ "type": "command", "command": COMMAND }] }));
    let edit = Edit { key: vec!["hooks".into(), "Notification".into()], value: Some(Value::Array(entries)) };
    write_checked(path, &text, &edit_json(&text, &[edit])?)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_once_beside_other_hooks() {
        let dir = std::env::temp_dir().join(format!("pretorium-hooks-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let theirs = r#"{
  "model": "opus",
  "hooks": {
    "Notification": [{ "hooks": [{ "type": "command", "command": "say hi" }] }]
  }
}
"#;
        std::fs::write(&path, theirs).unwrap();
        assert!(install_in(&path).unwrap());
        assert!(!install_in(&path).unwrap(), "already there");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["model"], "opus");
        let hooks = v["hooks"]["Notification"].as_array().unwrap();
        assert_eq!(hooks.len(), 2);
        assert_eq!(hooks[0]["hooks"][0]["command"], "say hi");
        assert_eq!(hooks[1]["matcher"], MATCHER);
        // Not JSON: not ours to fix.
        std::fs::write(&path, "{ oops").unwrap();
        assert!(install_in(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ oops");
        std::fs::remove_file(&path).unwrap();
        assert!(install_in(&path).unwrap(), "no settings yet");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_hook_touches_the_shells_file() {
        let (_, path) = env("test-hook-shell");
        let _ = std::fs::remove_file(&path);
        let run = |var: Option<&Path>| {
            let mut c = std::process::Command::new("/bin/sh");
            c.args(["-c", COMMAND]).env_remove(VAR);
            if let Some(p) = var {
                c.env(VAR, p);
            }
            assert!(c.status().unwrap().success());
        };
        run(None);
        assert!(take("test-hook-shell").is_none(), "outside Pretorium: nothing");
        run(Some(&path));
        assert!(take("test-hook-shell").is_some());
        assert!(take("test-hook-shell").is_none(), "taken");
    }
}
