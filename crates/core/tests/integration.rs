/// Integration tests: full pipeline Score → save_midi → load_midi → encode → decode.
///
/// These tests exercise the real MIDI serialisation/parsing path that the unit
/// tests skip, and verify that all four tokenizers produce valid, consistent
/// output from a round-tripped MIDI file.
use notatok_core::midi::{load_midi, save_midi, Note, Score, TempoChange, TimeSignatureChange, Track};
use notatok_core::tokenizer::{
    abc::{AbcConfig, AbcTokenizer},
    compound::{CompoundConfig, CompoundTokenizer},
    midi_like::{MidiLikeConfig, MidiLikeTokenizer},
    remi::{RemiConfig, RemiTokenizer},
    Tokenizer,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A two-bar C-major arpeggio at 120 BPM, 4/4, 480 ticks/beat.
fn reference_score() -> Score {
    let pitches = [60u8, 64, 67, 72, 67, 64, 60, 64]; // C4 E4 G4 C5 …
    let notes = pitches
        .iter()
        .enumerate()
        .map(|(i, &pitch)| Note {
            pitch,
            velocity: 80,
            start_tick: i as u64 * 480,
            duration_ticks: 360,
            channel: 0,
        })
        .collect();

    Score {
        tracks: vec![Track { notes, program: None, name: None }],
        tempo_changes: vec![TempoChange { tick: 0, us_per_beat: 500_000 }],
        time_signature_changes: vec![TimeSignatureChange {
            tick: 0,
            numerator: 4,
            denominator: 2, // 2^2 = 4 → 4/4
        }],
        ticks_per_beat: 480,
    }
}

/// Save `score` to MIDI bytes then load it back. This exercises the full
/// serialise → parse round-trip without touching the filesystem.
fn midi_round_trip(score: &Score) -> Score {
    let bytes = save_midi(score).expect("save_midi failed");
    load_midi(&bytes).expect("load_midi failed on bytes produced by save_midi")
}

// ---------------------------------------------------------------------------
// save_midi / load_midi round-trip
// ---------------------------------------------------------------------------

#[test]
fn midi_round_trip_preserves_note_count() {
    let original = reference_score();
    let reloaded = midi_round_trip(&original);
    let orig_count: usize = original.tracks.iter().map(|t| t.notes.len()).sum();
    let reload_count: usize = reloaded.tracks.iter().map(|t| t.notes.len()).sum();
    assert_eq!(orig_count, reload_count, "note count changed after MIDI round-trip");
}

#[test]
fn midi_round_trip_preserves_pitches() {
    let original = reference_score();
    let reloaded = midi_round_trip(&original);

    let mut orig_pitches: Vec<u8> =
        original.tracks.iter().flat_map(|t| t.notes.iter().map(|n| n.pitch)).collect();
    let mut reload_pitches: Vec<u8> =
        reloaded.tracks.iter().flat_map(|t| t.notes.iter().map(|n| n.pitch)).collect();
    orig_pitches.sort_unstable();
    reload_pitches.sort_unstable();

    assert_eq!(orig_pitches, reload_pitches, "pitches changed after MIDI round-trip");
}

#[test]
fn midi_round_trip_preserves_ticks_per_beat() {
    let original = reference_score();
    let reloaded = midi_round_trip(&original);
    assert_eq!(original.ticks_per_beat, reloaded.ticks_per_beat);
}

#[test]
fn midi_round_trip_preserves_tempo() {
    let original = reference_score();
    let reloaded = midi_round_trip(&original);
    assert_eq!(
        original.tempo_changes[0].us_per_beat,
        reloaded.tempo_changes[0].us_per_beat
    );
}

// ---------------------------------------------------------------------------
// Tokenizer encode on a MIDI-round-tripped Score
// ---------------------------------------------------------------------------

fn loaded_score() -> Score {
    midi_round_trip(&reference_score())
}

#[test]
fn remi_encode_from_loaded_midi_is_nonempty_and_in_range() {
    let score = loaded_score();
    let t = RemiTokenizer::new(RemiConfig::default());
    let tokens = t.encode(&score).expect("remi encode failed");
    assert!(!tokens.is_empty());
    let vs = t.vocab_size() as u32;
    assert!(tokens.iter().all(|&id| id < vs), "token out of vocab range");
}

#[test]
fn compound_encode_from_loaded_midi_is_nonempty_and_in_range() {
    let score = loaded_score();
    let t = CompoundTokenizer::new(CompoundConfig::default());
    let tokens = t.encode(&score).expect("compound encode failed");
    assert!(!tokens.is_empty());
    let vs = t.vocab_size() as u32;
    assert!(tokens.iter().all(|&id| id < vs), "token out of vocab range");
}

#[test]
fn midi_like_encode_from_loaded_midi_is_nonempty_and_in_range() {
    let score = loaded_score();
    let t = MidiLikeTokenizer::new(MidiLikeConfig::default());
    let tokens = t.encode(&score).expect("midi-like encode failed");
    assert!(!tokens.is_empty());
    let vs = t.vocab_size() as u32;
    assert!(tokens.iter().all(|&id| id < vs), "token out of vocab range");
}

#[test]
fn abc_encode_from_loaded_midi_is_nonempty_and_in_range() {
    let score = loaded_score();
    let t = AbcTokenizer::new(AbcConfig::default());
    let tokens = t.encode(&score).expect("abc encode failed");
    assert!(!tokens.is_empty());
    let vs = t.vocab_size() as u32;
    assert!(tokens.iter().all(|&id| id < vs), "token out of vocab range");
}

// ---------------------------------------------------------------------------
// encode → decode → save_midi (full pipeline)
// ---------------------------------------------------------------------------

fn pitches_from_score(score: &Score) -> Vec<u8> {
    let mut p: Vec<u8> =
        score.tracks.iter().flat_map(|t| t.notes.iter().map(|n| n.pitch)).collect();
    p.sort_unstable();
    p
}

#[test]
fn remi_full_pipeline_preserves_pitches() {
    let score = loaded_score();
    let expected = pitches_from_score(&score);

    let t = RemiTokenizer::new(RemiConfig::default());
    let tokens = t.encode(&score).unwrap();
    let decoded = t.decode(&tokens).unwrap();
    let midi_bytes = save_midi(&decoded).unwrap();
    let reloaded = load_midi(&midi_bytes).unwrap();

    assert_eq!(pitches_from_score(&reloaded), expected);
}

#[test]
fn compound_full_pipeline_preserves_pitches() {
    let score = loaded_score();
    let expected = pitches_from_score(&score);

    let t = CompoundTokenizer::new(CompoundConfig::default());
    let tokens = t.encode(&score).unwrap();
    let decoded = t.decode(&tokens).unwrap();
    let midi_bytes = save_midi(&decoded).unwrap();
    let reloaded = load_midi(&midi_bytes).unwrap();

    assert_eq!(pitches_from_score(&reloaded), expected);
}

#[test]
fn midi_like_full_pipeline_preserves_pitches() {
    let score = loaded_score();
    let expected = pitches_from_score(&score);

    let t = MidiLikeTokenizer::new(MidiLikeConfig::default());
    let tokens = t.encode(&score).unwrap();
    let decoded = t.decode(&tokens).unwrap();
    let midi_bytes = save_midi(&decoded).unwrap();
    let reloaded = load_midi(&midi_bytes).unwrap();

    assert_eq!(pitches_from_score(&reloaded), expected);
}
