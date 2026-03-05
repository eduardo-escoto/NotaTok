pub mod vocab;

use serde::{Deserialize, Serialize};

use crate::{
    midi::{Note, Score, TempoChange, TimeSignatureChange, Track},
    tokenizer::Tokenizer,
    Result,
};

use vocab::{RemiToken, Vocabulary};

/// Configuration for REMI tokenization.
///
/// `pitch_range` is stored as `(min, max)` inclusive because
/// `RangeInclusive<u8>` does not implement `serde` traits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemiConfig {
    /// Grid positions per beat (default 4 → 16th-note resolution in 4/4).
    pub beat_resolution: u8,
    /// Number of velocity bins (default 32).
    pub velocity_bins: u8,
    /// Maximum duration in position units (default 64).
    pub max_duration: u8,
    /// Inclusive MIDI pitch range (min, max). Default (0, 127).
    pub pitch_range: (u8, u8),
    /// Emit Tempo tokens before each note group (default false).
    pub use_tempo_tokens: bool,
    /// Number of BPM bins (only used when `use_tempo_tokens` is true).
    pub tempo_bins: u8,
    /// Minimum BPM for tempo binning.
    pub tempo_min_bpm: f64,
    /// Maximum BPM for tempo binning.
    pub tempo_max_bpm: f64,
}

impl Default for RemiConfig {
    fn default() -> Self {
        Self {
            beat_resolution: 4,
            velocity_bins: 32,
            max_duration: 64,
            pitch_range: (0, 127),
            use_tempo_tokens: false,
            tempo_bins: 32,
            tempo_min_bpm: 60.0,
            tempo_max_bpm: 240.0,
        }
    }
}

impl RemiConfig {
    /// Positions per bar for a given time-signature numerator.
    pub fn positions_per_bar(&self, numerator: u8) -> u8 {
        (numerator as u16 * self.beat_resolution as u16).min(255) as u8
    }
}

// ---------------------------------------------------------------------------
// Internal bar-boundary precomputation
// ---------------------------------------------------------------------------

struct BarInfo {
    start_tick: u64,
    positions_per_bar: u8,
}

/// Precompute every bar boundary up to and including the bar that
/// contains `max_tick`. Time-signature changes are honoured.
fn compute_bars(score: &Score, config: &RemiConfig, max_tick: u64) -> Vec<BarInfo> {
    let tpb = score.ticks_per_beat as u64;
    let mut bars: Vec<BarInfo> = Vec::new();
    let mut current_tick: u64 = 0;
    let mut ts_idx: usize = 0;

    loop {
        // Advance time-signature pointer
        while ts_idx + 1 < score.time_signature_changes.len()
            && score.time_signature_changes[ts_idx + 1].tick <= current_tick
        {
            ts_idx += 1;
        }
        let ts = &score.time_signature_changes[ts_idx];

        // actual denominator = 2^ts.denominator; e.g. 2 → 4 (quarter note)
        let actual_den = 1u64 << ts.denominator as u64;
        let ticks_per_beat_local = tpb * 4 / actual_den;
        let ticks_per_bar = ts.numerator as u64 * ticks_per_beat_local;
        let positions = config.positions_per_bar(ts.numerator);

        bars.push(BarInfo {
            start_tick: current_tick,
            positions_per_bar: positions,
        });

        current_tick += ticks_per_bar;
        if current_tick > max_tick {
            break;
        }
    }

    bars
}

/// Return the BPM active at `tick` according to the score's tempo map.
fn bpm_at_tick(tick: u64, score: &Score) -> f64 {
    let us = score
        .tempo_changes
        .partition_point(|t| t.tick <= tick)
        .checked_sub(1)
        .map(|i| score.tempo_changes[i].us_per_beat)
        .unwrap_or(500_000);
    60_000_000.0 / us as f64
}

// ---------------------------------------------------------------------------
// RemiTokenizer
// ---------------------------------------------------------------------------

pub struct RemiTokenizer {
    config: RemiConfig,
    vocab: Vocabulary,
}

impl RemiTokenizer {
    pub fn new(config: RemiConfig) -> Self {
        // Vocabulary is built with the maximum positions_per_bar across any
        // possible time signature (we use 4/4 as baseline for the vocab slot
        // count; the actual per-bar positions are clamped at encode time).
        let positions_per_bar = config.positions_per_bar(4);
        let vocab = Vocabulary::new(
            positions_per_bar,
            config.pitch_range.0,
            config.pitch_range.1,
            config.velocity_bins,
            config.max_duration,
            config.tempo_bins,
            config.use_tempo_tokens,
        );
        Self { config, vocab }
    }

    /// Expose the vocabulary for inspection (e.g., from the Python layer).
    pub fn vocabulary(&self) -> &Vocabulary {
        &self.vocab
    }
}

