# REMI Tokenizer

**Module:** `crates/core/src/tokenizer/remi/`

REMI (Revamped MIDI-derived events) was introduced by Huang & Yang in
["Pop Music Transformer: Beat-based Modeling and Generation of Expressive Pop Piano Compositions"](https://arxiv.org/abs/2002.00212) (ACM MM 2020).
It is the most widely-used symbolic music tokenization scheme in the research
literature and the primary tokenizer in notatok.

---

## Key Idea

REMI organises music onto a **bar-and-beat grid**. Time is expressed as a
`Bar` marker followed by a `Position` index within that bar, rather than as a
raw millisecond offset. This gives the model an explicit metrical structure to
learn from, at the cost of quantising sub-grid timing.

---

## Token types

| Token | Meaning |
|-------|---------|
| `Bar` | Marks the start of the next bar (0-indexed internally, but the token is a single sentinel) |
| `Position(p)` | Grid step within the current bar (0-indexed, 0..positions_per_bar) |
| `Pitch(n)` | MIDI pitch of the note being described |
| `Velocity(b)` | Binned velocity index (0..velocity_bins−1) |
| `Duration(d)` | Note duration in position units (1..=max_duration) |
| `Tempo(t)` | *(optional)* Binned BPM index, emitted before the first note in each group |

Each note is encoded as a fixed-width event sequence within a bar:

```
Bar  Position  [Tempo]  Pitch  Velocity  Duration
```

`Tempo` is only present when `use_tempo_tokens = true`.

---

## Vocabulary

Default configuration (`beat_resolution` = 4, `pitch_range` = 0–127,
`velocity_bins` = 32, `max_duration` = 64, `use_tempo_tokens` = false):

```
0          → Bar
1  – 16    → Position(0)  – Position(15)     [4 steps/beat × 4 beats/bar]
17 – 144   → Pitch(0)     – Pitch(127)
145 – 176  → Velocity(0)  – Velocity(31)
177 – 240  → Duration(1)  – Duration(64)
```

Total: **241 tokens**.

With `use_tempo_tokens = true` (default 32 BPM bins): 241 + 32 = **273 tokens**.

The vocab size formula is:

```
1 + positions_per_bar + n_pitches + velocity_bins + max_duration [+ tempo_bins]
```

---

## Configuration

```rust
use notatok_core::tokenizer::remi::RemiConfig;

let config = RemiConfig {
    beat_resolution: 4,      // grid positions per beat; 4 → 16th-note grid in 4/4
    velocity_bins: 32,
    max_duration: 64,        // max note length in position units
    pitch_range: (21, 108),  // piano range
    use_tempo_tokens: true,
    tempo_bins: 32,
    tempo_min_bpm: 60.0,
    tempo_max_bpm: 240.0,
};
```

`positions_per_bar = beat_resolution × time_signature_numerator`.
For 4/4 with `beat_resolution = 4` this gives 16 positions/bar.

---

## Encoding

1. All notes across all tracks are collected and filtered by `pitch_range`.
2. Bar boundaries are precomputed from the score's time-signature map (honours
   time-signature changes mid-piece).
3. Each note is quantised:
   - **Bar index** — binary search for the bar containing `note.start_tick`.
   - **Position** — `floor((tick_in_bar) / ticks_per_pos)`, clamped to
     `[0, positions_per_bar − 1]`.
   - **Duration** — `floor(duration_ticks / ticks_per_pos)`, clamped to
     `[1, max_duration]`.
   - **Velocity** — linearly binned: `floor(velocity × velocity_bins / 128)`.
4. Notes are sorted by `(bar_idx, position, pitch)` for deterministic output.
5. The token stream is emitted: a `Bar` token is emitted when the bar index
   changes, then `Position → [Tempo] → Pitch → Velocity → Duration` per note.

### Example token sequence

A C-major chord (C4, E4, G4) on beat 1 of bar 0 at 120 BPM, each a quarter
note long, default config:

```
Bar
Position(0)
Pitch(60)   Velocity(16)   Duration(4)
Position(0)
Pitch(64)   Velocity(16)   Duration(4)
Position(0)
Pitch(67)   Velocity(16)   Duration(4)
```

(No `Tempo` token because `use_tempo_tokens` defaults to false.)

---

## Decoding

Decoding uses a fixed reference of 480 ticks/beat and assumes 4/4 time.

State machine over the token stream:
- `Bar` — increment bar counter, reset position and pending note state.
- `Position(p)` — set position within current bar; clear pending note.
- `Tempo(t)` — unbin BPM and emit a `TempoChange` if it differs from the previous one.
- `Pitch(p)` — store as pending pitch.
- `Velocity(v)` — store as pending velocity bin.
- `Duration(d)` — if both pitch and velocity are pending, emit a `Note`:
  `start_tick = bar × ticks_per_bar + position × ticks_per_pos`,
  `duration_ticks = d × ticks_per_pos`.

The result is a single merged track. Multi-track information is not encoded in
REMI and is therefore not preserved through a decode round-trip.

### What is lost on decode

| Information | Loss |
|-------------|------|
| Sub-grid timing | Quantised to `ticks_per_pos` |
| Velocity | Binned (32 bins by default) |
| Multi-track | Merged to one track |
| Time signature | Fixed to 4/4 in decoder |
| Tempo | Preserved if `use_tempo_tokens = true`; otherwise fixed at 120 BPM |

---

## Usage

### Rust

```rust
use notatok_core::midi::load_midi;
use notatok_core::tokenizer::remi::{RemiConfig, RemiTokenizer};
use notatok_core::tokenizer::Tokenizer;

let bytes = std::fs::read("song.mid")?;
let score = load_midi(&bytes)?;
let tokenizer = RemiTokenizer::new(RemiConfig::default());

let tokens = tokenizer.encode(&score)?;               // Vec<u32>
println!("vocab_size={}, tokens={}", tokenizer.vocab_size(), tokens.len());

let reconstructed = tokenizer.decode(&tokens)?;       // Score (lossy)
```

### CLI

```bash
notatok tokenize song.mid --scheme remi
notatok tokenize song.mid --scheme remi --output tokens.json
```

---

## Comparison with MIDI-Like

| Property | REMI | MIDI-Like |
|----------|------|-----------|
| Time model | Bar + beat grid | Milliseconds (TimeShift tokens) |
| Note encoding | `Pitch + Duration` | `NoteOn + NoteOff` pair |
| Sequence length | Shorter (one event per note) | ~2× longer (two events per note) |
| Polyphony | Implicit (same position) | Natural (open/close pairs) |
| Metrical structure | Explicit | Not encoded |
| Tempo encoding | Optional `Tempo` token | Implicit in TimeShift step size |
