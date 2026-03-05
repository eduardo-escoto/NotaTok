use crate::{CoreError, Result};
use serde::{Deserialize, Serialize};

/// A single REMI token variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemiToken {
    /// Marks the start of a new bar.
    Bar,
    /// Grid position within the current bar (0-indexed).
    Position(u8),
    /// Absolute MIDI pitch value within the configured pitch range.
    Pitch(u8),
    /// Binned velocity index (0..velocity_bins−1).
    Velocity(u8),
    /// Duration in position units (1..=max_duration).
    Duration(u8),
    /// Binned BPM index (0..tempo_bins−1). Only used when `use_tempo_tokens` is enabled.
    Tempo(u8),
}

/// Flat vocabulary that maps [`RemiToken`] variants to contiguous integer IDs.
///
/// ID layout (given default config):
/// ```text
/// 0          → Bar
/// 1 – 16     → Position(0) – Position(15)
/// 17 – 144   → Pitch(0)    – Pitch(127)
/// 145 – 176  → Velocity(0) – Velocity(31)
/// 177 – 240  → Duration(1) – Duration(64)
///              (no Tempo tokens in default config)
/// ```
#[derive(Debug, Clone)]
pub struct Vocabulary {
    pub bar_id: u32,
    pub position_base: u32,
    pub pitch_base: u32,
    pub velocity_base: u32,
    pub duration_base: u32,
    pub tempo_base: u32,

    pub positions_per_bar: u8,
    pub n_pitches: usize,
    pub pitch_offset: u8,
    pub velocity_bins: u8,
    pub max_duration: u8,
    pub tempo_bins: u8,
    pub use_tempo: bool,

    total: usize,
}

impl Vocabulary {
    pub fn new(
        positions_per_bar: u8,
        pitch_min: u8,
        pitch_max: u8,
        velocity_bins: u8,
        max_duration: u8,
        tempo_bins: u8,
        use_tempo: bool,
    ) -> Self {
        let n_pitches = (pitch_max as usize).saturating_sub(pitch_min as usize) + 1;
        let bar_id = 0u32;
        let position_base = 1u32;
        let pitch_base = position_base + positions_per_bar as u32;
        let velocity_base = pitch_base + n_pitches as u32;
        let duration_base = velocity_base + velocity_bins as u32;
        let tempo_base = duration_base + max_duration as u32;
        let total =
            tempo_base as usize + if use_tempo { tempo_bins as usize } else { 0 };

        Self {
            bar_id,
            position_base,
            pitch_base,
            velocity_base,
            duration_base,
            tempo_base,
            positions_per_bar,
            n_pitches,
            pitch_offset: pitch_min,
            velocity_bins,
            max_duration,
            tempo_bins,
            use_tempo,
            total,
        }
    }

    pub fn size(&self) -> usize {
        self.total
    }

    pub fn token_to_id(&self, token: &RemiToken) -> Result<u32> {
        match token {
            RemiToken::Bar => Ok(self.bar_id),

            RemiToken::Position(p) => {
                if *p >= self.positions_per_bar {
                    return Err(CoreError::Tokenizer(format!(
                        "Position({p}) out of range (max {})",
                        self.positions_per_bar - 1
                    )));
                }
                Ok(self.position_base + *p as u32)
            }

            RemiToken::Pitch(p) => {
                let offset = p.checked_sub(self.pitch_offset).filter(|&o| {
                    (o as usize) < self.n_pitches
                });
                match offset {
                    Some(o) => Ok(self.pitch_base + o as u32),
                    None => Err(CoreError::Tokenizer(format!(
                        "Pitch({p}) out of range ({} – {})",
                        self.pitch_offset,
                        self.pitch_offset + self.n_pitches as u8 - 1
                    ))),
                }
            }

            RemiToken::Velocity(v) => {
                if *v >= self.velocity_bins {
                    return Err(CoreError::Tokenizer(format!(
                        "Velocity bin {v} out of range (max {})",
                        self.velocity_bins - 1
                    )));
                }
                Ok(self.velocity_base + *v as u32)
            }

            RemiToken::Duration(d) => {
                if *d == 0 || *d > self.max_duration {
                    return Err(CoreError::Tokenizer(format!(
                        "Duration({d}) out of range (1 – {})",
                        self.max_duration
                    )));
                }
                Ok(self.duration_base + (*d - 1) as u32)
            }

            RemiToken::Tempo(t) => {
                if !self.use_tempo || *t >= self.tempo_bins {
                    return Err(CoreError::Tokenizer(format!(
                        "Tempo({t}) invalid (use_tempo={}, bins={})",
                        self.use_tempo, self.tempo_bins
                    )));
                }
                Ok(self.tempo_base + *t as u32)
            }
        }
    }