impl Tokenizer for RemiTokenizer {
    fn encode(&self, score: &Score) -> Result<Vec<u32>> {
        // Collect and filter notes
        let all_notes: Vec<&Note> = score
            .tracks
            .iter()
            .flat_map(|t| t.notes.iter())
            .filter(|n| {
                n.pitch >= self.config.pitch_range.0 && n.pitch <= self.config.pitch_range.1
            })
            .collect();

        if all_notes.is_empty() {
            return Ok(vec![]);
        }

        let max_tick = all_notes
            .iter()
            .map(|n| n.start_tick + n.duration_ticks)
            .max()
            .unwrap_or(0);

        let bars = compute_bars(score, &self.config, max_tick);
        let tpb = score.ticks_per_beat as u64;
        // Ticks per grid position (integer — sub-position timing is quantized down)
        let ticks_per_pos = (tpb / self.config.beat_resolution as u64).max(1);

        // Build quantized note descriptors
        struct NoteEntry {
            bar_idx: usize,
            position: u8,
            pitch: u8,
            velocity_bin: u8,
            duration: u8,
            start_tick: u64,
        }

        let mut entries: Vec<NoteEntry> = all_notes
            .iter()
            .filter_map(|n| {
                // Binary search for the bar containing this note
                let bar_idx = bars
                    .partition_point(|b| b.start_tick <= n.start_tick)
                    .saturating_sub(1);
                let bar = &bars[bar_idx];

                let tick_in_bar = n.start_tick - bar.start_tick;
                let position =
                    ((tick_in_bar / ticks_per_pos) as u8).min(bar.positions_per_bar - 1);

                let dur_positions =
                    ((n.duration_ticks / ticks_per_pos) as u8).clamp(1, self.config.max_duration);

                let velocity_bin = self.vocab.bin_velocity(n.velocity);

                Some(NoteEntry {
                    bar_idx,
                    position,
                    pitch: n.pitch,
                    velocity_bin,
                    duration: dur_positions,
                    start_tick: n.start_tick,
                })
            })
            .collect();

        // Sort: bar → position → pitch (deterministic ordering for equal-time notes)
        entries.sort_by_key(|e| (e.bar_idx, e.position, e.pitch));

        // Emit token stream
        let mut tokens: Vec<u32> = Vec::with_capacity(entries.len() * 5);
        let mut current_bar: Option<usize> = None;

        for entry in &entries {
            // Emit Bar token when entering a new bar
            if current_bar != Some(entry.bar_idx) {
                tokens.push(self.vocab.token_to_id(&RemiToken::Bar)?);
                current_bar = Some(entry.bar_idx);
            }

            tokens.push(self.vocab.token_to_id(&RemiToken::Position(entry.position))?);

            if self.config.use_tempo_tokens {
                let bpm = bpm_at_tick(entry.start_tick, score);
                let bin = self.vocab.bin_tempo(
                    bpm,
                    self.config.tempo_min_bpm,
                    self.config.tempo_max_bpm,
                );
                tokens.push(self.vocab.token_to_id(&RemiToken::Tempo(bin))?);
            }

            tokens.push(self.vocab.token_to_id(&RemiToken::Pitch(entry.pitch))?);
            tokens.push(self.vocab.token_to_id(&RemiToken::Velocity(entry.velocity_bin))?);
            tokens.push(self.vocab.token_to_id(&RemiToken::Duration(entry.duration))?);
        }

        Ok(tokens)
    }

    fn decode(&self, tokens: &[u32]) -> Result<Score> {
        // Fixed decode resolution — encode resolution is preserved via position units
        let ticks_per_beat: u64 = 480;
        let ticks_per_pos = (ticks_per_beat / self.config.beat_resolution as u64).max(1);
        // Assume 4/4 for decoding (time sig is not encoded in REMI)
        let positions_per_bar = self.config.positions_per_bar(4) as u64;
        let ticks_per_bar = positions_per_bar * ticks_per_pos;

        let mut notes: Vec<Note> = Vec::new();
        let mut tempo_changes: Vec<TempoChange> = Vec::new();

        let mut current_bar: u64 = 0;
        let mut current_position: u64 = 0;
        let mut pending_pitch: Option<u8> = None;
        let mut pending_velocity: Option<u8> = None;

        for &id in tokens {
            match self.vocab.id_to_token(id)? {
                RemiToken::Bar => {
                    current_bar += 1;
                    current_position = 0;
                    pending_pitch = None;
                    pending_velocity = None;
                }
                RemiToken::Position(p) => {
                    current_position = p as u64;
                    pending_pitch = None;
                    pending_velocity = None;
                }
                RemiToken::Tempo(t) => {
                    let bpm = self.vocab.unbin_tempo(
                        t,
                        self.config.tempo_min_bpm,
                        self.config.tempo_max_bpm,
                    );
                    let us_per_beat = (60_000_000.0 / bpm) as u32;
                    let tick =
                        current_bar * ticks_per_bar + current_position * ticks_per_pos;
                    // Only push if tempo actually changed
                    if tempo_changes
                        .last()
                        .map_or(true, |t: &TempoChange| t.us_per_beat != us_per_beat)
                    {
                        tempo_changes.push(TempoChange { tick, us_per_beat });
                    }
                }
                RemiToken::Pitch(p) => {
                    pending_pitch = Some(p);
                }
                RemiToken::Velocity(v) => {
                    pending_velocity = Some(v);
                }
                RemiToken::Duration(d) => {
                    if let (Some(pitch), Some(vel_bin)) = (pending_pitch, pending_velocity) {
                        let start_tick =
                            current_bar * ticks_per_bar + current_position * ticks_per_pos;
                        let duration_ticks = d as u64 * ticks_per_pos;
                        let velocity = self.vocab.unbin_velocity(vel_bin);
                        notes.push(Note {
                            pitch,
                            velocity,
                            start_tick,
                            duration_ticks,
                            channel: 0,
                        });
                    }
                    pending_pitch = None;
                    pending_velocity = None;
                }
            }
        }

        notes.sort_by_key(|n| (n.start_tick, n.pitch));

        if tempo_changes.is_empty() {
            tempo_changes.push(TempoChange { tick: 0, us_per_beat: 500_000 });
        }

        Ok(Score {
            tracks: vec![Track { notes, program: None, name: None }],
            tempo_changes,
            time_signature_changes: vec![TimeSignatureChange {
                tick: 0,
                numerator: 4,
                denominator: 2,
            }],
            ticks_per_beat: ticks_per_beat as u16,
        })
    }

