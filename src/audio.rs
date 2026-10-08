use crate::library::Track;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player as RodioPlayer};
use std::fs::File;
use std::io::BufReader;
use std::time::Duration;

/// Wrapper rodio. Posisi putar dibaca dari `get_pos()` rodio
/// sehingga lirik selalu tersinkron.
pub struct Player {
    _sink: MixerDeviceSink,
    inner: RodioPlayer,
    playing: bool,
    duration: f64,
    volume: f32,
}

impl Player {
    pub fn new() -> anyhow::Result<Self> {
        let sink = DeviceSinkBuilder::open_default_sink()?;
        let inner = RodioPlayer::connect_new(sink.mixer());
        inner.set_volume(0.8);
        Ok(Self {
            _sink: sink,
            inner,
            playing: false,
            duration: 0.0,
            volume: 0.8,
        })
    }

    pub fn play(&mut self, track: &Track) -> anyhow::Result<()> {
        self.inner.clear();
        let file = BufReader::new(File::open(&track.path)?);
        let source = Decoder::new(file)?;
        self.inner.append(source);
        self.inner.play();
        self.duration = track.duration_secs;
        self.playing = true;
        Ok(())
    }

    pub fn stop(&mut self) {
        self.inner.clear();
        self.inner.stop();
        self.playing = false;
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