    pub fn id_to_token(&self, id: u32) -> Result<RemiToken> {
        if id == self.bar_id {
            return Ok(RemiToken::Bar);
        }
        if id >= self.position_base && id < self.pitch_base {
            return Ok(RemiToken::Position((id - self.position_base) as u8));
        }
        if id >= self.pitch_base && id < self.velocity_base {
            return Ok(RemiToken::Pitch(
                self.pitch_offset + (id - self.pitch_base) as u8,
            ));
        }
        if id >= self.velocity_base && id < self.duration_base {
            return Ok(RemiToken::Velocity((id - self.velocity_base) as u8));
        }
        if id >= self.duration_base && id < self.tempo_base {
            return Ok(RemiToken::Duration((id - self.duration_base) as u8 + 1));
        }
        if self.use_tempo
            && id >= self.tempo_base
            && id < self.tempo_base + self.tempo_bins as u32
        {
            return Ok(RemiToken::Tempo((id - self.tempo_base) as u8));
        }
        Err(CoreError::UnknownTokenId(id))
    }

    /// Map a raw MIDI velocity (0–127) to a bin index (0..velocity_bins−1).
    pub fn bin_velocity(&self, vel: u8) -> u8 {
        ((vel as u16 * self.velocity_bins as u16) / 128) as u8
    }

    /// Reconstruct a representative raw velocity from a bin index.
    pub fn unbin_velocity(&self, bin: u8) -> u8 {
        ((bin as u16 * 128 + 64) / self.velocity_bins as u16).min(127) as u8
    }

    /// Map a BPM value to a tempo bin index.
    pub fn bin_tempo(&self, bpm: f64, min_bpm: f64, max_bpm: f64) -> u8 {
        let ratio = (bpm - min_bpm) / (max_bpm - min_bpm);
        let bin = (ratio * self.tempo_bins as f64).floor() as i32;
        bin.clamp(0, self.tempo_bins as i32 - 1) as u8
    }

    /// Reconstruct a representative BPM from a tempo bin index.
    pub fn unbin_tempo(&self, bin: u8, min_bpm: f64, max_bpm: f64) -> f64 {
        let step = (max_bpm - min_bpm) / self.tempo_bins as f64;
        min_bpm + step * (bin as f64 + 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_vocab() -> Vocabulary {
        // positions_per_bar=16, pitch 0-127, 32 vel bins, max_dur=64, no tempo
        Vocabulary::new(16, 0, 127, 32, 64, 32, false)
    }

    #[test]
    fn vocab_size_is_241_for_defaults() {
        // 1 (Bar) + 16 (Position) + 128 (Pitch) + 32 (Velocity) + 64 (Duration) = 241
        assert_eq!(default_vocab().size(), 241);
    }

    #[test]
    fn bar_is_id_zero() {
        assert_eq!(default_vocab().token_to_id(&RemiToken::Bar).unwrap(), 0);
    }

    #[test]
    fn round_trip_all_ids() {
        let v = default_vocab();
        for id in 0..v.size() as u32 {
            let token = v.id_to_token(id).unwrap();
            assert_eq!(
                v.token_to_id(&token).unwrap(),
                id,
                "round-trip failed for id {id}"
            );
        }
    }

    #[test]
    fn velocity_binning_is_monotone() {
        let v = default_vocab();
        let bins: Vec<u8> = (0u8..=127).map(|vel| v.bin_velocity(vel)).collect();
        for w in bins.windows(2) {
            assert!(w[1] >= w[0], "binning is not monotone at {:?}", w);
        }
    }

    #[test]
    fn out_of_range_pitch_is_error() {
        let v = Vocabulary::new(16, 60, 72, 32, 64, 32, false);
        assert!(v.token_to_id(&RemiToken::Pitch(59)).is_err());
        assert!(v.token_to_id(&RemiToken::Pitch(73)).is_err());
        assert!(v.token_to_id(&RemiToken::Pitch(60)).is_ok());
        assert!(v.token_to_id(&RemiToken::Pitch(72)).is_ok());
    }

    #[test]
    fn unknown_id_is_error() {
        let v = default_vocab();
        assert!(v.id_to_token(v.size() as u32).is_err());
        assert!(v.id_to_token(u32::MAX).is_err());
    }

    #[test]
    fn tempo_tokens_add_to_size() {
        let with_tempo = Vocabulary::new(16, 0, 127, 32, 64, 32, true);
        assert_eq!(with_tempo.size(), 241 + 32);
    }
}
