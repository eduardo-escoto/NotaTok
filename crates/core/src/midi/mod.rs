pub mod load;
pub mod save;

pub use load::load_midi;
pub use save::save_midi;

use serde::{Deserialize, Serialize};

/// A single MIDI note with absolute timing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// MIDI pitch 0–127.
    pub pitch: u8,
    /// Raw MIDI velocity 0–127.
    pub velocity: u8,
    /// Absolute start position in ticks (accumulated delta times).
    pub start_tick: u64,
    /// Duration in ticks (end_tick − start_tick). 0 if NoteOn was never closed.
    pub duration_ticks: u64,
    /// MIDI channel 0–15.
    pub channel: u8,
}

/// A tempo change event. Default MIDI tempo is 500 000 µs/beat (120 BPM).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TempoChange {
    pub tick: u64,
    /// Microseconds per beat.
    pub us_per_beat: u32,
}

/// A time signature change.
///
/// `denominator` is stored as a power of 2, exactly as the SMF spec and midly
/// represent it. Actual denominator = `1 << denominator`.
/// For example, 4/4 → numerator=4, denominator=2 (2^2 = 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSignatureChange {
    pub tick: u64,
    pub numerator: u8,
    /// Power-of-2 exponent: actual denominator = 1 << denominator.
    pub denominator: u8,
}

/// A single MIDI track (one instrument voice).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub notes: Vec<Note>,
    /// GM programme number 0–127, if a ProgramChange event was present.
    pub program: Option<u8>,
    /// Track name from MetaMessage::TrackName, UTF-8 lossy.
    pub name: Option<String>,
}

/// Normalised intermediate representation of a MIDI file.
///
/// All timing is in ticks. Delta times have been accumulated into absolute
/// positions. NoteOn/NoteOff pairs have been matched into [`Note`] structs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Score {
    pub tracks: Vec<Track>,
    /// Sorted ascending by tick.
    pub tempo_changes: Vec<TempoChange>,
    /// Sorted ascending by tick.
    pub time_signature_changes: Vec<TimeSignatureChange>,
    pub ticks_per_beat: u16,
}
