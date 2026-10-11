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
/// Juga mendukung tag [offset:+500] / [offset:-200] (mili-detik) yang
/// menggeser semua timestamp.
pub fn parse_lrc(text: &str) -> Lyrics {
    let mut lines: Vec<LyricLine> = Vec::new();
    let mut offset_secs: f64 = 0.0;
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
            // Tag offset: [offset:+500] atau [offset:-200] dalam mili-detik
            if let Some(ms) = tag
                .strip_prefix("offset:")
                .and_then(|v| v.trim().parse::<f64>().ok())
            {
                offset_secs = ms / 1000.0;
            } else if let Some(t) = parse_timestamp(tag) {
                // Tag metadata seperti [ar:..], [ti:..], [length:..] dilewati
                // (parse_timestamp gagal untuk tag non-waktu)
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
    if offset_secs != 0.0 {
        for l in lines.iter_mut() {
            l.time = (l.time + offset_secs).max(0.0);
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibSearchItem {
    #[serde(default)]
    synced_lyrics: Option<String>,
    #[serde(default)]
    plain_lyrics: Option<String>,
}

fn user_agent() -> String {
    format!("JoniMusic/{}", env!("CARGO_PKG_VERSION"))
}

fn http_get(url: &str) -> Option<ureq::http::Response<ureq::Body>> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(12)))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    agent.get(url).header("User-Agent", &user_agent()).call().ok()
}

fn nonempty(opt: Option<String>) -> Option<String> {
    opt.filter(|s| !s.trim().is_empty())
}

/// Coba satu URL /api/get lrclib; kembalikan lirik (synced diutamakan).
fn query_get(url: &str) -> Option<String> {
    let mut resp = http_get(url)?;
    if resp.status() != 200 {
        return None;
    }
    let body: LrclibResponse = resp.body_mut().read_json().ok()?;
    nonempty(body.synced_lyrics).or_else(|| nonempty(body.plain_lyrics))
}

/// Fallback: /api/search dengan query bebas; ambil hasil pertama yang
/// punya lirik (synced diutamakan).
fn query_search(artist: &str, title: &str) -> Option<String> {
    let q = format!("{} {}", artist, title);
    if q.trim().is_empty() {
        return None;
    }
    let url = format!("https://lrclib.net/api/search?q={}", url_encode(&q));
    let mut resp = http_get(&url)?;
    if resp.status() != 200 {
        return None;
    }
    let items: Vec<LrclibSearchItem> = resp.body_mut().read_json().ok()?;
    // Utamakan hasil yang liriknya sinkron
    for it in &items {
        if let Some(s) = nonempty(it.synced_lyrics.clone()) {
            return Some(s);
        }
    }
    for it in &items {
        if let Some(p) = nonempty(it.plain_lyrics.clone()) {
            return Some(p);
        }
    }
    None
}

/// Sederhanakan nama artis/judul agar pencarian lebih longgar:
/// buang embel-embel seperti " (feat. X)", " [Live]", " - Remastered".
fn normalize_query(s: &str) -> String {
    let mut out = s.trim().to_string();
    // Potong di " (" atau " [" paling akhir (biasanya feat./live/versi).
    // Aman untuk judul CJK/Jepang/Korea karena butuh spasi sebelum kurung.
    for marker in [" (", " ["] {
        if let Some(i) = out.rfind(marker) {
            out.truncate(i);
        }
    }
    // Potong " - Remastered ..." (case-insensitive)
    let lower = out.to_lowercase();
    for marker in [" - remaster", " - deluxe", " - bonus"] {
        if let Some(i) = lower.rfind(marker) {
            out.truncate(i);
            break;
        }
    }
    out.trim().to_string()
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
    let artist = track.artist.trim();
    let title = track.title.trim();
    let album = track.album.trim();
    let n_artist = normalize_query(artist);
    let n_title = normalize_query(title);

    let mut urls: Vec<String> = Vec::new();
    // 1. Paling spesifik: artis + judul + album + durasi
    let mut u = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}",
        url_encode(artist),
        url_encode(title)
    );
    if !album.is_empty() {
        u.push_str(&format!("&album_name={}", url_encode(album)));
    }
    if track.duration_secs > 0.0 {
        u.push_str(&format!("&duration={}", track.duration_secs.round() as u64));
    }
    urls.push(u);
    // 2. Tanpa album (tag album sering beda dengan database)
    urls.push(format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}",
        url_encode(artist),
        url_encode(title)
    ));
    // 3. Hanya judul (untuk file dengan tag artis kosong/aneh)
    if !title.is_empty() {
        urls.push(format!(
            "https://lrclib.net/api/get?track_name={}",
            url_encode(title)
        ));
    }
    // 4. Versi yang dinormalisasi (tanpa feat./remastered/dsb)
    if n_artist != artist || n_title != title {
        urls.push(format!(
            "https://lrclib.net/api/get?artist_name={}&track_name={}",
            url_encode(&n_artist),
            url_encode(&n_title)
        ));
    }

    for url in &urls {
        if let Some(s) = query_get(url) {
            return Some(s);
        }
    }
    // 5. Fallback terakhir: pencarian fuzzy
    query_search(&n_artist, &n_title)
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

    #[test]
    fn offset_tag_shifts_timestamps() {
        let lrc = "[offset:+500]\n[00:10.00] hi\n";
        let lyrics = parse_lrc(lrc);
        assert!(lyrics.synced);
        assert!((lyrics.lines[0].time - 10.5).abs() < 0.01);
    }

    #[test]
    fn offset_tag_negative_clamped_at_zero() {
        let lrc = "[offset:-2000]\n[00:01.00] hi\n[00:10.00] ho\n";
        let lyrics = parse_lrc(lrc);
        assert!((lyrics.lines[0].time - 0.0).abs() < 0.01);
        assert!((lyrics.lines[1].time - 8.0).abs() < 0.01);
    }

    #[test]
    fn offset_tag_itself_not_a_lyric_line() {
        let lrc = "[offset:+300]\n[ar:Someone]\n[00:05.00] line\n";
        let lyrics = parse_lrc(lrc);
        assert_eq!(lyrics.lines.len(), 1);
        assert!((lyrics.lines[0].time - 5.3).abs() < 0.01);
    }

    #[test]
    fn normalize_strips_feat_and_remaster() {
        assert_eq!(normalize_query("Song (feat. Someone)"), "Song");
        assert_eq!(normalize_query("Song [Live]"), "Song");
        assert_eq!(normalize_query("Song - Remastered 2015"), "Song");
        assert_eq!(normalize_query("  Plain Title  "), "Plain Title");
    }

    #[test]
    fn normalize_keeps_cjk_titles() {
        // Judul CJK/Jepang/Korea tanpa spasi+kurung tidak boleh terpotong
        assert_eq!(normalize_query("夜に駆ける"), "夜に駆ける");
        assert_eq!(normalize_query("봄여름가을겨울"), "봄여름가을겨울");
        assert_eq!(normalize_query("晴天"), "晴天");
    }
}
