# ABC Notation Tokenizer

**Module:** `crates/core/src/tokenizer/abc/`

---

## What it does

Converts a [`Score`] to [ABC notation](https://abcnotation.com/) text, then maps each character to an integer ID from a fixed 46-token vocabulary. This makes the output compatible with character-level language models and BPE tokenizers.

Unlike REMI (which produces a compact integer sequence), the ABC tokenizer produces a human-readable intermediate that preserves musical intent in a widely-understood notation.

---

## Two-stage pipeline

```
Score ──► AbcConverter::to_abc() ──► ABC string ──► char_to_id() ──► Vec<u32>
Vec<u32> ──► id_to_char() ──► ABC string ──► parser::parse_abc_score() ──► Score
```

---

## ABC output format

```
X:1
T:Untitled
M:4/4
L:1/16
Q:1/4=120
K:C
V:1
c4 e4 g4 | c'4 e'4 z4
V:2
C4 E4 G4 | ...
```

| Header | Meaning |
|--------|---------|
| `M:4/4` | Time signature from `Score.time_signature_changes[0]` |
| `L:1/16` | Unit note length; always 1/16 (a 16th note) |
| `Q:1/4=120` | Tempo in BPM from first `TempoChange` |
| `K:C` | Key from `AbcConfig.key` (sharps always explicit; key not used for pitch) |
| `V:n` | One section per track |

### Note encoding

| MIDI pitch range | ABC representation |
|------------------|--------------------|
| 48–59 (octave 4) | `C D E F G A B` (uppercase) |
| 60–71 (octave 5) | `c d e f g a b` (lowercase) |
| 72–83 (octave 6) | `c' d' …` (lowercase + `'`) |
| 36–47 (octave 3) | `C, D, …` (uppercase + `,`) |
| Sharps | `^` prefix (e.g. `^c` = C#5) |

Duration is expressed as a multiplier of the unit note length (`L:1/16`):
- `c4` = quarter note (4 × 1/16)
- `c` = sixteenth note (1 × 1/16)

Simultaneous notes (chords) are grouped with `[...]`.
Gaps between notes within a bar are filled with `z` rests.
Trailing rests at the end of a bar are **not** emitted — the parser snaps to bar boundaries on `|`.

---

## Character vocabulary (46 tokens)

| Range | Characters |
|-------|------------|
| 0 | PAD / UNK |
| 1–7 | `A B C D E F G` |
| 8–14 | `a b c d e f g` |
| 15 | `z` (rest) |
| 16–18 | `^ _ =` (accidentals) |
| 19–20 | `' ,` (octave marks) |
| 21–30 | `0–9` |
| 31–35 | `/ [ ] \| :` |
| 36–37 | space, newline |
| 38–44 | `X T M L Q K V` (header letters) |
| 45 | `%` (comment marker) |

---

## Parser design (`parser.rs`)

The decoder reconstructs a `Score` by:

1. **Header scan** — extract `M:`, `L:`, `Q:` values before the `K:` line.
2. **Body parse** — character-by-character state machine per line.
3. **Bar snapping** — on each `|`, snap `current_tick` to `bar_count × bar_ticks` to account for unfilled trailing rests in the encoder output.

See [`docs/plans/001-abc-parser.md`](../plans/001-abc-parser.md) for the full design rationale.

### Decoding is approximate

- Velocity is fixed at 64 (not encoded in ABC).
- Multi-track information is preserved (one `Track` per `V:` voice).
- Sub-grid timing is quantized to 16th-note units.
- Key signature accidentals (e.g. `K:G` implies F#) are ignored; all accidentals are explicit in the encoder output.

---

## Configuration

```rust
AbcConfig {
    title: "Untitled".into(), // written to T: header
    key: "C".into(),          // written to K: header; does not affect pitch output
    beat_resolution: 4,       // grid steps per beat (default 4 = 16th notes)
}
```

`beat_resolution` must match the resolution used when the original `Score` was created (e.g. from `RemiConfig::beat_resolution`) for tick positions to align correctly on round-trip.

---

## Usage

```rust
use notatok_core::tokenizer::abc::{AbcConfig, AbcConverter, AbcTokenizer};
use notatok_core::tokenizer::Tokenizer;

// ABC text (for inspection / BPE training)
let text = AbcConverter::new(AbcConfig::default()).to_abc(&score);

// Integer token IDs (for character-level models)
let tokenizer = AbcTokenizer::default();
let tokens = tokenizer.encode(&score)?;
let decoded_score = tokenizer.decode(&tokens)?;
```

CLI:
```bash
notatok tokenize file.mid --scheme abc
notatok tokenize file.mid --scheme abc --output tokens.json
```
