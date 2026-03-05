//! ABC notation tokenization scheme.
//!
//! Primary API: [`AbcConverter::to_abc`] — converts a [`Score`] to an ABC notation string.
//!
//! Secondary API: [`AbcTokenizer`] — implements [`Tokenizer`] with a fixed 46-character
//! vocabulary so that ABC text can be passed through a character-level model or BPE tokenizer.

pub mod parser;

use crate::{
    midi::{Score, TimeSignatureChange, Track},
    tokenizer::Tokenizer,
    Result,
};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Character vocabulary  (46 tokens, IDs 0–45)
// ---------------------------------------------------------------------------

/// Total number of tokens in the ABC character vocabulary.
pub const VOCAB_SIZE: usize = 46;

/// Map an ABC character to its vocabulary ID.  Returns 0 for unknown characters.
///
/// Layout:
/// ```text
/// 0        PAD / UNK
/// 1– 7     A B C D E F G     (uppercase note names)
/// 8–14     a b c d e f g     (lowercase note names)
/// 15       z                 (rest)
/// 16       ^                 (sharp)
/// 17       _                 (flat)
/// 18       =                 (natural / key-value separator in headers)
/// 19       '                 (octave up mark)
/// 20       ,                 (octave down mark)
/// 21–30    0 1 2 3 4 5 6 7 8 9
/// 31       /
/// 32       [
/// 33       ]
/// 34       |
/// 35       :
/// 36       ' ' (space)
/// 37       \n
/// 38–44    X T M L Q K V    (header field letters)
/// 45       %
/// ```
pub fn char_to_id(c: char) -> u32 {
    match c {
        'A' => 1,
        'B' => 2,
        'C' => 3,
        'D' => 4,
        'E' => 5,
        'F' => 6,
        'G' => 7,
        'a' => 8,
        'b' => 9,
        'c' => 10,
        'd' => 11,
        'e' => 12,
        'f' => 13,
        'g' => 14,
        'z' => 15,
        '^' => 16,
        '_' => 17,
        '=' => 18,
        '\'' => 19,
        ',' => 20,
        '0' => 21,
        '1' => 22,
        '2' => 23,
        '3' => 24,
        '4' => 25,
        '5' => 26,
        '6' => 27,
        '7' => 28,
        '8' => 29,
        '9' => 30,
        '/' => 31,
        '[' => 32,
        ']' => 33,
        '|' => 34,
        ':' => 35,
        ' ' => 36,
        '\n' => 37,
        'X' => 38,
        'T' => 39,
        'M' => 40,
        'L' => 41,
        'Q' => 42,
        'K' => 43,
        'V' => 44,
        '%' => 45,
        _ => 0,
    }
}

