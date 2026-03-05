//! ABC notation parser: convert an ABC string back into a [`Score`].
//!
//! This parser handles the subset of ABC 2.1 produced by
//! [`super::AbcConverter::to_abc`]. It tolerates common extensions but does
//! not implement the full specification.
//!
//! # Key design decisions
//!
//! **Bar-boundary snapping:** `AbcConverter` does not emit trailing rests at
//! the end of a bar, so a bar containing only one short note looks like `c | e`
//! rather than `c z15 | e`. Without explicit snapping, `e` would be placed
//! immediately after `c` rather than at the start of the next bar. The parser
//! therefore tracks how many `|` symbols it has seen (`bar_count`) and on each
//! `|` snaps `current_tick` to `bar_count * bar_ticks`.
//!
//! **Fixed decode resolution:** Output `Score` always uses 480 ticks-per-beat,
//! matching the REMI decoder. Velocity is fixed at 64 (ABC does not encode it).

use crate::{
    midi::{Note, Score, TempoChange, TimeSignatureChange, Track},
    CoreError, Result,
};

/// Tick resolution for decoded scores (matches REMI decoder).
const TICKS_PER_BEAT: u64 = 480;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Parse an ABC notation string into a [`Score`].
///
/// Designed for the output of [`super::AbcConverter::to_abc`]. Returns
/// [`CoreError::InvalidInput`] if the input contains no `K:` header.
pub fn parse_abc_score(text: &str) -> Result<Score> {
    let mut header = HeaderInfo::default();
    let mut voices: Vec<VoiceState> = Vec::new();
    let mut current_voice: usize = 0;
    let mut in_body = false;

    for line in text.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('%') {
            continue;
        }

        if let Some((field, value)) = split_header_line(trimmed) {
            match field {
                'M' => header.parse_meter(value),
                'L' => header.parse_unit_length(value),
                'Q' => header.parse_tempo(value),
                'K' => {
                    in_body = true;
                    if voices.is_empty() {
                        voices.push(VoiceState::default());
                    }
                }
                'V' if in_body => {
                    let idx: usize =
                        value.trim().parse::<usize>().unwrap_or(1).saturating_sub(1);
                    while voices.len() <= idx {
                        voices.push(VoiceState::default());
                    }
                    current_voice = idx;
                }
                _ => {}
            }
        } else if in_body {
            if voices.is_empty() {
                voices.push(VoiceState::default());
            }
            let unit = header.unit_ticks();
            let bar_ticks = header.bar_ticks();
            parse_body_line(trimmed, &mut voices[current_voice], unit, bar_ticks)?;
        }
    }

    if !in_body {
        return Err(CoreError::InvalidInput(
            "no K: header found in ABC input".into(),
        ));
    }

    let tracks: Vec<Track> = voices
        .into_iter()
        .filter(|v| !v.notes.is_empty())
        .map(|v| {
            let mut notes = v.notes;
            notes.sort_by_key(|n| (n.start_tick, n.pitch));
            Track { notes, program: None, name: None }
        })
        .collect();

    let us_per_beat = (60_000_000.0 / header.bpm).round() as u32;

    // meter_den is stored as the actual denominator (e.g. 4 for 4/4).
    // TimeSignatureChange.denominator is 2^exp → find exp.
    let den_exp = header.meter_den.trailing_zeros() as u8;

    Ok(Score {
        tracks,
        tempo_changes: vec![TempoChange { tick: 0, us_per_beat }],
        time_signature_changes: vec![TimeSignatureChange {
            tick: 0,
            numerator: header.meter_num,
            denominator: den_exp,
        }],
        ticks_per_beat: TICKS_PER_BEAT as u16,
    })
}

// ---------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct HeaderInfo {
    meter_num: u8,
    /// Actual denominator value (not power-of-2 exponent). E.g. 4 for 4/4.
    meter_den: u32,
    /// Unit note length numerator (L: p/q).
    unit_p: u64,
    /// Unit note length denominator.
    unit_q: u64,
    bpm: f64,
}

impl Default for HeaderInfo {
    fn default() -> Self {
        Self { meter_num: 4, meter_den: 4, unit_p: 1, unit_q: 16, bpm: 120.0 }
    }
}

