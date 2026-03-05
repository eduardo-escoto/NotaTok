use serde::{Deserialize, Serialize};

use crate::{
    midi::{Note, Score, TempoChange, TimeSignatureChange, Track},
    tokenizer::Tokenizer,
    CoreError, Result,
};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the Compound Word (CP) tokenizer.
///
/// CP decomposes each note's bar-relative position into two separate fields:
/// `Beat` (beat index within the bar) and `SubPosition` (grid slot within the
/// beat). This mirrors the architecture from Hsiao et al. 2021, giving
/// downstream models independent embeddings for beat-level and sub-beat timing.
///
/// `pitch_range` is stored as `(min, max)` inclusive because
/// `RangeInclusive<u8>` does not implement `serde` traits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompoundConfig {
    /// Grid positions per beat. Default 4 → 16th-note resolution.
    pub beat_resolution: u8,
    /// Number of velocity bins. Default 32.
    pub velocity_bins: u8,
    /// Maximum duration in position units. Default 64.
    pub max_duration: u8,
    /// Inclusive MIDI pitch range (min, max). Default (0, 127).
    pub pitch_range: (u8, u8),
    /// Maximum beats per bar supported. Default 8 (covers up to 8/4 time).
    pub max_beats: u8,
}

impl Default for CompoundConfig {
    fn default() -> Self {
        Self {
            beat_resolution: 4,
            velocity_bins: 32,
            max_duration: 64,
            pitch_range: (0, 127),
            max_beats: 8,
        }
    }
}

// ---------------------------------------------------------------------------
// Token types
// ---------------------------------------------------------------------------

/// A single Compound Word token variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompoundToken {
    /// Marks the start of a new bar.
    Bar,
    /// Beat index within the current bar (0..max_beats−1).
    Beat(u8),
    /// Sub-beat grid position within the current beat (0..beat_resolution−1).
    SubPosition(u8),
    /// Absolute MIDI pitch value within the configured pitch range.
    Pitch(u8),
    /// Binned velocity index (0..velocity_bins−1).
    Velocity(u8),
    /// Duration in position units (1..=max_duration, 1-indexed).
    Duration(u8),
}

// ---------------------------------------------------------------------------
// Vocabulary
// ---------------------------------------------------------------------------

/// Flat vocabulary that maps [`CompoundToken`] variants to contiguous integer IDs.
///
/// ID layout (default config: max_beats=8, beat_resolution=4, pitch 0-127,
/// velocity_bins=32, max_duration=64):
/// ```text
/// 0          → Bar
/// 1  – 8     → Beat(0)        – Beat(7)
/// 9  – 12    → SubPosition(0) – SubPosition(3)
/// 13 – 140   → Pitch(0)       – Pitch(127)
/// 141 – 172  → Velocity(0)    – Velocity(31)
/// 173 – 236  → Duration(1)    – Duration(64)
/// ```
/// Total: 1 + 8 + 4 + 128 + 32 + 64 = 237
#[derive(Debug, Clone)]
pub struct Vocabulary {
    pub bar_id: u32,
    pub beat_base: u32,
    pub sub_pos_base: u32,
    pub pitch_base: u32,
    pub velocity_base: u32,
    pub duration_base: u32,

    pub max_beats: u8,
    pub beat_resolution: u8,
    pub n_pitches: usize,
    pub pitch_offset: u8,
    pub velocity_bins: u8,
    pub max_duration: u8,

    total: usize,
}

