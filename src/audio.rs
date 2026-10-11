use crate::library::Track;
use rodio::{
    cpal,
    cpal::traits::HostTrait,
    Decoder, DeviceSinkBuilder, DeviceTrait, MixerDeviceSink, Player as RodioPlayer,
};
use std::fs::File;
use std::io::BufReader;
use std::time::Duration;

/// Label untuk entri "pakai default sistem" di daftar perangkat audio.
pub const DEFAULT_DEVICE_LABEL: &str = "Default Sistem";

fn names_match(device_name: &str, wanted: &str) -> bool {
    let a = device_name.trim();
    let b = wanted.trim();
    a == b || a.contains(b) || b.contains(a)
}

/// Buka sink + player rodio untuk perangkat tertentu.
/// `label = None` berarti pakai perangkat default sistem.
fn open_for(label: Option<&str>) -> anyhow::Result<(MixerDeviceSink, RodioPlayer)> {
    let sink = match label {
        None => DeviceSinkBuilder::open_default_sink()?,
        Some(name) => {
            let host = cpal::default_host();
            let mut found = None;
            for d in host.output_devices()? {
                if let Ok(n) = d.name() {
                    if names_match(&n, name) {
                        found = Some(d);
                        break;
                    }
                }
            }
            let device = found
                .ok_or_else(|| anyhow::anyhow!("perangkat audio '{name}' tidak ditemukan"))?;
            DeviceSinkBuilder::from_device(device)?.open_sink_or_fallback()?
        }
    };
    let inner = RodioPlayer::connect_new(sink.mixer());
    Ok((sink, inner))
}

/// Daftar nama perangkat output audio yang tersedia (tanpa duplikat).
/// Dipanggil saat UI butuh (mis. popup daftar dibuka), bukan tiap frame.
pub fn output_device_names() -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(devices) = cpal::default_host().output_devices() {
        for d in devices {
            if let Ok(n) = d.name() {
                let n = n.trim().to_string();
                if !n.is_empty() && !names.contains(&n) {
                    names.push(n);
                }
            }
        }
    }
    names
}

/// Wrapper rodio. Posisi putar dibaca dari `get_pos()` rodio
/// sehingga lirik selalu tersinkron.
pub struct Player {
    _sink: MixerDeviceSink,
    inner: RodioPlayer,
    playing: bool,
    duration: f64,
    volume: f32,
    current_track: Option<Track>,
    device_label: Option<String>,
}

impl Player {
    pub fn new() -> anyhow::Result<Self> {
        Self::with_device(None)
    }

    pub fn with_device(label: Option<&str>) -> anyhow::Result<Self> {
        let (sink, inner) = open_for(label)?;
        let p = Self {            _sink: sink,
            inner,
            playing: false,
            duration: 0.0,
            volume: 0.8,
            current_track: None,
            device_label: label.map(|s| s.to_string()),
        };
        p.inner.set_volume(0.8);
        Ok(p)
    }

    /// Label perangkat yang sedang dipakai; None = default sistem.
    pub fn device_label(&self) -> Option<&str> {
        self.device_label.as_deref()
    }

    /// Ganti perangkat output audio. Lagu yang sedang diputar dilanjutkan
    /// dari posisi yang sama di perangkat baru.
    pub fn set_output_device(&mut self, label: Option<&str>) -> anyhow::Result<()> {
        if self.device_label.as_deref() == label {
            return Ok(());
        }
        let was_playing = self.playing;
        let pos = self.position();
        let track = self.current_track.clone();
        let (sink, inner) = open_for(label)?;
        self._sink = sink;
        self.inner = inner;
        self.inner.set_volume(self.volume);
        self.device_label = label.map(|s| s.to_string());
        if let Some(t) = track {
            self.play_raw(&t, pos, was_playing)?;
        } else {
            self.playing = false;
        }
        Ok(())
    }

    fn play_raw(&mut self, track: &Track, from_pos: f64, autoplay: bool) -> anyhow::Result<()> {
        self.inner.clear();
        let file = BufReader::new(File::open(&track.path)?);
        let source = Decoder::new(file)?;
        self.inner.append(source);
        self.duration = track.duration_secs;
        self.current_track = Some(track.clone());
        if from_pos > 0.5 {
            let _ = self
                .inner
                .try_seek(Duration::from_secs_f64(from_pos.clamp(0.0, self.duration)));
        }
        if autoplay {
            self.inner.play();
            self.playing = true;
        } else {
            self.inner.pause();
            self.playing = false;
        }
        Ok(())
    }

    pub fn play(&mut self, track: &Track) -> anyhow::Result<()> {
        self.play_raw(track, 0.0, true)
    }

    pub fn stop(&mut self) {
        self.inner.clear();
        self.inner.stop();
        self.playing = false;
        self.current_track = None;
    }

    pub fn toggle(&mut self) {
        if self.playing {
            self.pause();
        } else {
            self.resume();
        }
    }

    pub fn pause(&mut self) {
        self.inner.pause();
        self.playing = false;
    }

    pub fn resume(&mut self) {
        if !self.inner.empty() {
            self.inner.play();
            self.playing = true;
        }
    }

    /// Posisi putar dalam detik, langsung dari rodio.
    pub fn position(&self) -> f64 {
        self.inner.get_pos().as_secs_f64().min(self.duration)
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn is_playing(&self) -> bool {
        self.playing && !self.inner.is_paused()
    }

    pub fn has_track(&self) -> bool {
        !self.inner.empty()
    }

    /// true jika lagu selesai natural.
    pub fn finished(&self) -> bool {
        self.playing && self.inner.empty()
    }

    pub fn seek(&mut self, pos: f64) {
        let pos = pos.clamp(0.0, self.duration);
        let _ = self.inner.try_seek(Duration::from_secs_f64(pos));
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        self.inner.set_volume(self.volume);
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }
}
