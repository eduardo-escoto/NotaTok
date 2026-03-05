# MIDI-Like Tokenizer

The MIDI-Like (performance) encoding was introduced by Oore et al. in
["This Time with Feeling: Learning Expressive Musical Performance"](https://arxiv.org/abs/1808.03715) (2018)
and used in Google's Music Transformer. It is the reference implementation for
event-driven MIDI tokenization.

## Key Differences from REMI

| Property        | REMI                       | MIDI-Like                     |
|-----------------|----------------------------|-------------------------------|
| Time model      | Bar + beat grid (ticks)    | Absolute milliseconds         |
| Note encoding   | `Pitch` + `Duration` tuple | `NoteOn` / `NoteOff` pair     |
| Polyphony       | Implicit (same position)   | Natural (overlapping pairs)   |
| Tempo           | Optional `Tempo` tokens    | Implicit in TimeShift steps   |

MIDI-Like produces longer sequences (roughly 2× for typical piano music because
each note needs two tokens instead of one) but preserves polyphonic timing
exactly and captures real expressive timing rather than a quantized grid.

## Vocabulary

Default configuration (`pitch_range` = 0–127, `time_shift_steps` = 100,
`velocity_bins` = 32):

```
  0 –  127 : NoteOn(0)    – NoteOn(127)
128 –  255 : NoteOff(0)   – NoteOff(127)
256 –  355 : TimeShift(1) – TimeShift(100)   [1-indexed, each = 10 ms]
356 –  387 : Velocity(0)  – Velocity(31)
```

Total: 128 + 128 + 100 + 32 = **388 tokens**.

The vocabulary size scales as `2 × n_pitches + time_shift_steps + velocity_bins`.

## Configuration

```rust
use notatok_core::tokenizer::midi_like::MidiLikeConfig;

let config = MidiLikeConfig {
    time_shift_ms: 10,       // ms per TimeShift step
    time_shift_steps: 100,   // steps in vocab → max 1 s per token
    velocity_bins: 32,
    pitch_range: (21, 108),  // piano range only
};
```

## Encoding

Given a `Score`, the encoder:

1. Flattens all tracks into a single event list (one `NoteOn` and one `NoteOff` per note).
2. Sorts events by tick; at the same tick, `NoteOff` precedes `NoteOn` (standard MIDI convention, prevents double-sustain artefacts).
3. Walks events in order:
   - Converts the tick delta to milliseconds using the score's tempo map (honours tempo changes).
   - Emits one or more `TimeShift` tokens to cover the elapsed time (max `time_shift_steps` per token; ceiling-rounds the last partial step).
   - For `NoteOn`: emits a `Velocity` token only when the velocity bin changes, then `NoteOn(pitch)`.
   - For `NoteOff`: emits `NoteOff(pitch)`.

### Example token sequence

For a single middle-C quarter note at 120 BPM (480 ticks = 500 ms):

```
Velocity(16)   ← bin for velocity 64
NoteOn(60)
TimeShift(50)  ← 50 × 10 ms = 500 ms
NoteOff(60)
```

## Decoding

Decoding is approximate (lossy):

- Timing is fixed at 120 BPM / 480 ticks/beat for the output `Score`.
- `TimeShift(s)` advances `current_ms` by `s × time_shift_ms`.
- `NoteOn(p)` opens a note; `NoteOff(p)` closes it and computes the duration.
- Notes still open at end-of-stream are closed with `duration_ticks = 1` (edge case).
- The result is a single merged track; multi-track information is not preserved.

## Usage

### Rust

```rust
use notatok_core::midi::load_midi;
use notatok_core::tokenizer::midi_like::{MidiLikeConfig, MidiLikeTokenizer};
use notatok_core::tokenizer::Tokenizer;

let bytes = std::fs::read("song.mid")?;
let score = load_midi(&bytes)?;
let tokenizer = MidiLikeTokenizer::new(MidiLikeConfig::default());
let tokens = tokenizer.encode(&score)?;
println!("vocab_size={}, tokens={}", tokenizer.vocab_size(), tokens.len());
```

### CLI

```bash
notatok tokenize song.mid --scheme midi-like
notatok tokenize song.mid --scheme midi-like --output tokens.json
```

## Design Decisions

**Why 10 ms steps?** At 100 steps per token the maximum gap representable in a
single token is 1 second, which covers almost all inter-note gaps in music up
to ~60 BPM. Gaps beyond 1 second (e.g. long rests) are covered by emitting
multiple max-step tokens.

**Why ceiling-round the last partial step?** Integer floor would silently shorten
notes by up to one step (10 ms). Ceiling preserves or slightly extends timing,
which is less damaging for generation fidelity.

**NoteOff before NoteOn at the same tick** matches the MIDI file convention and
prevents a note from sustaining into its replacement when a sequence is rendered
back through a synthesiser.
