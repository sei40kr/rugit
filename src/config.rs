//! User configuration: `$XDG_CONFIG_HOME/rugit/config.toml`.
//!
//! ```toml
//! scrolloff = 3
//!
//! [keys.global]
//! "g"   = "refresh"
//! "P p" = "push"      # space-separated key sequences are supported
//!
//! [keys.status]
//! "s" = "stage"
//!
//! [keys.rebase-todo]
//! "x" = "todo-drop"
//!
//! [colors]            # role names: see src/theme.rs
//! diff-add  = "green"
//! cursor-bg = "#3a3a3a"
//! key       = "42"    # 256-color index
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

use crate::command;
use crate::keymap::{parse_keys, Keymaps, PaneKind};

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    pub scrolloff: Option<usize>,
    #[serde(default)]
    pub keys: KeysConfig,
    #[serde(default)]
    pub colors: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct KeysConfig {
    #[serde(default)]
    pub global: HashMap<String, String>,
    #[serde(default)]
    pub status: HashMap<String, String>,
    #[serde(default, rename = "rebase-todo")]
    pub rebase_todo: HashMap<String, String>,
}

pub fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("rugit").join("config.toml"))
}

/// Load the config file if present. Parse failures are reported as warnings
/// rather than aborting startup.
pub fn load() -> (Config, Vec<String>) {
    let mut warnings = Vec::new();
    let Some(path) = config_path() else {
        return (Config::default(), warnings);
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return (Config::default(), warnings);
    };
    match toml::from_str(&text) {
        Ok(cfg) => (cfg, warnings),
        Err(e) => {
            warnings.push(format!("config error in {}: {e}", path.display()));
            (Config::default(), warnings)
        }
    }
}

/// Merge user bindings over the defaults. Unknown commands or bad key specs
/// become warnings.
pub fn apply_keys(cfg: &Config, keymaps: &mut Keymaps, warnings: &mut Vec<String>) {
    let mut apply = |bindings: &HashMap<String, String>, map: &mut crate::keymap::Keymap| {
        for (spec, cmd_name) in bindings {
            let Some(cmd) = command::by_name(cmd_name) else {
                warnings.push(format!("config: unknown command {cmd_name:?}"));
                continue;
            };
            match parse_keys(spec) {
                Ok(seq) => map.insert(&seq, cmd),
                Err(e) => warnings.push(format!("config: {e}")),
            }
        }
    };
    apply(&cfg.keys.global, &mut keymaps.global);
    apply(
        &cfg.keys.status,
        keymaps.local.entry(PaneKind::Status).or_default(),
    );
    apply(
        &cfg.keys.rebase_todo,
        keymaps.local.entry(PaneKind::RebaseTodo).or_default(),
    );
}

/// Override theme roles from `[colors]`. Bad keys/values become warnings.
pub fn apply_colors(cfg: &Config, theme: &mut crate::theme::Theme, warnings: &mut Vec<String>) {
    for (key, value) in &cfg.colors {
        if let Err(e) = theme.set(key, value) {
            warnings.push(e);
        }
    }
}
