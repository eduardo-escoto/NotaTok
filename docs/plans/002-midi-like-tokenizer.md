# Plan 002: MIDI-Like / Performance Encoding Tokenizer

**Status:** Complete

## Goal

Implement the MIDI-Like (performance) encoding tokenizer (Oore et al. 2018, Music Transformer) as the third standard tokenizer in notatok, alongside REMI and ABC.

## Motivation

MIDI-Like is the second most widely used tokenization scheme in music-language-model research. It provides a meaningful contrast to REMI:

| Property        | REMI                        | MIDI-Like                      |
|-----------------|-----------------------------|--------------------------------|
| Time reference  | Bar + beat grid             | Absolute milliseconds          |
| Note repr.      | Pitch + Duration tuple      | NoteOn / NoteOff pair          |
| Polyphony       | Implicit (same position)    | Natural (overlapping pairs)    |
| Tempo encoding  | Optional Tempo tokens       | Implicit via TimeShift steps   |

## Vocabulary (388 tokens by default)

```
0   – 127 : NoteOn(0)    – NoteOn(127)
128 – 255 : NoteOff(0)   – NoteOff(127)
256 – 355 : TimeShift(1) – TimeShift(100)   (1-indexed)
356 – 387 : Velocity(0)  – Velocity(31)
```

Default: 128 + 128 + 100 + 32 = **388 tokens**

## Configuration

```rust
pub struct MidiLikeConfig {
    pub time_shift_ms: u32,      // Default 10 ms
    pub time_shift_steps: u8,    // Default 100 → 1 s max per token
    pub velocity_bins: u8,       // Default 32
    pub pitch_range: (u8, u8),   // Default (0, 127)
}
```

## Encoding Approach

1. Collect all notes from all tracks, filter by `pitch_range`.
2. Build a flat event list — two entries per note:
   - `(start_tick, NoteOn, pitch, velocity)`
   - `(start_tick + duration_ticks, NoteOff, pitch, _)`
3. Sort: tick ascending; at equal tick, NoteOff before NoteOn (MIDI convention), then pitch ascending.
4. Walk events maintaining `current_tick` and `current_velocity_bin`:
   - Compute `delta_ms` from elapsed ticks using the active tempo.
   - Emit `TimeShift(max_steps)` tokens for full chunks; emit one `TimeShift(remainder)` for partial; skip if delta == 0.
   - For NoteOn: emit `Velocity(bin)` only if the bin changed, then `NoteOn(pitch)`.
   - For NoteOff: emit `NoteOff(pitch)`.
   - Advance `current_tick`.

## Decoding Approach

Fixed reference: 120 BPM / 480 ticks_per_beat (`ms_to_tick(ms) = ms × 480 / 500`).

State machine over the token stream:
- `current_ms = 0`, `current_velocity_bin = bin_velocity(64)`
- `open_notes: HashMap<pitch, (start_tick, velocity)>`
- `TimeShift(s)` → `current_ms += s × time_shift_ms`
- `Velocity(b)` → update current bin
- `NoteOn(p)` → insert into open_notes
- `NoteOff(p)` → close note, emit to results
- After all tokens: close remaining open notes with duration 1 tick

## Files Changed

| File | Action |
|------|--------|
| `crates/core/src/tokenizer/midi_like/mod.rs` | Created — full implementation |
| `crates/core/src/tokenizer/mod.rs` | Added `pub mod midi_like;` |
| `crates/cli/src/main.rs` | Added `"midi-like"` scheme arm |
| `docs/plans/002-midi-like-tokenizer.md` | This file |
| `docs/tokenizers/midi_like.md` | Companion documentation |

## Open Questions

None. No new dependencies required.
