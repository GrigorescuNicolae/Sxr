//! The application's config file: `$XDG_CONFIG_HOME/sxr/config`, by default
//! `~/.config/sxr/config`.
//!
//! Deliberately minimal format — `key=value`, one per line, with `#` for
//! comments — so that we need no TOML or JSON dependency at all. The module
//! knows nothing about which keys exist: it is a plain store of pairs, meant so
//! that the settings menu still to come can add its own keys without touching
//! anything in here.
//!
//! Nothing in this module ever panics: a missing, unreadable or broken file
//! simply means "no value", so the caller is left with its own defaults.

use std::path::PathBuf;

/// The home directory, falling back to the temporary one if `HOME` is missing.
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// `$XDG_CONFIG_HOME/sxr`, by default `~/.config/sxr`.
pub fn dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".config"))
        .join("sxr")
}

/// The full path of the config file.
pub fn path() -> PathBuf {
    dir().join("config")
}

/// The pairs read from the file, in the order they appear there. The order only
/// matters so the rewritten file resembles the one we read; the lookup is
/// linear, but the settings can be counted on one hand.
#[derive(Default)]
pub struct Config {
    items: Vec<(String, String)>,
}

impl Config {
    /// Reads the file. Any error (missing, permissions, invalid bytes) yields an
    /// empty config, so the caller gets the defaults.
    pub fn load() -> Self {
        let Ok(raw) = std::fs::read(path()) else { return Self::default() };
        let text = String::from_utf8_lossy(&raw);
        let mut items = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            // blank lines and comments say nothing; a line without `=` is
            // broken and we skip it, without losing the rest of the file
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            if k.is_empty() {
                continue;
            }
            items.retain(|(ek, _): &(String, String)| ek != k);
            items.push((k.to_owned(), v.to_owned()));
        }
        Self { items }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.items.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// Changes a key's value, or appends it at the end if it was missing.
    pub fn set(&mut self, key: &str, val: &str) {
        match self.items.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = val.to_owned(),
            None => self.items.push((key.to_owned(), val.to_owned())),
        }
    }

    /// Writes the file, creating the directory if it is missing.
    pub fn save(&self) -> std::io::Result<()> {
        let mut out = String::from("# sxr\n");
        for (k, v) in &self.items {
            out.push_str(k);
            out.push('=');
            out.push_str(v);
            out.push('\n');
        }
        std::fs::create_dir_all(dir())?;
        std::fs::write(path(), out)
    }
}

/// A key's value, read straight from the file.
pub fn get(key: &str) -> Option<String> {
    Config::load().get(key).map(str::to_owned)
}

/// Writes a key, keeping the rest of the file. `false` if the write failed —
/// the caller decides whether that is worth saying; the app carries on either way.
pub fn set(key: &str, val: &str) -> bool {
    let mut c = Config::load();
    c.set(key, val);
    c.save().is_ok()
}
