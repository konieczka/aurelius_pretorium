//! Claude Code's colours in our terminals. Unless its theme is "auto" it never looks at the terminal: a fresh install
//! is "dark", so in the light theme its diffs and dimmed text came out near-white on cream. "auto" asks the terminal's
//! background (OSC 11) and follows our light/dark reports (mode 2031). We set it only while it was never chosen (no
//! `theme` saved by /theme, none in the user's settings), where Claude Code's own /theme keeps it.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::agent_config::{edit_json, read, write_checked, Edit};

/// Claude Code's global config: `.claude.json` in CLAUDE_CONFIG_DIR, else in the home folder.
fn config() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default())).join(".claude.json")
}

pub fn follow() {
    let _ = follow_in(&config(), &crate::workdir::claude_dir().join("settings.json"));
}

/// Set the theme to "auto" in `config` if neither it nor `settings` has one. No config (not installed, or never run):
/// nothing. Left alone if either doesn't parse: Claude's to report.
fn follow_in(config: &Path, settings: &Path) -> crate::Res<bool> {
    let text = read(config)?;
    if text.trim().is_empty() {
        return Ok(false);
    }
    let theme = |t: &str| -> crate::Res<bool> { Ok(!t.trim().is_empty() && !serde_json::from_str::<Value>(t).map_err(crate::err)?["theme"].is_null()) };
    if theme(&text)? || theme(&read(settings)?)? {
        return Ok(false);
    }
    let edit = Edit { key: vec!["theme".into()], value: Some(json!("auto")) };
    write_checked(config, &text, &edit_json(&text, &[edit])?)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_only_where_never_chosen() {
        let dir = std::env::temp_dir().join(format!("pretorium-claude-theme-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (config, settings) = (dir.join(".claude.json"), dir.join("settings.json"));
        let get = || serde_json::from_str::<Value>(&std::fs::read_to_string(&config).unwrap()).unwrap();

        assert!(!follow_in(&config, &settings).unwrap(), "not installed: nothing");
        assert!(!config.exists());

        std::fs::write(&config, "{\n  \"numStartups\": 3\n}\n").unwrap();
        assert!(follow_in(&config, &settings).unwrap());
        assert_eq!(get()["theme"], "auto");
        assert_eq!(get()["numStartups"], 3);
        assert!(!follow_in(&config, &settings).unwrap(), "already set");

        std::fs::write(&config, r#"{"theme": "dark"}"#).unwrap();
        assert!(!follow_in(&config, &settings).unwrap(), "chosen with /theme");
        assert_eq!(get()["theme"], "dark");

        std::fs::write(&config, "{}").unwrap();
        std::fs::write(&settings, r#"{"theme": "light"}"#).unwrap();
        assert!(!follow_in(&config, &settings).unwrap(), "chosen in settings");
        assert!(get()["theme"].is_null());

        std::fs::write(&settings, "{ oops").unwrap();
        assert!(follow_in(&config, &settings).is_err(), "settings not ours to fix");
        std::fs::write(&config, "{ oops").unwrap();
        assert!(follow_in(&config, &dir.join("none.json")).is_err());
        assert_eq!(std::fs::read_to_string(&config).unwrap(), "{ oops");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
