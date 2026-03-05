# Compound Word (CP) Tokenizer

Implementation: `crates/core/src/tokenizer/compound/mod.rs`

Based on Hsiao et al. 2021, "Compound Word Transformer."

## Key Idea

REMI uses a single flat `Position` index (0..positions_per_bar−1) to locate a
note within a bar. CP splits this into two fields:

- **Beat** — which beat within the bar (0..max_beats−1)
- **SubPosition** — which grid slot within that beat (0..beat_resolution−1)

This gives downstream sequence models separate embedding dimensions for
beat-level and sub-beat timing, which is the paper's core architectural
contribution.

## Sequence Format

```
Bar [Beat SubPos Pitch Vel Dur] [Beat SubPos Pitch Vel Dur] … Bar …
```

Each note is exactly 5 tokens. Bar boundaries get one `Bar` token.

## Vocabulary (default config)

| Range      | Token          | Count |
|------------|---------------|-------|
| 0          | Bar           | 1     |
| 1 – 8      | Beat(0–7)     | 8     |
| 9 – 12     | SubPosition(0–3) | 4  |
| 13 – 140   | Pitch(0–127)  | 128   |
| 141 – 172  | Velocity(0–31)| 32    |
| 173 – 236  | Duration(1–64)| 64    |
| **Total**  |               | **237** |

## Configuration

```rust
CompoundConfig {
    beat_resolution: 4,   // grid slots per beat (16th-note resolution)
    velocity_bins: 32,
    max_duration: 64,     // in position units
    pitch_range: (0, 127),
    max_beats: 8,         // covers up to 8/4 time signatures
}
```

## Quantisation

Given `ticks_per_beat` from the MIDI file:

```
ticks_per_pos   = ticks_per_beat / beat_resolution  (min 1)
tick_in_bar     = note.start_tick − bar.start_tick
global_pos      = tick_in_bar / ticks_per_pos
beat            = (global_pos / beat_resolution).min(max_beats − 1)
sub_pos         = global_pos % beat_resolution
duration        = (note.duration_ticks / ticks_per_pos).clamp(1, max_duration)
```

## Decoding

Decoding uses fixed 480 ticks/beat and 4/4 time (time signature is not
encoded). It is lossy — quantisation and velocity binning are not reversible.

```
ticks_per_pos   = 480 / beat_resolution
ticks_per_bar   = beat_resolution × 4 × ticks_per_pos
global_pos      = beat × beat_resolution + sub_pos
start_tick      = current_bar × ticks_per_bar + global_pos × ticks_per_pos
duration_ticks  = duration × ticks_per_pos
```

## CLI Usage

```bash
./target/debug/notatok tokenize file.mid --scheme compound
./target/debug/notatok tokenize file.mid --scheme compound --output tokens.json
```

## Comparison with REMI

| Aspect | REMI | CP |
|--------|------|----|
| Position encoding | Flat `Position(0..N−1)` | `Beat` + `SubPosition` |
| Tokens per note | 4 (+ optional Tempo) | 5 |
| Bar token | Yes | Yes |
| Vocab size (default) | 241 | 237 |
| Tempo tokens | Optional | Not supported |
