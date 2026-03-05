# Plan 003: Compound Word (CP) Tokenizer

**Status:** Implemented

## Goal

Add a Compound Word (CP) tokenizer as the fourth tokenization scheme in notatok,
following Hsiao et al. 2021 ("Compound Word Transformer").

## Approach

CP's defining difference from REMI: decompose each note's bar-relative position
into two separate fields — `Beat` (beat within bar) and `SubPosition` (position
within beat) — rather than one flat `Position` index. This gives downstream
models separate embeddings for beat-level and sub-beat timing.

Each note is encoded as a 5-token compound word:
`Beat | SubPosition | Pitch | Velocity | Duration`

Bar boundaries are marked with a single `Bar` token (same as REMI).

## Vocabulary (237 tokens by default)

```
ID 0               : Bar
1  – 8             : Beat(0) – Beat(7)           [max_beats = 8]
9  – 12            : SubPosition(0) – SubPosition(3)    [beat_resolution = 4]
13 – 140           : Pitch(0) – Pitch(127)
141 – 172          : Velocity(0) – Velocity(31)      [32 bins]
173 – 236          : Duration(1) – Duration(64)      [1-indexed, 64 slots]
```

Total: 1 + 8 + 4 + 128 + 32 + 64 = **237**

## Files Changed

| File | Action |
|------|--------|
| `crates/core/src/tokenizer/compound/mod.rs` | Created — full implementation |
| `crates/core/src/tokenizer/mod.rs` | Added `pub mod compound;` |
| `crates/cli/src/main.rs` | Added `"compound"` scheme arm |
| `docs/plans/003-compound-tokenizer.md` | This file |
| `docs/tokenizers/compound.md` | Created |

## No new dependencies

Bar-boundary precomputation and velocity binning are implemented locally
(same algorithm as REMI, copied to keep modules independent).

## Tests (10)

- `default_vocab_size_is_237`
- `empty_score_returns_empty_tokens`
- `first_tokens_are_bar_beat_subpos`
- `two_notes_same_bar_share_one_bar_token`
- `note_in_second_bar_emits_two_bar_tokens`
- `beat_and_sub_position_decomposed_correctly`
- `encode_decode_preserves_pitches_and_order`
- `all_token_ids_within_vocab_size`
- `velocity_token_emitted_per_note`
- `beat_sub_position_round_trip_through_vocab`

All 58 tests pass (`cargo test -p notatok-core`).
