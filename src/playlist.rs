use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Satu playlist: nama + daftar path file audio.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Playlist {
    pub name: String,
    pub paths: Vec<PathBuf>,
}

impl Playlist {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            paths: Vec::new(),
        }
    }

    /// Tambah lagu bila belum ada.
    pub fn add(&mut self, path: &Path) -> bool {
        if self.paths.iter().any(|p| p == path) {
            false
        } else {
            self.paths.push(path.to_path_buf());
            true
        }
    }

    pub fn remove(&mut self, path: &Path) {
        self.paths.retain(|p| p != path);
    }
}

fn playlists_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("id", "joni", "JoniMusic")
        .map(|d| d.config_dir().join("playlists.json"))
}

/// Muat semua playlist dari disk.
pub fn load() -> Vec<Playlist> {
    let p = match playlists_path() {
        Some(p) => p,
        None => return Vec::new(),
    };
    let text = match std::fs::read_to_string(p) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Simpan semua playlist ke disk.
pub fn save(playlists: &[Playlist]) {
    if let Some(p) = playlists_path() {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(playlists) {
            let _ = std::fs::write(p, text);
        }
    }
}