impl HeaderInfo {
    /// Ticks per unit note length.
    ///
    /// A whole note = 4 * `TICKS_PER_BEAT`. Unit = `unit_p/unit_q` whole notes.
    /// E.g. L:1/16 → 1 * 4 * 480 / 16 = 120 ticks.
    fn unit_ticks(&self) -> u64 {
        self.unit_p * 4 * TICKS_PER_BEAT / self.unit_q
    }

    /// Ticks per bar, computed from the time signature and unit note length.
    ///
    /// `units_per_bar = meter_num * unit_q / (meter_den * unit_p)`
    /// E.g. 4/4 with L:1/16 → 4 * 16 / (4 * 1) = 16 units/bar.
    fn bar_ticks(&self) -> u64 {
        let units_per_bar =
            self.meter_num as u64 * self.unit_q / (self.meter_den as u64 * self.unit_p);
        units_per_bar * self.unit_ticks()
    }

    fn parse_meter(&mut self, value: &str) {
        let value = value.trim();
        match value {
            "C" | "c" => {
                self.meter_num = 4;
                self.meter_den = 4;
            }
            "C|" => {
                self.meter_num = 2;
                self.meter_den = 2;
            }
            _ => {
                if let Some(pos) = value.find('/') {
                    if let (Ok(n), Ok(d)) = (
                        value[..pos].trim().parse::<u8>(),
                        value[pos + 1..].trim().parse::<u32>(),
                    ) {
                        self.meter_num = n;
                        self.meter_den = d;
                    }
                }
            }
        }
    }

    fn parse_unit_length(&mut self, value: &str) {
        // Expects "p/q", e.g. "1/16"
        let value = value.trim();
        if let Some(pos) = value.find('/') {
            if let (Ok(p), Ok(q)) = (
                value[..pos].trim().parse::<u64>(),
                value[pos + 1..].trim().parse::<u64>(),
            ) {
                self.unit_p = p;
                self.unit_q = q;
            }
        }
    }

    fn parse_tempo(&mut self, value: &str) {
        // Handles "1/4=120" or "120"
        let value = value.trim();
        let bpm_str = if let Some(eq) = value.find('=') {
            value[eq + 1..].trim()
        } else {
            value
        };
        if let Ok(bpm) = bpm_str.parse::<f64>() {
            self.bpm = bpm;
        }
    }
}

// ---------------------------------------------------------------------------
// Voice state
// ---------------------------------------------------------------------------

#[derive(Default)]
struct VoiceState {
    /// Accumulated tick position as notes/rests are parsed.
    current_tick: u64,
    /// Number of `|` bar-line tokens seen so far (used for tick snapping).
    bar_count: u64,
    notes: Vec<Note>,
}

// ---------------------------------------------------------------------------
// Body parsing
// ---------------------------------------------------------------------------

