use crate::library::Track;
use biquad::{Biquad, Coefficients, DirectForm1, ToHertz, Q_BUTTERWORTH_F32, Type};
use rodio::{
    cpal,
    cpal::traits::HostTrait,
    Decoder, DeviceSinkBuilder, DeviceTrait, MixerDeviceSink, Player as RodioPlayer, Source,
};
use std::fs::File;
use std::io::BufReader;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Frekuensi tengah 5 band equalizer (Hz).
pub const EQ_FREQS: [f32; 5] = [60.0, 230.0, 910.0, 3600.0, 14000.0];
/// Nama band untuk UI.
pub const EQ_NAMES: [&str; 5] = ["60", "230", "910", "3.6K", "14K"];

/// Gain per band dalam dB, shared antara UI dan audio thread.
pub type EqGains = Arc<RwLock<[f32; 5]>>;

/// Preset equalizer: nama -> gain 5 band (dB).
pub const EQ_PRESETS: &[(&str, [f32; 5])] = &[
    ("Flat", [0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Rock", [5.0, 3.0, -1.0, 3.0, 5.0]),
    ("Pop", [2.0, 3.0, 0.0, 2.0, 3.0]),
    ("Jazz", [3.0, 2.0, 0.0, 2.0, 4.0]),
    ("Klasik", [4.0, 2.0, -2.0, 2.0, 4.0]),
    ("Bass", [6.0, 4.0, 1.0, 0.0, 0.0]),
];

/// Wrapper Source rodio: 5 band peaking EQ (biquad) per channel.
/// Gain dibaca dari `EqGains` (shared), jadi slider UI berpengaruh
/// langsung ke lagu yang sedang diputar tanpa restart.
pub struct Equalizer<S> {
    input: S,
    filters: Vec<[DirectForm1<f32>; 5]>,
    gains: EqGains,
    last_gains: [f32; 5],
    ch_idx: usize,
    n_channels: usize,
}

impl<S> Equalizer<S>
where
    S: Source,
{
    pub fn new(input: S, gains: EqGains) -> Self {
        let sample_rate = input.sample_rate();
        let n_channels = input.channels().get() as usize;
        let g = *gains.read().unwrap_or_else(|e| e.into_inner());
        let filters = (0..n_channels)
            .map(|_| {
                EQ_FREQS.map(|f| {
                    let coeffs = Coefficients::<f32>::from_params(
                        Type::PeakingEQ(0.0),
                        sample_rate.get().hz(),
                        f.hz(),
                        Q_BUTTERWORTH_F32,
                    )
                    .unwrap();
                    DirectForm1::<f32>::new(coeffs)
                })
            })
            .collect();
        let mut eq = Self {
            input,
            filters,
            gains,
            last_gains: [f32::NAN; 5], // paksa update pertama
            ch_idx: 0,
            n_channels: n_channels.max(1),
        };
        eq.sync_gains(&g);
        eq
    }

    fn sync_gains(&mut self, g: &[f32; 5]) {
        let sr = self.input.sample_rate();
        for ch in self.filters.iter_mut() {
            for (band, filt) in ch.iter_mut().enumerate() {
                let coeffs = Coefficients::<f32>::from_params(
                    Type::PeakingEQ(g[band]),
                    sr.get().hz(),
                    EQ_FREQS[band].hz(),
                    Q_BUTTERWORTH_F32,
                )
                .unwrap();
                filt.update_coefficients(coeffs);
            }
        }
        self.last_gains = *g;
    }
}

impl<S> Iterator for Equalizer<S>
where
    S: Source,
{
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.input.next()?;
        // Sinkronkan gain bila berubah: baca dulu, lepas lock, baru update.
        let new_gains: Option<[f32; 5]> = self
            .gains
            .try_read()
            .ok()
            .filter(|g| **g != self.last_gains)
            .map(|g| *g);
        if let Some(gg) = new_gains {
            self.sync_gains(&gg);
        }
        let mut x = sample;
        let n = self.filters.len();
        let ch = &mut self.filters[self.ch_idx % n];
        for filt in ch.iter_mut() {
            x = filt.run(x);
        }
        // Soft clip agar boost besar tidak pecah.
        let x = x.clamp(-1.2, 1.2);
        self.ch_idx = (self.ch_idx + 1) % self.n_channels;
        Some(x)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.input.size_hint()
    }
}

impl<S> Source for Equalizer<S>
where
    S: Source,
{
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> std::num::NonZero<u16> {
        self.input.channels()
    }

    fn sample_rate(&self) -> std::num::NonZero<u32> {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }
}

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
    eq_gains: EqGains,
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
            eq_gains: Arc::new(RwLock::new([0.0; 5])),
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
        let source = Equalizer::new(source, self.eq_gains.clone());
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

    /// Handle gain EQ yang dishare dengan audio thread; UI menulis lewat ini.
    pub fn eq_gains(&self) -> EqGains {
        self.eq_gains.clone()
    }

    /// Atur gain satu band (dB, -12..+12). Berlaku langsung ke lagu berjalan.
    pub fn set_eq_gain(&self, band: usize, db: f32) {
        if band < 5 {
            if let Ok(mut g) = self.eq_gains.write() {
                g[band] = db.clamp(-12.0, 12.0);
            }
        }
    }

    /// Atur semua band sekaligus (preset).
    pub fn set_eq_all(&self, gains: [f32; 5]) {
        if let Ok(mut g) = self.eq_gains.write() {
            *g = gains;
        }
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
