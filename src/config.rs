//! Fișierul de configurare al aplicației: `$XDG_CONFIG_HOME/sxr/config`,
//! implicit `~/.config/sxr/config`.
//!
//! Format deliberat minimal — `cheie=valoare`, câte una pe linie, cu `#` pentru
//! comentarii — ca să nu avem nevoie de nicio dependență de TOML sau JSON.
//! Modulul nu știe nimic despre ce chei există: e un simplu magazin de perechi,
//! gândit ca meniul de setări care urmează să-și adauge propriile chei fără să
//! atingă nimic de aici.
//!
//! Nimic din acest modul nu intră vreodată în panică: un fișier lipsă, ilizibil
//! sau stricat înseamnă pur și simplu „nicio valoare", deci apelantul rămâne cu
//! implicitele lui.

use std::path::PathBuf;

/// Directorul personal, cu rezervă la directorul temporar dacă `HOME` lipsește.
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// `$XDG_CONFIG_HOME/sxr`, implicit `~/.config/sxr`.
pub fn dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".config"))
        .join("sxr")
}

/// Calea completă a fișierului de configurare.
pub fn path() -> PathBuf {
    dir().join("config")
}

/// Perechile citite din fișier, în ordinea în care apar acolo. Ordinea contează
/// doar ca fișierul rescris să semene cu cel citit; căutarea e liniară, dar
/// setările se numără pe degete.
#[derive(Default)]
pub struct Config {
    items: Vec<(String, String)>,
}

impl Config {
    /// Citește fișierul. Orice eroare (lipsă, drepturi, octeți invalizi) dă o
    /// configurare goală, deci apelantul primește implicitele.
    pub fn load() -> Self {
        let Ok(raw) = std::fs::read(path()) else { return Self::default() };
        let text = String::from_utf8_lossy(&raw);
        let mut items = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            // liniile goale și comentariile nu spun nimic; o linie fără `=` e
            // stricată și o sărim, fără să ratăm restul fișierului
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

    /// Schimbă valoarea unei chei, sau o adaugă la sfârșit dacă lipsea.
    pub fn set(&mut self, key: &str, val: &str) {
        match self.items.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = val.to_owned(),
            None => self.items.push((key.to_owned(), val.to_owned())),
        }
    }

    /// Scrie fișierul, creând directorul dacă lipsește.
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

/// Valoarea unei chei, citită direct din fișier.
pub fn get(key: &str) -> Option<String> {
    Config::load().get(key).map(str::to_owned)
}

/// Scrie o cheie, păstrând restul fișierului. `false` dacă scrierea a eșuat —
/// apelantul decide dacă are ce spune despre asta; aplicația merge mai departe.
pub fn set(key: &str, val: &str) -> bool {
    let mut c = Config::load();
    c.set(key, val);
    c.save().is_ok()
}
