//! Opus for the voice channel: mono 48 kHz in 20 ms frames, as
//! /docs/PROTOCOL.md specifies.

use std::collections::HashMap;

use anyhow::Context;
use hearth_protocol::MAX_VOICE_FRAME;

pub const SAMPLE_RATE: u32 = 48_000;
/// 20 ms at 48 kHz.
pub const FRAME: usize = 960;

pub struct Encoder {
    opus: opus::Encoder,
    /// Captured samples that have not filled a frame yet.
    pending: Vec<f32>,
}

impl Encoder {
    pub fn new() -> anyhow::Result<Self> {
        let mut opus = opus::Encoder::new(SAMPLE_RATE, opus::Channels::Mono, opus::Application::Voip)
            .context("creating the Opus encoder")?;
        // Voice at 24 kbit/s, and silence costs almost nothing.
        let _ = opus.set_bitrate(opus::Bitrate::Bits(24_000));
        Ok(Self { opus, pending: Vec::with_capacity(FRAME * 2) })
    }

    /// Takes any number of samples and returns one encoded frame per complete
    /// 20 ms worth of audio.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Vec<u8>> {
        self.pending.extend_from_slice(samples);
        let mut frames = Vec::new();
        let mut buf = [0u8; MAX_VOICE_FRAME];
        while self.pending.len() >= FRAME {
            match self.opus.encode_float(&self.pending[..FRAME], &mut buf) {
                Ok(len) if len > 0 => frames.push(buf[..len].to_vec()),
                _ => {}
            }
            self.pending.drain(..FRAME);
        }
        // A backlog means the caller stopped sending; never grow without bound.
        if self.pending.len() > FRAME * 4 {
            self.pending.clear();
        }
        frames
    }
}

/// One decoder per speaker: Opus is stateful, so frames from different players
/// must not share one.
#[derive(Default)]
pub struct Decoders(HashMap<u16, opus::Decoder>);

impl Decoders {
    pub fn decode(&mut self, from: u16, frame: &[u8]) -> Option<Vec<f32>> {
        let decoder = match self.0.entry(from) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(opus::Decoder::new(SAMPLE_RATE, opus::Channels::Mono).ok()?)
            }
        };
        let mut pcm = vec![0.0; FRAME * 6]; // Room for the largest Opus frame size.
        let samples = decoder.decode_float(frame, &mut pcm, false).ok()?;
        pcm.truncate(samples);
        Some(pcm)
    }

    pub fn forget(&mut self, id: u16) {
        self.0.remove(&id);
    }

    /// Changing instance replaces every speaker at once.
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tone_survives_the_round_trip() {
        let tone: Vec<f32> = (0..FRAME * 3)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / SAMPLE_RATE as f32).sin() * 0.5)
            .collect();
        let mut encoder = Encoder::new().unwrap();
        let frames = encoder.push(&tone);
        assert_eq!(frames.len(), 3, "three 20 ms frames out of 60 ms of audio");
        assert!(frames.iter().all(|frame| !frame.is_empty() && frame.len() <= MAX_VOICE_FRAME));

        let mut decoders = Decoders::default();
        let decoded: Vec<f32> = frames.iter().flat_map(|frame| decoders.decode(7, frame).unwrap()).collect();
        assert_eq!(decoded.len(), tone.len());
        // Opus needs a frame or two to warm up, so judge the tail: it must be
        // audible, not silence or noise blown up.
        let peak = decoded[FRAME * 2..].iter().fold(0.0f32, |max, s| max.max(s.abs()));
        assert!((0.1..1.0).contains(&peak), "decoded peak {peak}");
    }

    #[test]
    fn partial_frames_wait_and_garbage_is_dropped() {
        let mut encoder = Encoder::new().unwrap();
        assert!(encoder.push(&vec![0.0; FRAME - 1]).is_empty());
        assert_eq!(encoder.push(&[0.0]).len(), 1);
        assert!(Decoders::default().decode(1, &[0xFF; 32]).is_none_or(|pcm| !pcm.is_empty()));
    }
}
