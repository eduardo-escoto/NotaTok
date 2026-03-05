use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    midi::{Note, Score, TempoChange, TimeSignatureChange, Track},
    tokenizer::Tokenizer,
    CoreError, Result,
};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for MIDI-Like (performance) tokenization.
///
/// This scheme follows Oore et al. 2018 (Music Transformer). Unlike REMI it
/// uses wall-clock time (milliseconds) expressed via `TimeShift` tokens, and
/// represents notes as open/close `NoteOn`/`NoteOff` pairs rather than
/// `Pitch`+`Duration` tuples.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiLikeConfig {
    /// Duration of each `TimeShift` step in milliseconds. Default 10.
    pub time_shift_ms: u32,
    /// Number of `TimeShift` steps in the vocabulary. Default 100 → max 1 s per token.
    pub time_shift_steps: u8,
    /// Number of velocity bins. Default 32.
    pub velocity_bins: u8,
    /// Inclusive MIDI pitch range (min, max). Default (0, 127).
    pub pitch_range: (u8, u8),
}

impl Default for MidiLikeConfig {
    fn default() -> Self {
        Self {
            time_shift_ms: 10,
            time_shift_steps: 100,
            velocity_bins: 32,
            pitch_range: (0, 127),
        }
    }
}

// ---------------------------------------------------------------------------
// Token enum
// ---------------------------------------------------------------------------

/// A single MIDI-Like vocabulary token.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MidiLikeToken {
    /// Note-on event for the given MIDI pitch.
    NoteOn(u8),
    /// Note-off event for the given MIDI pitch.
    NoteOff(u8),
    /// Advance playback time by `steps × time_shift_ms` ms. Steps are 1-indexed (1..=steps).
    TimeShift(u8),
    /// Set the current velocity bin (0..velocity_bins).
    Velocity(u8),
}

// ---------------------------------------------------------------------------
// Vocabulary
// ---------------------------------------------------------------------------

/// Flat integer vocabulary for MIDI-Like tokens.
///
/// ID layout (default: pitch 0–127, 100 time-shift steps, 32 vel bins):
/// ```text
///   0 –  127 : NoteOn(0)    – NoteOn(127)
/// 128 –  255 : NoteOff(0)   – NoteOff(127)
/// 256 –  355 : TimeShift(1) – TimeShift(100)   (1-indexed)
/// 356 –  387 : Velocity(0)  – Velocity(31)
/// ```
/// Total: 128 + 128 + 100 + 32 = **388**.
struct Vocabulary {
    note_on_base: u32,
    note_off_base: u32,
    time_shift_base: u32,
    velocity_base: u32,
    n_pitches: usize,
    pitch_offset: u8,
    time_shift_steps: u8,
    velocity_bins: u8,
    total: usize,
}

impl Vocabulary {
    fn new(pitch_min: u8, pitch_max: u8, time_shift_steps: u8, velocity_bins: u8) -> Self {
        let n_pitches = (pitch_max as usize).saturating_sub(pitch_min as usize) + 1;
        let note_on_base = 0u32;
        let note_off_base = n_pitches as u32;
        let time_shift_base = note_off_base + n_pitches as u32;
        let velocity_base = time_shift_base + time_shift_steps as u32;
        let total = velocity_base as usize + velocity_bins as usize;
        Self {
            note_on_base,
            note_off_base,
            time_shift_base,
            velocity_base,
            n_pitches,
            pitch_offset: pitch_min,
            time_shift_steps,
            velocity_bins,
            total,
        }
    }

    fn size(&self) -> usize {
        self.total
    }

