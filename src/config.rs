//! Pengaturan aplikasi yang disimpan ke `config.json`.
//!
//! Semua field memakai `#[serde(default)]` sehingga file config lama
//! (hanya `music_dir`, `audio_device`, `auto_mini`) tetap terbaca.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn cycle(self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RepeatMode::Off => "Ulangi: mati",
            RepeatMode::All => "Ulangi: semua",
            RepeatMode::One => "Ulangi: satu lagu",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum SortKey {
    #[default]
    Default,
    Title,
    Artist,
    Album,
    Duration,
}

impl SortKey {
    pub const ALL: [SortKey; 5] = [
        SortKey::Default,
        SortKey::Title,
        SortKey::Artist,
        SortKey::Album,
        SortKey::Duration,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SortKey::Default => "Urutan asli",
            SortKey::Title => "Judul",
            SortKey::Artist => "Artis",
            SortKey::Album => "Album",
            SortKey::Duration => "Durasi",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum SearchScope {
    #[default]
    All,
    Title,
    Artist,
    Album,
}

impl SearchScope {
    pub const ALL: [SearchScope; 4] = [
        SearchScope::All,
        SearchScope::Title,
        SearchScope::Artist,
        SearchScope::Album,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SearchScope::All => "Semua",
            SearchScope::Title => "Judul",
            SearchScope::Artist => "Artis",
            SearchScope::Album => "Album",
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
    pub music_dir: Option<PathBuf>,
    pub audio_device: Option<String>,
    pub auto_mini: bool,
    /// Volume 0..1.
    pub volume: f32,
    pub muted: bool,
    pub eq_gains: [f32; 5],
    pub eq_preset: String,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub sidebar_width: f32,
    /// Lagu terakhir + posisinya, untuk dilanjutkan saat aplikasi dibuka lagi.
    pub last_track: Option<PathBuf>,
    pub last_pos: f64,
    pub view_lyrics: bool,
    pub sort_key: SortKey,
    pub sort_asc: bool,
    pub search_scope: SearchScope,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            music_dir: None,
            audio_device: None,
            auto_mini: true,
            volume: 0.8,
            muted: false,
            eq_gains: [0.0; 5],
            eq_preset: String::from("Flat"),
            shuffle: false,
            repeat: RepeatMode::Off,
            sidebar_width: 300.0,
            last_track: None,
            last_pos: 0.0,
            view_lyrics: false,
            sort_key: SortKey::Default,
            sort_asc: true,
            search_scope: SearchScope::All,
        }
    }
}

fn config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("id", "joni", "JoniMusic")
        .map(|d| d.config_dir().join("config.json"))
}

pub fn load() -> Settings {
    let Some(p) = config_path() else {
        return Settings::default();
    };
    let Ok(text) = std::fs::read_to_string(p) else {
        return Settings::default();
    };
    let mut s: Settings = serde_json::from_str(&text).unwrap_or_default();
    s.volume = if s.volume.is_finite() { s.volume.clamp(0.0, 1.0) } else { 0.8 };
    for g in s.eq_gains.iter_mut() {
        *g = if g.is_finite() { g.clamp(-12.0, 12.0) } else { 0.0 };
    }
    if !s.last_pos.is_finite() || s.last_pos < 0.0 {
        s.last_pos = 0.0;
    }
    s
}

/// Tulis atomik (file sementara lalu rename) agar config tidak korup
/// bila aplikasi ditutup paksa saat menulis.
pub fn save(s: &Settings) {
    let Some(p) = config_path() else { return };
    write_atomic(&p, &serde_json::to_string_pretty(s).unwrap_or_default());
}

pub fn write_atomic(path: &std::path::Path, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::write(path, text);
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_format_still_loads() {
        let old = r#"{"music_dir":"C:\\Music","audio_device":null,"auto_mini":false}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(s.music_dir, Some(PathBuf::from("C:\\Music")));
        assert!(!s.auto_mini);
        assert!((s.volume - 0.8).abs() < 1e-6);
        assert_eq!(s.repeat, RepeatMode::Off);
    }

    #[test]
    fn roundtrip() {
        let mut s = Settings::default();
        s.repeat = RepeatMode::One;
        s.sort_key = SortKey::Album;
        let text = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(back.repeat, RepeatMode::One);
        assert_eq!(back.sort_key, SortKey::Album);
    }
}