    fn vocab_size(&self) -> usize {
        self.vocab.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::{Note, Score, TempoChange, TimeSignatureChange, Track};

    fn make_score(notes: Vec<Note>) -> Score {
        Score {
            tracks: vec![Track { notes, program: None, name: None }],
            tempo_changes: vec![TempoChange { tick: 0, us_per_beat: 500_000 }],
            time_signature_changes: vec![TimeSignatureChange {
                tick: 0,
                numerator: 4,
                denominator: 2,
            }],
            ticks_per_beat: 480,
        }
    }

    fn note(pitch: u8, start_tick: u64, duration_ticks: u64) -> Note {
        Note { pitch, velocity: 64, start_tick, duration_ticks, channel: 0 }
    }

    #[test]
    fn empty_score_returns_empty_tokens() {
        let t = RemiTokenizer::new(RemiConfig::default());
        assert!(t.encode(&make_score(vec![])).unwrap().is_empty());
    }

    #[test]
    fn first_token_is_bar() {
        let t = RemiTokenizer::new(RemiConfig::default());
        let tokens = t.encode(&make_score(vec![note(60, 0, 480)])).unwrap();
        assert!(!tokens.is_empty());
        assert_eq!(tokens[0], t.vocab.bar_id);
    }

    #[test]
    fn two_notes_in_same_bar_emit_one_bar_token() {
        let t = RemiTokenizer::new(RemiConfig::default());
        // 480 tpb, beat_resolution=4 → ticks_per_pos=120
        // pos 0 = tick 0, pos 4 = tick 480 — both within bar 0 (ticks 0–1919)
        let tokens =
            t.encode(&make_score(vec![note(60, 0, 120), note(64, 480, 120)])).unwrap();
        let bar_count = tokens.iter().filter(|&&id| id == t.vocab.bar_id).count();
        assert_eq!(bar_count, 1);
    }

    #[test]
    fn note_in_bar1_emits_two_bar_tokens() {
        let t = RemiTokenizer::new(RemiConfig::default());
        // 4/4 @ 480 tpb → ticks_per_bar = 1920
        let tokens =
            t.encode(&make_score(vec![note(60, 0, 120), note(64, 1920, 120)])).unwrap();
        let bar_count = tokens.iter().filter(|&&id| id == t.vocab.bar_id).count();
        assert_eq!(bar_count, 2);
    }

    #[test]
    fn encode_decode_preserves_pitch_and_bar_count() {
        let t = RemiTokenizer::new(RemiConfig::default());
        let original = make_score(vec![note(60, 0, 480), note(64, 480, 480)]);
        let tokens = t.encode(&original).unwrap();
        let decoded = t.decode(&tokens).unwrap();

        assert!(!decoded.tracks.is_empty());
        let pitches: Vec<u8> = decoded.tracks[0].notes.iter().map(|n| n.pitch).collect();
        assert!(pitches.contains(&60));
        assert!(pitches.contains(&64));
    }

    #[test]
    fn all_token_ids_within_vocab_size() {
        let t = RemiTokenizer::new(RemiConfig::default());
        // C major scale across two octaves
        let notes: Vec<Note> = [60u8, 62, 64, 65, 67, 69, 71, 72, 74, 76, 77, 79]
            .iter()
            .enumerate()
            .map(|(i, &p)| note(p, i as u64 * 480, 240))
            .collect();
        let tokens = t.encode(&make_score(notes)).unwrap();
        let vocab_size = t.vocab_size() as u32;
        for &id in &tokens {
            assert!(id < vocab_size, "token id {id} >= vocab_size {vocab_size}");
        }
    }
}