/// Map a vocabulary ID back to its ABC character.  Returns `None` for ID 0 (UNK/PAD).
pub fn id_to_char(id: u32) -> Option<char> {
    match id {
        1 => Some('A'),
        2 => Some('B'),
        3 => Some('C'),
        4 => Some('D'),
        5 => Some('E'),
        6 => Some('F'),
        7 => Some('G'),
        8 => Some('a'),
        9 => Some('b'),
        10 => Some('c'),
        11 => Some('d'),
        12 => Some('e'),
        13 => Some('f'),
        14 => Some('g'),
        15 => Some('z'),
        16 => Some('^'),
        17 => Some('_'),
        18 => Some('='),
        19 => Some('\''),
        20 => Some(','),
        21 => Some('0'),
        22 => Some('1'),
        23 => Some('2'),
        24 => Some('3'),
        25 => Some('4'),
        26 => Some('5'),
        27 => Some('6'),
        28 => Some('7'),
        29 => Some('8'),
        30 => Some('9'),
        31 => Some('/'),
        32 => Some('['),
        33 => Some(']'),
        34 => Some('|'),
        35 => Some(':'),
        36 => Some(' '),
        37 => Some('\n'),
        38 => Some('X'),
        39 => Some('T'),
        40 => Some('M'),
        41 => Some('L'),
        42 => Some('Q'),
        43 => Some('K'),
        44 => Some('V'),
        45 => Some('%'),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Pitch helpers
// ---------------------------------------------------------------------------

/// Convert a MIDI pitch (0–127) to its ABC notation string.
///
/// Convention (`midi / 12` = octave index):
/// - Octave 5 (MIDI 60–71): lowercase, no marks  → `c d e … b`  (middle C = `c`)
/// - Octave 4 (MIDI 48–59): uppercase, no marks  → `C D E … B`
/// - Octave 6 (MIDI 72–83): lowercase + `'`      → `c' d' …`
/// - Octave 3 (MIDI 36–47): uppercase + `,`      → `C, D, …`
/// - Sharps use `^` prefix; no flat spellings are emitted.
pub fn midi_to_abc_pitch(midi: u8) -> String {
    // (base_name, is_sharp) indexed by semitone 0–11
    const SEMITONES: [(&str, bool); 12] = [
        ("C", false),
        ("C", true),
        ("D", false),
        ("D", true),
        ("E", false),
        ("F", false),
        ("F", true),
        ("G", false),
        ("G", true),
        ("A", false),
        ("A", true),
        ("B", false),
    ];

    let octave = (midi / 12) as i32;
    let (base, is_sharp) = SEMITONES[(midi % 12) as usize];
    let acc = if is_sharp { "^" } else { "" };

    match octave.cmp(&5) {
        std::cmp::Ordering::Equal => {
            format!("{}{}", acc, base.to_lowercase())
        }
        std::cmp::Ordering::Less => {
            let commas = if octave < 4 { (4 - octave) as usize } else { 0 };
            format!("{}{}{}", acc, base, ",".repeat(commas))
        }
        std::cmp::Ordering::Greater => {
            let apostrophes = (octave - 5) as usize;
            format!("{}{}{}", acc, base.to_lowercase(), "'".repeat(apostrophes))
        }
    }
}

// ---------------------------------------------------------------------------
// AbcConfig
// ---------------------------------------------------------------------------

/// Configuration for the ABC notation converter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbcConfig {
    /// Title written to the `T:` header field.
    pub title: String,
    /// Key signature written to the `K:` header field.
    /// All accidentals in note output use sharps regardless of key.
    pub key: String,
    /// Number of 16th-note grid steps per beat.  Must divide `ticks_per_beat` evenly.
    /// Default 4 matches REMI's beat resolution.
    pub beat_resolution: u8,
}

impl Default for AbcConfig {
    fn default() -> Self {
        Self {
            title: "Untitled".into(),
            key: "C".into(),
            beat_resolution: 4,
        }
    }
}

// ---------------------------------------------------------------------------
// AbcConverter
// ---------------------------------------------------------------------------

/// Converts a [`Score`] into ABC notation text.
///
/// The output contains a standard header followed by one `V:` voice section per track.
/// Unit note length is always `L:1/16`.
pub struct AbcConverter {
    pub config: AbcConfig,
}

impl AbcConverter {
    pub fn new(config: AbcConfig) -> Self {
        Self { config }
    }

    /// Convert a [`Score`] to an ABC notation string.
    pub fn to_abc(&self, score: &Score) -> String {
        let ts = score
            .time_signature_changes
            .first()
            .copied()
            .unwrap_or(TimeSignatureChange { tick: 0, numerator: 4, denominator: 2 });
        let actual_den = 1u32 << ts.denominator as u32;

        let bpm = score
            .tempo_changes
            .first()
            .map(|tc| 60_000_000u32 / tc.us_per_beat)
            .unwrap_or(120);

        let mut out = String::new();
        out.push_str("X:1\n");
        out.push_str(&format!("T:{}\n", self.config.title));
        out.push_str(&format!("M:{}/{}\n", ts.numerator, actual_den));
        out.push_str("L:1/16\n");
        out.push_str(&format!("Q:1/4={}\n", bpm));
        out.push_str(&format!("K:{}\n", self.config.key));

        for (i, track) in score.tracks.iter().enumerate() {
            out.push_str(&format!("V:{}\n", i + 1));
            let voice = render_voice(track, score, &self.config);
            if !voice.is_empty() {
                out.push_str(&voice);
                out.push('\n');
            }
        }

        out
    }
}

// ---------------------------------------------------------------------------
// render_voice  (private)
// ---------------------------------------------------------------------------

fn render_voice(track: &Track, score: &Score, config: &AbcConfig) -> String {
    if track.notes.is_empty() {
        return String::new();
    }

    let tpb = score.ticks_per_beat as u64;
    let ticks_per_unit = (tpb / config.beat_resolution as u64).max(1);
    let ts = score
        .time_signature_changes
        .first()
        .copied()
        .unwrap_or(TimeSignatureChange { tick: 0, numerator: 4, denominator: 2 });
    let units_per_bar = ts.numerator as u64 * config.beat_resolution as u64;

    // Build (bar_idx, pos_in_bar, dur_units, pitch_str) for every note.
    // We sort by bar → position → pitch_str for a deterministic, readable output.
    let mut entries: Vec<(u64, u64, u64, String)> = track
        .notes
        .iter()
        .map(|n| {
            let start = n.start_tick / ticks_per_unit;
            let dur = (n.duration_ticks / ticks_per_unit).max(1);
            let bar = start / units_per_bar;
            let pos = start % units_per_bar;
            (bar, pos, dur, midi_to_abc_pitch(n.pitch))
        })
        .collect();

    entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.3.cmp(&b.3)));

    let max_bar = entries.last().map(|e| e.0).unwrap_or(0);
    let mut bars: Vec<String> = Vec::new();
    let mut idx = 0;

    for bar_idx in 0..=max_bar {
        let mut bar_str = String::new();
        let mut pos: u64 = 0;

        while idx < entries.len() && entries[idx].0 == bar_idx {
            let note_pos = entries[idx].1;

            // Fill any gap before this note group with a rest.
            if pos < note_pos {
                let gap = note_pos - pos;
                if gap == 1 {
                    bar_str.push_str("z ");
                } else {
                    bar_str.push_str(&format!("z{} ", gap));
                }
                pos = note_pos;
            }

            // Collect all notes at the same (bar_idx, pos) — they form a chord.
            let j = entries[idx..]
                .partition_point(|e| e.0 == bar_idx && e.1 == note_pos);
            let group = &entries[idx..idx + j];
            let min_dur = group.iter().map(|e| e.2).min().unwrap_or(1);
            let dur_str = if min_dur == 1 { String::new() } else { min_dur.to_string() };

            if group.len() == 1 {
                bar_str.push_str(&group[0].3);
                bar_str.push_str(&dur_str);
                bar_str.push(' ');
            } else {
                bar_str.push('[');
                for e in group {
                    bar_str.push_str(&e.3);
                }
                bar_str.push(']');
                bar_str.push_str(&dur_str);
                bar_str.push(' ');
            }

            pos += min_dur;
            idx += j;
        }

        bars.push(bar_str.trim_end().to_string());
    }

    bars.join(" | ")
}