    fn token_to_id(&self, token: &MidiLikeToken) -> Result<u32> {
        match token {
            MidiLikeToken::NoteOn(p) => {
                let offset = p
                    .checked_sub(self.pitch_offset)
                    .filter(|&o| (o as usize) < self.n_pitches)
                    .ok_or_else(|| {
                        CoreError::Tokenizer(format!(
                            "NoteOn pitch {p} out of range ({} – {})",
                            self.pitch_offset,
                            self.pitch_offset + self.n_pitches as u8 - 1
                        ))
                    })?;
                Ok(self.note_on_base + offset as u32)
            }
            MidiLikeToken::NoteOff(p) => {
                let offset = p
                    .checked_sub(self.pitch_offset)
                    .filter(|&o| (o as usize) < self.n_pitches)
                    .ok_or_else(|| {
                        CoreError::Tokenizer(format!(
                            "NoteOff pitch {p} out of range ({} – {})",
                            self.pitch_offset,
                            self.pitch_offset + self.n_pitches as u8 - 1
                        ))
                    })?;
                Ok(self.note_off_base + offset as u32)
            }
            MidiLikeToken::TimeShift(s) => {
                if *s == 0 || *s > self.time_shift_steps {
                    return Err(CoreError::Tokenizer(format!(
                        "TimeShift({s}) out of range (1 – {})",
                        self.time_shift_steps
                    )));
                }
                Ok(self.time_shift_base + (*s - 1) as u32)
            }
            MidiLikeToken::Velocity(v) => {
                if *v >= self.velocity_bins {
                    return Err(CoreError::Tokenizer(format!(
                        "Velocity bin {v} out of range (max {})",
                        self.velocity_bins - 1
                    )));
                }
                Ok(self.velocity_base + *v as u32)
            }
        }
    }

    fn id_to_token(&self, id: u32) -> Result<MidiLikeToken> {
        if id < self.note_off_base {
            return Ok(MidiLikeToken::NoteOn(self.pitch_offset + (id - self.note_on_base) as u8));
        }
        if id < self.time_shift_base {
            return Ok(MidiLikeToken::NoteOff(
                self.pitch_offset + (id - self.note_off_base) as u8,
            ));
        }
        if id < self.velocity_base {
            // Steps are 1-indexed in the token
            return Ok(MidiLikeToken::TimeShift((id - self.time_shift_base) as u8 + 1));
        }
        if id < self.total as u32 {
            return Ok(MidiLikeToken::Velocity((id - self.velocity_base) as u8));
        }
        Err(CoreError::UnknownTokenId(id))
    }

    /// Map a raw MIDI velocity (0–127) to a bin index (0..velocity_bins−1).
    fn bin_velocity(&self, vel: u8) -> u8 {
        ((vel as u16 * self.velocity_bins as u16) / 128) as u8
    }

    /// Reconstruct a representative raw velocity from a bin index.
    fn unbin_velocity(&self, bin: u8) -> u8 {
        ((bin as u16 * 128 + 64) / self.velocity_bins as u16).min(127) as u8
    }
}

// ---------------------------------------------------------------------------
// Internal event representation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum EventKind {
    NoteOn { velocity: u8 },
    NoteOff,
}

#[derive(Debug, Clone, Copy)]
struct Event {
    tick: u64,
    kind: EventKind,
    pitch: u8,
}

impl Event {
    /// Sort key: tick ascending, NoteOff before NoteOn at equal tick, then pitch ascending.
    fn sort_key(&self) -> (u64, u8, u8) {
        let kind_order = match self.kind {
            EventKind::NoteOff => 0u8,
            EventKind::NoteOn { .. } => 1u8,
        };
        (self.tick, kind_order, self.pitch)
    }
}

// ---------------------------------------------------------------------------
// Tempo helpers
// ---------------------------------------------------------------------------

/// Return the `us_per_beat` active at `tick` from the score's tempo map.
fn us_per_beat_at(tick: u64, score: &Score) -> u32 {
    score
        .tempo_changes
        .partition_point(|t| t.tick <= tick)
        .checked_sub(1)
        .map(|i| score.tempo_changes[i].us_per_beat)
        .unwrap_or(500_000)
}

