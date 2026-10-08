use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::Accessor;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const AUDIO_EXTS: &[&str] = &["mp3", "flac", "ogg", "oga", "wav", "m4a", "aac", "opus", "wma"];

#[derive(Clone, Debug)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: f64,
}

impl Track {
    pub fn display(&self) -> String {
        if self.artist.is_empty() {
            self.title.clone()
        } else {
            format!("{} - {}", self.artist, self.title)
        }
    }

    /// Key unik untuk cache lirik.
    pub fn cache_key(&self) -> String {
        sanitize(&format!("{} - {}", self.artist, self.title))
    }
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Ambil artist/title dari nama file "Artist - Title.ext".
fn from_filename(path: &Path) -> (String, String) {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown");
    if let Some((a, t)) = stem.split_once(" - ") {
        (a.trim().to_string(), t.trim().to_string())
    } else if let Some((a, t)) = stem.split_once('-') {
        (a.trim().to_string(), t.trim().to_string())
    } else {
        (String::new(), stem.trim().to_string())
    }
}

fn read_track(path: &Path) -> Option<Track> {
    let (mut artist, mut title) = (String::new(), String::new());
    let mut album = String::new();
    let mut duration_secs = 0.0;

    if let Ok(tagged) = lofty::read_from_path(path) {
        if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
            if let Some(t) = tag.title().as_deref() {
                title = t.to_string();
            }
            if let Some(a) = tag.artist().as_deref() {
                artist = a.to_string();
            }
            if let Some(a) = tag.album().as_deref() {
                album = a.to_string();
            }
        }
        duration_secs = tagged.properties().duration().as_secs_f64();
    }

    if title.is_empty() {
        let (a, t) = from_filename(path);
        if artist.is_empty() {
            artist = a;
        }
        title = t;
    }
    if title.is_empty() {
        title = "Unknown Title".to_string();
    }

    Some(Track {
        path: path.to_path_buf(),
        title,
        artist,
        album,
        duration_secs,
    })
}

pub fn scan(dir: &Path) -> Vec<Track> {
    let mut tracks: Vec<Track> = WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| AUDIO_EXTS.contains(&x.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .filter_map(|e| read_track(e.path()))
        .collect();
    tracks.sort_by(|a, b| a.display().to_lowercase().cmp(&b.display().to_lowercase()));
    tracks
}
