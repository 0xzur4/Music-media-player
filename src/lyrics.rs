use crate::library::Track;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

/// Satu baris lirik: waktu (detik) -> teks.
#[derive(Clone, Debug)]
pub struct LyricLine {
    pub time: f64,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    /// true jika lirik punya timestamp sinkron (LRC), false jika lirik biasa.
    pub synced: bool,
}

impl Lyrics {
    /// Index baris yang sedang aktif pada posisi `pos` detik.
    pub fn active_index(&self, pos: f64) -> Option<usize> {
        if !self.synced || self.lines.is_empty() {
            return None;
        }
        let mut idx = None;
        for (i, line) in self.lines.iter().enumerate() {
            if line.time <= pos + 0.05 {
                idx = Some(i);
            } else {
                break;
            }
        }
        idx
    }
}

fn parse_timestamp(s: &str) -> Option<f64> {
    // format mm:ss.xx atau mm:ss
    let (m, rest) = s.split_once(':')?;
    let minutes: f64 = m.trim().parse().ok()?;
    let seconds: f64 = rest.trim().parse().ok()?;
    Some(minutes * 60.0 + seconds)
}

/// Parse teks LRC. Mendukung banyak timestamp dalam satu baris: [00:12.00][00:15.00]teks
pub fn parse_lrc(text: &str) -> Lyrics {
    let mut lines: Vec<LyricLine> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        // Kumpulkan semua [..] di awal baris
        let mut rest = line;
        let mut times: Vec<f64> = Vec::new();
        loop {
            if !rest.starts_with('[') {
                break;
            }
            let end = match rest.find(']') {
                Some(i) => i,
                None => break,
            };
            let tag = &rest[1..end];
            // Tag metadata seperti [ar:..], [ti:..], [length:..] dilewati
            if let Some(t) = parse_timestamp(tag) {
                times.push(t);
            }
            rest = rest[end + 1..].trim_start();
        }
        if times.is_empty() {
            continue;
        }
        // Baris kosong (musik instrumental) tetap disimpan agar timing terjaga
        let text = rest.to_string();
        for t in times {
            lines.push(LyricLine { time: t, text: text.clone() });
        }
    }
    lines.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    let synced = !lines.is_empty();
    Lyrics { lines, synced }
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibResponse {
    #[serde(default)]
    synced_lyrics: Option<String>,
    #[serde(default)]
    plain_lyrics: Option<String>,
}

fn cache_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("id", "joni", "JoniMusic").map(|d| d.cache_dir().join("lyrics"))
}

fn cache_path(track: &Track) -> Option<PathBuf> {
    cache_dir().map(|d| d.join(format!("{}.lrc", track.cache_key())))
}

/// Ambil lirik: cache dulu, kalau tidak ada coba unduh dari lrclib.net.
/// Mengembalikan (Lyrics, dari_cache).
pub fn get_lyrics(track: &Track) -> (Option<Lyrics>, bool) {
    if let Some(p) = cache_path(track) {
        if let Ok(text) = fs::read_to_string(&p) {
            let lyrics = parse_lrc(&text);
            if lyrics.synced {
                return (Some(lyrics), true);
            }
        }
    }

    match fetch_online(track) {
        Some(text) => {
            if let Some(p) = cache_path(track) {
                if let Some(parent) = p.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::write(&p, &text);
            }
            let lyrics = parse_lrc(&text);
            if lyrics.synced {
                (Some(lyrics), false)
            } else {
                // Simpan lirik biasa sebagai fallback tampilan
                (
                    Some(Lyrics {
                        lines: text
                            .lines()
                            .map(|l| LyricLine {
                                time: 0.0,
                                text: l.trim().to_string(),
                            })
                            .filter(|l| !l.text.is_empty())
                            .collect(),
                        synced: false,
                    }),
                    false,
                )
            }
        }
        None => (None, false),
    }
}

fn fetch_online(track: &Track) -> Option<String> {
    let mut url = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}",
        url_encode(&track.artist),
        url_encode(&track.title)
    );
    if !track.album.is_empty() {
        url.push_str(&format!("&album_name={}", url_encode(&track.album)));
    }
    if track.duration_secs > 0.0 {
        url.push_str(&format!("&duration={}", track.duration_secs.round() as u64));
    }

    let config = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(12)))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let resp: LrclibResponse = agent
        .get(&url)
        .header("User-Agent", "JoniMusic/1.0")
        .call()
        .ok()?
        .into_body()
        .read_json::<LrclibResponse>()
        .ok()?;
    if let Some(s) = resp.synced_lyrics {
        if !s.trim().is_empty() {
            return Some(s);
        }
    }
    if let Some(p) = resp.plain_lyrics {
        if !p.trim().is_empty() {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_lrc() {
        let lrc = "[ar:Artist]\n[ti:Title]\n[00:06.22] Hello, it's me\n[00:11.84][00:15.00] second line\n\n[01:00] third\n";
        let lyrics = parse_lrc(lrc);
        assert!(lyrics.synced);
        assert_eq!(lyrics.lines.len(), 4);
        assert!((lyrics.lines[0].time - 6.22).abs() < 0.01);
        assert_eq!(lyrics.lines[0].text, "Hello, it's me");
        // dua timestamp -> dua baris
        assert!((lyrics.lines[1].time - 11.84).abs() < 0.01);
        assert!((lyrics.lines[2].time - 15.00).abs() < 0.01);
        assert_eq!(lyrics.lines[1].text, lyrics.lines[2].text);
    }

    #[test]
    fn active_line_tracking() {
        let lrc = "[00:05.00] one\n[00:10.00] two\n[00:20.00] three\n";
        let lyrics = parse_lrc(lrc);
        assert_eq!(lyrics.active_index(0.0), None);
        assert_eq!(lyrics.active_index(5.0), Some(0));
        assert_eq!(lyrics.active_index(12.0), Some(1));
        assert_eq!(lyrics.active_index(99.0), Some(2));
    }

    #[test]
    fn plain_text_is_not_synced() {
        let lyrics = parse_lrc("just some\nplain lyrics\n");
        assert!(!lyrics.synced);
        assert!(lyrics.lines.is_empty());
    }
}