/// Convert a tick delta to whole milliseconds using the tempo active at `from_tick`.
///
/// Formula: `ms = delta_ticks × us_per_beat / (ticks_per_beat × 1000)`
fn ticks_to_ms(from_tick: u64, delta_ticks: u64, score: &Score) -> u64 {
    if delta_ticks == 0 {
        return 0;
    }
    let us_per_beat = us_per_beat_at(from_tick, score) as u64;
    let tpb = score.ticks_per_beat as u64;
    delta_ticks * us_per_beat / tpb / 1000
}

// ---------------------------------------------------------------------------
// MidiLikeTokenizer
// ---------------------------------------------------------------------------

/// Tokenizer implementing the MIDI-Like (performance) encoding from Oore et al. 2018.
///
/// This is a purely event-driven scheme with no bar or beat structure. Timing is
/// expressed in milliseconds via `TimeShift` tokens, and notes are open/close
/// `NoteOn`/`NoteOff` pairs, preserving polyphony naturally.
///
/// Decoding is lossy: timing is quantised to `time_shift_ms` steps and velocities
/// are binned. The reconstructed `Score` uses a fixed 120 BPM reference tempo.
pub struct MidiLikeTokenizer {
    config: MidiLikeConfig,
    vocab: Vocabulary,
}

impl MidiLikeTokenizer {
    /// Create a new tokenizer with the given configuration.
    pub fn new(config: MidiLikeConfig) -> Self {
        let vocab = Vocabulary::new(
            config.pitch_range.0,
            config.pitch_range.1,
            config.time_shift_steps,
            config.velocity_bins,
        );
        Self { config, vocab }
    }
}

impl Tokenizer for MidiLikeTokenizer {
    fn encode(&self, score: &Score) -> Result<Vec<u32>> {
        // Collect notes within the configured pitch range
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

        // Build flat event list: two entries per note (NoteOn + NoteOff)
        let mut events: Vec<Event> = Vec::with_capacity(all_notes.len() * 2);
        for note in &all_notes {
            events.push(Event {
                tick: note.start_tick,
                kind: EventKind::NoteOn { velocity: note.velocity },
                pitch: note.pitch,
            });
            events.push(Event {
                tick: note.start_tick + note.duration_ticks,
                kind: EventKind::NoteOff,
                pitch: note.pitch,
            });
        }

        // Sort: tick asc, NoteOff before NoteOn at equal tick, pitch asc
        events.sort_by_key(|e| e.sort_key());

        let mut tokens: Vec<u32> = Vec::with_capacity(events.len() * 3);
        let mut current_tick: u64 = 0;
        let mut current_velocity_bin: Option<u8> = None;

        let steps = self.config.time_shift_steps as u64;
        let step_ms = self.config.time_shift_ms as u64;
        let max_ms_per_token = steps * step_ms;

        for event in &events {
            // Emit TimeShift tokens to cover the elapsed time
            if event.tick > current_tick {
                let mut remaining_ms =
                    ticks_to_ms(current_tick, event.tick - current_tick, score);

                // Emit full max-step chunks
                while remaining_ms >= max_ms_per_token {
                    tokens.push(
                        self.vocab
                            .token_to_id(&MidiLikeToken::TimeShift(self.config.time_shift_steps))?,
                    );
                    remaining_ms -= max_ms_per_token;
                }

                // Emit remainder (ceiling division to avoid losing small gaps)
                if remaining_ms > 0 {
                    let partial = ((remaining_ms + step_ms - 1) / step_ms).min(steps) as u8;
                    tokens.push(self.vocab.token_to_id(&MidiLikeToken::TimeShift(partial))?);
                }

                current_tick = event.tick;
            }

            match event.kind {
                EventKind::NoteOn { velocity } => {
                    let vel_bin = self.vocab.bin_velocity(velocity);
                    if current_velocity_bin != Some(vel_bin) {
                        tokens.push(self.vocab.token_to_id(&MidiLikeToken::Velocity(vel_bin))?);
                        current_velocity_bin = Some(vel_bin);
                    }
                    tokens.push(self.vocab.token_to_id(&MidiLikeToken::NoteOn(event.pitch))?);
                }
                EventKind::NoteOff => {
                    tokens.push(self.vocab.token_to_id(&MidiLikeToken::NoteOff(event.pitch))?);
                }
            }
        }

        Ok(tokens)
    }