/// Parse one line of ABC body content into `voice`.
///
/// `unit` is ticks per unit note length; `bar_ticks` is ticks per bar.
fn parse_body_line(
    line: &str,
    voice: &mut VoiceState,
    unit: u64,
    bar_ticks: u64,
) -> Result<()> {
    let mut p = BodyParser::new(line, unit);

    while let Some(c) = p.peek() {
        match c {
            // Bar line — snap current_tick to next bar boundary
            '|' => {
                p.advance();
                if bar_ticks > 0 {
                    voice.bar_count += 1;
                    voice.current_tick = voice.bar_count * bar_ticks;
                }
            }

            // Repeat / section markers that contain ':'
            ':' => {
                p.advance();
            }

            // Whitespace
            ' ' | '\t' | '\r' => {
                p.advance();
            }

            // Comment — skip rest of line
            '%' => break,

            // Chord: [note note ...]duration
            '[' => {
                p.advance(); // consume '['
                let mut chord_pitches: Vec<u8> = Vec::new();

                while p.peek().map_or(false, |ch| ch != ']') {
                    // Skip spaces within chord
                    if matches!(p.peek(), Some(' ') | Some('\t')) {
                        p.advance();
                        continue;
                    }
                    let acc = p.parse_accidental();
                    match p.peek() {
                        Some(l) if is_note_letter(l) => {
                            p.advance();
                            let oct = p.parse_octave_shift();
                            if let Some(midi) = note_to_midi(l, acc, oct) {
                                chord_pitches.push(midi);
                            }
                        }
                        Some('z') | Some('Z') => {
                            p.advance();
                            p.parse_octave_shift();
                        }
                        _ => {
                            p.advance();
                        }
                    }
                }
                if p.peek() == Some(']') {
                    p.advance();
                }
                let dur = p.parse_duration();
                for pitch in chord_pitches {
                    voice.notes.push(Note {
                        pitch,
                        velocity: 64,
                        start_tick: voice.current_tick,
                        duration_ticks: dur,
                        channel: 0,
                    });
                }
                voice.current_tick += dur;
            }

            // Accidental prefix — followed by a note letter
            '^' | '_' | '=' => {
                let acc = p.parse_accidental();
                if let Some(l) = p.peek().filter(|&ch| is_note_letter(ch)) {
                    p.advance();
                    let oct = p.parse_octave_shift();
                    let dur = p.parse_duration();
                    if let Some(midi) = note_to_midi(l, acc, oct) {
                        voice.notes.push(Note {
                            pitch: midi,
                            velocity: 64,
                            start_tick: voice.current_tick,
                            duration_ticks: dur,
                            channel: 0,
                        });
                    }
                    voice.current_tick += dur;
                }
            }

            // Note letter
            l if is_note_letter(l) => {
                p.advance();
                let oct = p.parse_octave_shift();
                let dur = p.parse_duration();
                if let Some(midi) = note_to_midi(l, 0, oct) {
                    voice.notes.push(Note {
                        pitch: midi,
                        velocity: 64,
                        start_tick: voice.current_tick,
                        duration_ticks: dur,
                        channel: 0,
                    });
                }
                voice.current_tick += dur;
            }

            // Rest
            'z' | 'Z' => {
                p.advance();
                p.parse_octave_shift();
                let dur = p.parse_duration();
                voice.current_tick += dur;
            }

            // Anything else (unknown characters, numbers outside duration context)
            _ => {
                p.advance();
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// BodyParser — character-by-character helper
// ---------------------------------------------------------------------------

struct BodyParser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    unit: u64,
}

impl<'a> BodyParser<'a> {
    fn new(s: &'a str, unit: u64) -> Self {
        Self { chars: s.chars().peekable(), unit }
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn advance(&mut self) {
        self.chars.next();
    }

    /// Consume a run of accidental characters and return the net semitone offset.
    /// Multiple `^` accumulate; `=` resets to 0.
    fn parse_accidental(&mut self) -> i32 {
        let mut acc = 0i32;
        loop {
            match self.peek() {
                Some('^') => {
                    self.advance();
                    acc += 1;
                }
                Some('_') => {
                    self.advance();
                    acc -= 1;
                }
                Some('=') => {
                    self.advance();
                    acc = 0;
                }
                _ => break,
            }
        }
        acc
    }

    /// Consume octave-shift marks (`'` = +1, `,` = -1) and return net shift.
    fn parse_octave_shift(&mut self) -> i32 {
        let mut shift = 0i32;
        loop {
            match self.peek() {
                Some('\'') => {
                    self.advance();
                    shift += 1;
                }
                Some(',') => {
                    self.advance();
                    shift -= 1;
                }
                _ => break,
            }
        }
        shift
    }

    /// Parse a duration modifier and return the tick duration.
    ///
    /// Formats:
    /// - `4`   → `unit * 4`
    /// - `/2`  → `unit / 2`
    /// - `/`   → `unit / 2`   (bare slash = ÷2)
    /// - `3/2` → `unit * 3 / 2`
    /// - `` (none) → `unit`
    fn parse_duration(&mut self) -> u64 {
        let num: u64 = if matches!(self.peek(), Some('0'..='9')) {
            let mut n = 0u64;
            while let Some(d) = self.peek().and_then(|c| c.to_digit(10)) {
                self.advance();
                n = n * 10 + d as u64;
            }
            n
        } else {
            1
        };

        let den: u64 = if self.peek() == Some('/') {
            self.advance();
            if matches!(self.peek(), Some('0'..='9')) {
                let mut d = 0u64;
                while let Some(digit) = self.peek().and_then(|c| c.to_digit(10)) {
                    self.advance();
                    d = d * 10 + digit as u64;
                }
                d
            } else {
                2 // bare '/' → divide by 2
            }
        } else {
            1
        };

        self.unit * num / den.max(1)
    }
}

// ---------------------------------------------------------------------------
// Pitch helpers
// ---------------------------------------------------------------------------

/// Return the semitone offset from C for a note letter (case-insensitive).
fn semitone_of(letter: char) -> Option<i32> {
    match letter.to_ascii_uppercase() {
        'C' => Some(0),
        'D' => Some(2),
        'E' => Some(4),
        'F' => Some(5),
        'G' => Some(7),
        'A' => Some(9),
        'B' => Some(11),
        _ => None,
    }
}

fn is_note_letter(c: char) -> bool {
    matches!(c, 'A'..='G' | 'a'..='g')
}

/// Convert a parsed note to its MIDI pitch.
///
/// - Uppercase letters → base octave 4 (C4 = MIDI 48)
/// - Lowercase letters → base octave 5 (C5 = MIDI 60)
/// - `octave_shift` from `'` / `,` marks
/// - `accidental` net semitone offset from `^` / `_` / `=`
///
/// Returns `None` for out-of-range pitches (silently dropped).
fn note_to_midi(letter: char, accidental: i32, octave_shift: i32) -> Option<u8> {
    let semitone = semitone_of(letter)?;
    let base_octave = if letter.is_uppercase() { 4i32 } else { 5i32 };
    let octave = base_octave + octave_shift;
    let midi = octave * 12 + semitone + accidental;
    if (0..=127).contains(&midi) {
        Some(midi as u8)
    } else {
        None
    }
}

/// If `line` is an ABC header field (`"F:value"`), return `(field_char, value)`.
///
/// Only matches lines where the second character is `:` to avoid false positives
/// on note lines like `C4 E4`.
fn split_header_line(line: &str) -> Option<(char, &str)> {
    let mut chars = line.chars();
    let field = chars.next()?;
    if !field.is_ascii_alphabetic() {
        return None;
    }
    if chars.next()? != ':' {
        return None;
    }
    Some((field, &line[2..]))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_abc(voice_body: &str) -> String {
        format!(
            "X:1\nT:Test\nM:4/4\nL:1/16\nQ:1/4=120\nK:C\nV:1\n{voice_body}"
        )
    }

    #[test]
    fn parses_single_quarter_note_c5() {
        // L:1/16, c4 = 4 units = 4*120 = 480 ticks = one quarter note
        let score = parse_abc_score(&minimal_abc("c4")).unwrap();
        assert_eq!(score.tracks.len(), 1);
        let notes = &score.tracks[0].notes;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].pitch, 60); // C5
        assert_eq!(notes[0].start_tick, 0);
        assert_eq!(notes[0].duration_ticks, 480);
    }

    #[test]
    fn rest_advances_tick_without_note() {
        // z4 = 480 ticks rest, then c4 starts at 480
        let score = parse_abc_score(&minimal_abc("z4 c4")).unwrap();
        let notes = &score.tracks[0].notes;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].start_tick, 480);
    }

    #[test]
    fn bar_line_snaps_to_bar_boundary() {
        // 4/4 L:1/16 → bar_ticks = 16 * 120 = 1920
        // c (1 unit = 120 ticks), then |, then d should start at 1920
        let score = parse_abc_score(&minimal_abc("c | d")).unwrap();
        let notes = &score.tracks[0].notes;
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].start_tick, 0);
        assert_eq!(notes[1].start_tick, 1920); // snapped to bar 1
    }

    #[test]
    fn sharp_accidental() {
        // ^c = C#5 = MIDI 61
        let score = parse_abc_score(&minimal_abc("^c4")).unwrap();
        assert_eq!(score.tracks[0].notes[0].pitch, 61);
    }

    #[test]
    fn flat_accidental() {
        // _e = Eb5 = MIDI 63
        let score = parse_abc_score(&minimal_abc("_e4")).unwrap();
        assert_eq!(score.tracks[0].notes[0].pitch, 63);
    }

    #[test]
    fn octave_up_mark() {
        // c' = C6 = MIDI 72
        let score = parse_abc_score(&minimal_abc("c'4")).unwrap();
        assert_eq!(score.tracks[0].notes[0].pitch, 72);
    }

    #[test]
    fn octave_down_uppercase() {
        // C, = C3 = MIDI 36
        let score = parse_abc_score(&minimal_abc("C,4")).unwrap();
        assert_eq!(score.tracks[0].notes[0].pitch, 36);
    }

    #[test]
    fn chord_notes_share_start_tick() {
        // [ceg]4 → three notes at tick 0
        let score = parse_abc_score(&minimal_abc("[ceg]4")).unwrap();
        let notes = &score.tracks[0].notes;
        assert_eq!(notes.len(), 3);
        assert!(notes.iter().all(|n| n.start_tick == 0));
        assert_eq!(notes.iter().map(|n| n.pitch).collect::<Vec<_>>(), vec![60, 64, 67]);
    }

    #[test]
    fn chord_advances_tick_by_duration() {
        // [ceg]4 d4 → d starts at 480
        let score = parse_abc_score(&minimal_abc("[ceg]4 d4")).unwrap();
        let d = score.tracks[0].notes.iter().find(|n| n.pitch == 62).unwrap();
        assert_eq!(d.start_tick, 480);
    }

    #[test]
    fn fractional_duration_slash() {
        // c/ = 1/2 unit = 60 ticks
        let score = parse_abc_score(&minimal_abc("c/")).unwrap();
        assert_eq!(score.tracks[0].notes[0].duration_ticks, 60);
    }

    #[test]
    fn dotted_duration() {
        // c3/2 = 3/2 * 120 = 180 ticks
        let score = parse_abc_score(&minimal_abc("c3/2")).unwrap();
        assert_eq!(score.tracks[0].notes[0].duration_ticks, 180);
    }

    #[test]
    fn multivoice_produces_separate_tracks() {
        let abc = "X:1\nT:T\nM:4/4\nL:1/16\nQ:1/4=120\nK:C\nV:1\nc4\nV:2\nG4\n";
        let score = parse_abc_score(abc).unwrap();
        assert_eq!(score.tracks.len(), 2);
        assert_eq!(score.tracks[0].notes[0].pitch, 60); // C5
        assert_eq!(score.tracks[1].notes[0].pitch, 55); // G4 (uppercase = octave 4)
    }

    #[test]
    fn tempo_round_trips_from_header() {
        // Q:1/4=90 → us_per_beat = 60_000_000/90 = 666_667
        let abc = "X:1\nT:T\nM:4/4\nL:1/16\nQ:1/4=90\nK:C\nc4\n";
        let score = parse_abc_score(abc).unwrap();
        let us = score.tempo_changes[0].us_per_beat;
        assert!((us as i64 - 666_667).abs() <= 1, "got {us}");
    }

    #[test]
    fn no_k_header_returns_error() {
        let result = parse_abc_score("X:1\nM:4/4\n");
        assert!(result.is_err());
    }

    #[test]
    fn uppercase_c_is_midi_48() {
        // C (uppercase, no marks) = C4 = MIDI 48
        let score = parse_abc_score(&minimal_abc("C4")).unwrap();
        assert_eq!(score.tracks[0].notes[0].pitch, 48);
    }

    #[test]
    fn roundtrip_c_major_scale() {
        use crate::midi::{Note, TempoChange, TimeSignatureChange, Track};
        use crate::tokenizer::abc::{AbcConfig, AbcConverter};

        let notes: Vec<Note> = [60u8, 62, 64, 65, 67, 69, 71, 72]
            .iter()
            .enumerate()
            .map(|(i, &p)| Note {
                pitch: p,
                velocity: 80,
                start_tick: i as u64 * 480,
                duration_ticks: 480,
                channel: 0,
            })
            .collect();

        let original = Score {
            tracks: vec![Track { notes: notes.clone(), program: None, name: None }],
            tempo_changes: vec![TempoChange { tick: 0, us_per_beat: 500_000 }],
            time_signature_changes: vec![TimeSignatureChange {
                tick: 0,
                numerator: 4,
                denominator: 2,
            }],
            ticks_per_beat: 480,
        };

        let abc_text = AbcConverter::new(AbcConfig::default()).to_abc(&original);
        let decoded = parse_abc_score(&abc_text).unwrap();

        let decoded_pitches: Vec<u8> =
            decoded.tracks.iter().flat_map(|t| t.notes.iter().map(|n| n.pitch)).collect();
        let original_pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
        assert_eq!(decoded_pitches, original_pitches, "abc:\n{abc_text}");
    }
}