// ---------------------------------------------------------------------------
// AbcTokenizer
// ---------------------------------------------------------------------------

/// Tokenizes a [`Score`] via ABC notation with a fixed 46-character vocabulary.
///
/// Encoding: `Score` → ABC text → `Vec<u32>` (one ID per character).
/// Decoding: `Vec<u32>` → ABC text → `Score` (approximate reconstruction).
///
/// Characters not in the vocabulary map to ID 0 (UNK) during encoding.
/// ID 0 is filtered out during decoding, so UNK tokens are silently dropped.
pub struct AbcTokenizer {
    converter: AbcConverter,
}

impl AbcTokenizer {
    pub fn new(config: AbcConfig) -> Self {
        Self { converter: AbcConverter::new(config) }
    }
}

impl Default for AbcTokenizer {
    fn default() -> Self {
        Self::new(AbcConfig::default())
    }
}

impl Tokenizer for AbcTokenizer {
    fn encode(&self, score: &Score) -> Result<Vec<u32>> {
        let text = self.converter.to_abc(score);
        Ok(text.chars().map(char_to_id).collect())
    }

    fn decode(&self, tokens: &[u32]) -> Result<Score> {
        let text: String = tokens.iter().filter_map(|&id| id_to_char(id)).collect();
        parser::parse_abc_score(&text)
    }