impl Vocabulary {
    pub fn new(
        max_beats: u8,
        beat_resolution: u8,
        pitch_min: u8,
        pitch_max: u8,
        velocity_bins: u8,
        max_duration: u8,
    ) -> Self {
        let n_pitches = (pitch_max as usize).saturating_sub(pitch_min as usize) + 1;
        let bar_id = 0u32;
        let beat_base = 1u32;
        let sub_pos_base = beat_base + max_beats as u32;
        let pitch_base = sub_pos_base + beat_resolution as u32;
        let velocity_base = pitch_base + n_pitches as u32;
        let duration_base = velocity_base + velocity_bins as u32;
        let total = duration_base as usize + max_duration as usize;

        Self {
            bar_id,
            beat_base,
            sub_pos_base,
            pitch_base,
            velocity_base,
            duration_base,
            max_beats,
            beat_resolution,
            n_pitches,
            pitch_offset: pitch_min,
            velocity_bins,
            max_duration,
            total,
        }
    }

    pub fn size(&self) -> usize {
        self.total
    }

    pub fn token_to_id(&self, token: &CompoundToken) -> Result<u32> {
        match token {
            CompoundToken::Bar => Ok(self.bar_id),

            CompoundToken::Beat(b) => {
                if *b >= self.max_beats {
                    return Err(CoreError::Tokenizer(format!(
                        "Beat({b}) out of range (max {})",
                        self.max_beats - 1
                    )));
                }
                Ok(self.beat_base + *b as u32)
            }

            CompoundToken::SubPosition(p) => {
                if *p >= self.beat_resolution {
                    return Err(CoreError::Tokenizer(format!(
                        "SubPosition({p}) out of range (max {})",
                        self.beat_resolution - 1
                    )));
                }
                Ok(self.sub_pos_base + *p as u32)
            }

            CompoundToken::Pitch(p) => {
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

            CompoundToken::Velocity(v) => {
                if *v >= self.velocity_bins {
                    return Err(CoreError::Tokenizer(format!(
                        "Velocity bin {v} out of range (max {})",
                        self.velocity_bins - 1
                    )));
                }
                Ok(self.velocity_base + *v as u32)
            }

            CompoundToken::Duration(d) => {
                if *d == 0 || *d > self.max_duration {
                    return Err(CoreError::Tokenizer(format!(
                        "Duration({d}) out of range (1 – {})",
                        self.max_duration
                    )));
                }
                Ok(self.duration_base + (*d - 1) as u32)
            }
        }
    }

    pub fn id_to_token(&self, id: u32) -> Result<CompoundToken> {
        if id == self.bar_id {
            return Ok(CompoundToken::Bar);
        }
        if id >= self.beat_base && id < self.sub_pos_base {
            return Ok(CompoundToken::Beat((id - self.beat_base) as u8));
        }
        if id >= self.sub_pos_base && id < self.pitch_base {
            return Ok(CompoundToken::SubPosition((id - self.sub_pos_base) as u8));
        }
        if id >= self.pitch_base && id < self.velocity_base {
            return Ok(CompoundToken::Pitch(
                self.pitch_offset + (id - self.pitch_base) as u8,
            ));
        }
        if id >= self.velocity_base && id < self.duration_base {
            return Ok(CompoundToken::Velocity((id - self.velocity_base) as u8));
        }
        if id >= self.duration_base && id < self.duration_base + self.max_duration as u32 {
            return Ok(CompoundToken::Duration((id - self.duration_base) as u8 + 1));
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
}

// ---------------------------------------------------------------------------
// Bar-boundary precomputation (same algorithm as REMI)
// ---------------------------------------------------------------------------

struct BarInfo {
    start_tick: u64,
}

/// Precompute every bar boundary up to and including the bar that
/// contains `max_tick`. Time-signature changes are honoured.
fn compute_bars(score: &Score, ticks_per_beat: u64, max_tick: u64) -> Vec<BarInfo> {
    let mut bars: Vec<BarInfo> = Vec::new();
    let mut current_tick: u64 = 0;
    let mut ts_idx: usize = 0;

    loop {
        while ts_idx + 1 < score.time_signature_changes.len()
            && score.time_signature_changes[ts_idx + 1].tick <= current_tick
        {
            ts_idx += 1;
        }
        let ts = &score.time_signature_changes[ts_idx];

        let actual_den = 1u64 << ts.denominator as u64;
        let ticks_per_beat_local = ticks_per_beat * 4 / actual_den;
        let ticks_per_bar = ts.numerator as u64 * ticks_per_beat_local;

        bars.push(BarInfo { start_tick: current_tick });

        current_tick += ticks_per_bar;
        if current_tick > max_tick {
            break;
        }
    }

    bars
}

// ---------------------------------------------------------------------------
// CompoundTokenizer
// ---------------------------------------------------------------------------

/// Compound Word (CP) tokenizer.
///
/// Encodes each note as a 5-token compound word:
/// `Beat | SubPosition | Pitch | Velocity | Duration`
///
/// Bar boundaries are marked with a single `Bar` token, producing sequences:
/// `Bar [Beat SubPos Pitch Vel Dur] [Beat SubPos Pitch Vel Dur] … Bar …`
pub struct CompoundTokenizer {
    config: CompoundConfig,
    vocab: Vocabulary,
}

impl CompoundTokenizer {
    pub fn new(config: CompoundConfig) -> Self {
        let vocab = Vocabulary::new(
            config.max_beats,
            config.beat_resolution,
            config.pitch_range.0,
            config.pitch_range.1,
            config.velocity_bins,
            config.max_duration,
        );
        Self { config, vocab }
    }

    /// Expose the vocabulary for inspection.
    pub fn vocabulary(&self) -> &Vocabulary {
        &self.vocab
    }
}

impl Tokenizer for CompoundTokenizer {
    fn encode(&self, score: &Score) -> Result<Vec<u32>> {
        // Collect and filter notes by pitch range
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

        let tpb = score.ticks_per_beat as u64;
        let ticks_per_pos = (tpb / self.config.beat_resolution as u64).max(1);

        let bars = compute_bars(score, tpb, max_tick);

        struct NoteEntry {
            bar_idx: usize,
            beat: u8,
            sub_pos: u8,
            pitch: u8,
            velocity_bin: u8,
            duration: u8,
        }

        let max_beats = self.config.max_beats;
        let br = self.config.beat_resolution;

        let mut entries: Vec<NoteEntry> = all_notes
            .iter()
            .filter_map(|n| {
                let bar_idx = bars
                    .partition_point(|b| b.start_tick <= n.start_tick)
                    .saturating_sub(1);
                let bar = &bars[bar_idx];

                let tick_in_bar = n.start_tick - bar.start_tick;
                let global_pos = tick_in_bar / ticks_per_pos;
                let beat = (global_pos / br as u64).min((max_beats - 1) as u64) as u8;
                let sub_pos = (global_pos % br as u64) as u8;

                let duration = ((n.duration_ticks / ticks_per_pos) as u8)
                    .max(1)
                    .min(self.config.max_duration);

                let velocity_bin = self.vocab.bin_velocity(n.velocity);

                Some(NoteEntry { bar_idx, beat, sub_pos, pitch: n.pitch, velocity_bin, duration })
            })
            .collect();

        // Sort: bar → beat → sub_pos → pitch (deterministic)
        entries.sort_by_key(|e| (e.bar_idx, e.beat, e.sub_pos, e.pitch));

        let mut tokens: Vec<u32> = Vec::with_capacity(entries.len() * 5 + bars.len());
        let mut current_bar: Option<usize> = None;

        for entry in &entries {
            if current_bar != Some(entry.bar_idx) {
                tokens.push(self.vocab.token_to_id(&CompoundToken::Bar)?);
                current_bar = Some(entry.bar_idx);
            }

            tokens.push(self.vocab.token_to_id(&CompoundToken::Beat(entry.beat))?);
            tokens.push(self.vocab.token_to_id(&CompoundToken::SubPosition(entry.sub_pos))?);
            tokens.push(self.vocab.token_to_id(&CompoundToken::Pitch(entry.pitch))?);
            tokens.push(self.vocab.token_to_id(&CompoundToken::Velocity(entry.velocity_bin))?);
            tokens.push(self.vocab.token_to_id(&CompoundToken::Duration(entry.duration))?);
        }

        Ok(tokens)
    }

    fn decode(&self, tokens: &[u32]) -> Result<Score> {
        // Fixed decode resolution: 480 ticks/beat, 4/4, 120 BPM
        let ticks_per_beat: u64 = 480;
        let br = self.config.beat_resolution as u64;
        let ticks_per_pos = (ticks_per_beat / br).max(1);
        // 4/4 → 4 beats per bar
        let positions_per_bar = br * 4;
        let ticks_per_bar = positions_per_bar * ticks_per_pos;

        let mut notes: Vec<Note> = Vec::new();

        let mut current_bar: u64 = 0;
        let mut pending_beat: Option<u8> = None;
        let mut pending_sub_pos: Option<u8> = None;
        let mut pending_pitch: Option<u8> = None;
        let mut pending_velocity: Option<u8> = None;

        for &id in tokens {
            match self.vocab.id_to_token(id)? {
                CompoundToken::Bar => {
                    current_bar += 1;
                    pending_beat = None;
                    pending_sub_pos = None;
                    pending_pitch = None;
                    pending_velocity = None;
                }
                CompoundToken::Beat(b) => {
                    pending_beat = Some(b);
                    pending_pitch = None;
                    pending_velocity = None;
                }
                CompoundToken::SubPosition(p) => {
                    pending_sub_pos = Some(p);
                    pending_pitch = None;
                    pending_velocity = None;
                }
                CompoundToken::Pitch(p) => {
                    pending_pitch = Some(p);
                }
                CompoundToken::Velocity(v) => {
                    pending_velocity = Some(v);
                }
                CompoundToken::Duration(d) => {
                    if let (Some(beat), Some(sub_pos), Some(pitch), Some(vel_bin)) =
                        (pending_beat, pending_sub_pos, pending_pitch, pending_velocity)
                    {
                        let global_pos = beat as u64 * br + sub_pos as u64;
                        let start_tick =
                            current_bar * ticks_per_bar + global_pos * ticks_per_pos;
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

        Ok(Score {
            tracks: vec![Track { notes, program: None, name: None }],
            tempo_changes: vec![TempoChange { tick: 0, us_per_beat: 500_000 }],
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
    fn default_vocab_size_is_237() {
        // 1 (Bar) + 8 (Beat) + 4 (SubPos) + 128 (Pitch) + 32 (Vel) + 64 (Dur) = 237
        let t = CompoundTokenizer::new(CompoundConfig::default());
        assert_eq!(t.vocab_size(), 237);
    }

    #[test]
    fn empty_score_returns_empty_tokens() {
        let t = CompoundTokenizer::new(CompoundConfig::default());
        assert!(t.encode(&make_score(vec![])).unwrap().is_empty());
    }

    #[test]
    fn first_tokens_are_bar_beat_subpos() {
        let t = CompoundTokenizer::new(CompoundConfig::default());
        let tokens = t.encode(&make_score(vec![note(60, 0, 480)])).unwrap();
        assert!(!tokens.is_empty());
        // tokens[0] = Bar, tokens[1] = Beat(0), tokens[2] = SubPosition(0)
        assert_eq!(tokens[0], t.vocab.bar_id);
        assert_eq!(tokens[1], t.vocab.beat_base); // Beat(0) = beat_base + 0
        assert_eq!(tokens[2], t.vocab.sub_pos_base); // SubPosition(0) = sub_pos_base + 0
    }

    #[test]
    fn two_notes_same_bar_share_one_bar_token() {
        let t = CompoundTokenizer::new(CompoundConfig::default());
        // 480 tpb, beat_resolution=4 → ticks_per_pos=120
        // both ticks 0 and 480 are in bar 0 (ticks 0–1919)
        let tokens =
            t.encode(&make_score(vec![note(60, 0, 120), note(64, 480, 120)])).unwrap();
        let bar_count = tokens.iter().filter(|&&id| id == t.vocab.bar_id).count();
        assert_eq!(bar_count, 1);
    }

    #[test]
    fn note_in_second_bar_emits_two_bar_tokens() {
        let t = CompoundTokenizer::new(CompoundConfig::default());
        // 4/4 @ 480 tpb → ticks_per_bar = 1920
        let tokens =
            t.encode(&make_score(vec![note(60, 0, 120), note(64, 1920, 120)])).unwrap();
        let bar_count = tokens.iter().filter(|&&id| id == t.vocab.bar_id).count();
        assert_eq!(bar_count, 2);
    }

    #[test]
    fn beat_and_sub_position_decomposed_correctly() {
        // tick 480, tpb=480, beat_resolution=4 → ticks_per_pos=120
        // tick_in_bar = 480, global_pos = 480/120 = 4
        // beat = 4 / 4 = 1, sub_pos = 4 % 4 = 0
        let t = CompoundTokenizer::new(CompoundConfig::default());
        let tokens = t.encode(&make_score(vec![note(60, 480, 120)])).unwrap();
        // tokens: [Bar, Beat(1), SubPos(0), Pitch(60), Vel, Dur]
        assert_eq!(tokens[1], t.vocab.beat_base + 1); // Beat(1)
        assert_eq!(tokens[2], t.vocab.sub_pos_base + 0); // SubPosition(0)
    }

    #[test]
    fn encode_decode_preserves_pitches_and_order() {
        let t = CompoundTokenizer::new(CompoundConfig::default());
        let original = make_score(vec![note(60, 0, 480), note(64, 480, 480)]);
        let tokens = t.encode(&original).unwrap();
        let decoded = t.decode(&tokens).unwrap();

        assert!(!decoded.tracks.is_empty());
        let pitches: Vec<u8> = decoded.tracks[0].notes.iter().map(|n| n.pitch).collect();
        assert!(pitches.contains(&60));
        assert!(pitches.contains(&64));
        // Order preserved (sorted by start_tick then pitch)
        assert!(decoded.tracks[0].notes[0].start_tick <= decoded.tracks[0].notes[1].start_tick);
    }

    #[test]
    fn all_token_ids_within_vocab_size() {
        let t = CompoundTokenizer::new(CompoundConfig::default());
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

    #[test]
    fn velocity_token_emitted_per_note() {
        let t = CompoundTokenizer::new(CompoundConfig::default());
        // 3 notes → 1 Bar + 3 * 5 compound tokens = 16 total
        let tokens = t
            .encode(&make_score(vec![
                note(60, 0, 120),
                note(64, 120, 120),
                note(67, 240, 120),
            ]))
            .unwrap();
        // Count velocity tokens: IDs in [velocity_base, duration_base)
        let vel_count = tokens
            .iter()
            .filter(|&&id| {
                id >= t.vocab.velocity_base && id < t.vocab.duration_base
            })
            .count();
        assert_eq!(vel_count, 3);
    }

    #[test]
    fn beat_sub_position_round_trip_through_vocab() {
        let t = CompoundTokenizer::new(CompoundConfig::default());
        let v = &t.vocab;
        // All Beat and SubPosition IDs round-trip cleanly
        for b in 0..v.max_beats {
            let id = v.token_to_id(&CompoundToken::Beat(b)).unwrap();
            assert_eq!(v.id_to_token(id).unwrap(), CompoundToken::Beat(b));
        }
        for p in 0..v.beat_resolution {
            let id = v.token_to_id(&CompoundToken::SubPosition(p)).unwrap();
            assert_eq!(v.id_to_token(id).unwrap(), CompoundToken::SubPosition(p));
        }
    }
}