    fn decode(&self, tokens: &[u32]) -> Result<Score> {
        // Fixed decode reference: 120 BPM (500 000 µs/beat), 480 ticks/beat.
        // ms_to_tick(ms) = ms × 480 / 500
        let ticks_per_beat: u64 = 480;
        let us_per_beat: u64 = 500_000;
        let ms_to_tick = |ms: u64| -> u64 { ms * ticks_per_beat * 1000 / us_per_beat };

        let step_ms = self.config.time_shift_ms as u64;

        let mut current_ms: u64 = 0;
        // Default to the bin that contains velocity 64
        let mut current_velocity_bin: u8 = self.vocab.bin_velocity(64);
        // pitch → (start_tick, velocity)
        let mut open_notes: HashMap<u8, (u64, u8)> = HashMap::new();
        let mut notes: Vec<Note> = Vec::new();

        for &id in tokens {
            match self.vocab.id_to_token(id)? {
                MidiLikeToken::TimeShift(s) => {
                    current_ms += s as u64 * step_ms;
                }
                MidiLikeToken::Velocity(v) => {
                    current_velocity_bin = v;
                }
                MidiLikeToken::NoteOn(p) => {
                    let start_tick = ms_to_tick(current_ms);
                    let velocity = self.vocab.unbin_velocity(current_velocity_bin);
                    open_notes.insert(p, (start_tick, velocity));
                }
                MidiLikeToken::NoteOff(p) => {
                    if let Some((start_tick, velocity)) = open_notes.remove(&p) {
                        let end_tick = ms_to_tick(current_ms);
                        let duration_ticks = end_tick.saturating_sub(start_tick).max(1);
                        notes.push(Note {
                            pitch: p,
                            velocity,
                            start_tick,
                            duration_ticks,
                            channel: 0,
                        });
                    }
                }
            }
        }

        // Close any notes left open at end of stream (edge case) with duration 1
        for (pitch, (start_tick, velocity)) in open_notes {
            notes.push(Note { pitch, velocity, start_tick, duration_ticks: 1, channel: 0 });
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

    fn note(pitch: u8, vel: u8, start_tick: u64, duration_ticks: u64) -> Note {
        Note { pitch, velocity: vel, start_tick, duration_ticks, channel: 0 }
    }

    #[test]
    fn default_vocab_size_is_388() {
        let t = MidiLikeTokenizer::new(MidiLikeConfig::default());
        // 128 NoteOn + 128 NoteOff + 100 TimeShift + 32 Velocity = 388
        assert_eq!(t.vocab_size(), 388);
    }

    #[test]
    fn empty_score_returns_empty_tokens() {
        let t = MidiLikeTokenizer::new(MidiLikeConfig::default());
        assert!(t.encode(&make_score(vec![])).unwrap().is_empty());
    }

    #[test]
    fn first_token_is_note_on_for_immediate_note() {
        let t = MidiLikeTokenizer::new(MidiLikeConfig::default());
        // A note at tick=0 requires no TimeShift; tokens begin with Velocity then NoteOn.
        let tokens = t.encode(&make_score(vec![note(60, 64, 0, 480)])).unwrap();
        assert!(!tokens.is_empty());
        let note_on_id = t.vocab.token_to_id(&MidiLikeToken::NoteOn(60)).unwrap();
        let note_on_pos = tokens.iter().position(|&id| id == note_on_id).unwrap();
        // Every token before NoteOn(60) must be a Velocity token (no TimeShift at tick 0)
        for &id in &tokens[..note_on_pos] {
            assert!(
                id >= t.vocab.velocity_base,
                "unexpected token id={id} before NoteOn at tick 0"
            );
        }
    }

    #[test]
    fn time_shift_emitted_between_notes() {
        let t = MidiLikeTokenizer::new(MidiLikeConfig::default());
        // 480 ticks gap at 120 BPM = 500 ms → at least one TimeShift token
        let tokens = t
            .encode(&make_score(vec![note(60, 64, 0, 240), note(64, 64, 480, 240)]))
            .unwrap();
        let has_shift = tokens
            .iter()
            .any(|&id| id >= t.vocab.time_shift_base && id < t.vocab.velocity_base);
        assert!(has_shift, "expected at least one TimeShift token");
    }

    #[test]
    fn long_silence_emits_multiple_time_shift_tokens() {
        let t = MidiLikeTokenizer::new(MidiLikeConfig::default());
        // 4800 ticks at 120 BPM = 5000 ms → 5 × TimeShift(100) tokens
        let tokens = t
            .encode(&make_score(vec![note(60, 64, 0, 240), note(64, 64, 4800, 240)]))
            .unwrap();
        let shift_count = tokens
            .iter()
            .filter(|&&id| id >= t.vocab.time_shift_base && id < t.vocab.velocity_base)
            .count();
        assert!(shift_count >= 2, "expected multiple TimeShift tokens, got {shift_count}");
    }

    #[test]
    fn velocity_token_emitted_on_change() {
        let t = MidiLikeTokenizer::new(MidiLikeConfig::default());
        // Two notes with different velocities → at least two Velocity tokens emitted
        let tokens = t
            .encode(&make_score(vec![
                note(60, 32, 0, 240),    // low velocity
                note(64, 120, 480, 240), // high velocity
            ]))
            .unwrap();
        let vel_count = tokens.iter().filter(|&&id| id >= t.vocab.velocity_base).count();
        assert!(vel_count >= 2, "expected at least 2 Velocity tokens, got {vel_count}");
    }

    #[test]
    fn note_off_before_note_on_at_same_tick() {
        let t = MidiLikeTokenizer::new(MidiLikeConfig::default());
        // NoteOff for pitch 60 and NoteOn for pitch 64 both fall at tick 480
        let tokens = t
            .encode(&make_score(vec![
                note(60, 64, 0, 480),   // ends at tick 480
                note(64, 64, 480, 240), // starts at tick 480
            ]))
            .unwrap();
        let note_off_60 = t.vocab.token_to_id(&MidiLikeToken::NoteOff(60)).unwrap();
        let note_on_64 = t.vocab.token_to_id(&MidiLikeToken::NoteOn(64)).unwrap();
        let off_pos = tokens.iter().position(|&id| id == note_off_60).unwrap();
        let on_pos = tokens.iter().position(|&id| id == note_on_64).unwrap();
        assert!(off_pos < on_pos, "NoteOff should precede NoteOn at the same tick");
    }

    #[test]
    fn encode_decode_preserves_pitches_and_order() {
        let t = MidiLikeTokenizer::new(MidiLikeConfig::default());
        let original = make_score(vec![
            note(60, 64, 0, 480),
            note(64, 64, 480, 480),
            note(67, 64, 960, 480),
        ]);
        let tokens = t.encode(&original).unwrap();
        let decoded = t.decode(&tokens).unwrap();

        assert!(!decoded.tracks.is_empty());
        let mut decoded_notes = decoded.tracks[0].notes.clone();
        decoded_notes.sort_by_key(|n| n.start_tick);
        let pitches: Vec<u8> = decoded_notes.iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, vec![60, 64, 67]);
    }

    #[test]
    fn all_token_ids_within_vocab_size() {
        let t = MidiLikeTokenizer::new(MidiLikeConfig::default());
        let notes: Vec<Note> = [60u8, 62, 64, 65, 67, 69, 71, 72]
            .iter()
            .enumerate()
            .map(|(i, &p)| note(p, 64, i as u64 * 480, 240))
            .collect();
        let tokens = t.encode(&make_score(notes)).unwrap();
        let vocab_size = t.vocab_size() as u32;
        for &id in &tokens {
            assert!(id < vocab_size, "token id {id} >= vocab_size {vocab_size}");
        }
    }
}