    fn vocab_size(&self) -> usize {
        VOCAB_SIZE
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

    #[test]
    fn empty_score_produces_header_only() {
        let score = Score {
            tracks: vec![],
            tempo_changes: vec![TempoChange { tick: 0, us_per_beat: 500_000 }],
            time_signature_changes: vec![TimeSignatureChange {
                tick: 0,
                numerator: 4,
                denominator: 2,
            }],
            ticks_per_beat: 480,
        };
        let abc = AbcConverter::new(AbcConfig::default()).to_abc(&score);
        assert!(abc.contains("X:1"), "missing X:1 in: {abc}");
        assert!(abc.contains("M:4/4"), "missing M:4/4 in: {abc}");
        assert!(abc.contains("L:1/16"), "missing L:1/16 in: {abc}");
        assert!(!abc.contains("V:"), "unexpected V: in empty score: {abc}");
    }

    #[test]
    fn c4_quarter_produces_lowercase_c4() {
        // MIDI 60 = octave 5 → lowercase 'c'.
        // Quarter note = 480 ticks; ticks_per_unit = 480/4 = 120 → 4 units → "c4".
        let score = make_score(vec![Note {
            pitch: 60,
            velocity: 80,
            start_tick: 0,
            duration_ticks: 480,
            channel: 0,
        }]);
        let abc = AbcConverter::new(AbcConfig::default()).to_abc(&score);
        assert!(abc.contains("c4"), "expected 'c4' in:\n{abc}");
    }

    #[test]
    fn rest_fills_gap_between_notes() {
        // ticks_per_unit = 480/4 = 120.
        // Note 1 ends at tick 120 (1 unit).  Note 2 starts at tick 360 (3 units).
        // Gap = 3 - 1 = 2 units → "z2".
        let score = make_score(vec![
            Note { pitch: 60, velocity: 80, start_tick: 0, duration_ticks: 120, channel: 0 },
            Note { pitch: 62, velocity: 80, start_tick: 360, duration_ticks: 120, channel: 0 },
        ]);
        let abc = AbcConverter::new(AbcConfig::default()).to_abc(&score);
        assert!(abc.contains("z2"), "expected 'z2' in:\n{abc}");
    }

    #[test]
    fn simultaneous_notes_form_chord() {
        let score = make_score(vec![
            Note { pitch: 60, velocity: 80, start_tick: 0, duration_ticks: 480, channel: 0 },
            Note { pitch: 64, velocity: 80, start_tick: 0, duration_ticks: 480, channel: 0 },
        ]);
        let abc = AbcConverter::new(AbcConfig::default()).to_abc(&score);
        assert!(abc.contains('['), "expected chord '[' in:\n{abc}");
        assert!(abc.contains(']'), "expected chord ']' in:\n{abc}");
    }

    #[test]
    fn bar_line_at_correct_boundary() {
        // 4/4 at beat_resolution=4 → units_per_bar = 16.
        // Bar boundary at unit 16 = 1920 ticks (with tpb=480, tpu=120).
        // Two notes in different bars → "|" in output.
        let score = make_score(vec![
            Note { pitch: 60, velocity: 80, start_tick: 0, duration_ticks: 120, channel: 0 },
            Note { pitch: 62, velocity: 80, start_tick: 1920, duration_ticks: 120, channel: 0 },
        ]);
        let abc = AbcConverter::new(AbcConfig::default()).to_abc(&score);
        assert!(abc.contains('|'), "expected bar line '|' in:\n{abc}");
    }

    #[test]
    fn encode_decode_roundtrip_preserves_pitches() {
        let notes = vec![
            Note { pitch: 60, velocity: 80, start_tick: 0, duration_ticks: 480, channel: 0 },
            Note { pitch: 64, velocity: 80, start_tick: 480, duration_ticks: 480, channel: 0 },
            Note { pitch: 67, velocity: 80, start_tick: 960, duration_ticks: 480, channel: 0 },
        ];
        let score = make_score(notes.clone());
        let tokenizer = AbcTokenizer::default();
        let tokens = tokenizer.encode(&score).unwrap();
        let decoded = tokenizer.decode(&tokens).unwrap();

        let decoded_pitches: Vec<u8> =
            decoded.tracks.iter().flat_map(|t| t.notes.iter().map(|n| n.pitch)).collect();
        let original_pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
        assert_eq!(decoded_pitches, original_pitches);
    }

    #[test]
    fn all_char_token_ids_within_vocab() {
        let score = make_score(vec![
            Note { pitch: 60, velocity: 80, start_tick: 0, duration_ticks: 480, channel: 0 },
            Note { pitch: 72, velocity: 80, start_tick: 480, duration_ticks: 240, channel: 0 },
            Note { pitch: 49, velocity: 80, start_tick: 720, duration_ticks: 120, channel: 0 },
        ]);
        let tokenizer = AbcTokenizer::default();
        let tokens = tokenizer.encode(&score).unwrap();
        for &id in &tokens {
            assert!(
                id < VOCAB_SIZE as u32,
                "token id {id} >= VOCAB_SIZE {VOCAB_SIZE}"
            );
        }
    }
}
