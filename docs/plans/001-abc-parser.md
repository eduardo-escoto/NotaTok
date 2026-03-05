# Plan 001 — ABC Notation Parser (`parser.rs`)

**Status:** In progress
**Goal:** Implement `parse_abc_score(text: &str) -> Result<Score>` — the decode half of `AbcTokenizer`.

---

## Context

`crates/core/src/tokenizer/abc/mod.rs` implements `AbcConverter::to_abc` (encode) and references `parser::parse_abc_score` for decode, but `parser.rs` does not yet exist. The `abc` module is also not registered in `tokenizer/mod.rs`, not wired into the CLI, and has no companion docs.

The ABC subset we need to parse is exactly what `AbcConverter` produces:

```
X:1
T:Untitled
M:4/4
L:1/16
Q:1/4=120
K:C
V:1
c4 e4 g4 | c'4 e'4 z4 |
V:2
C4 E4 G4 | ...
```

---

## Design

### 1. Header parsing

Scan lines of the form `F:value` before the first `K:` line:

| Field | Action |
|-------|--------|
| `M:4/4` | Store `meter_num=4`, `meter_den=4` |
| `L:1/16` | Store `unit_p=1`, `unit_q=16` |
| `Q:1/4=120` | Extract BPM from the `=N` part |
| `K:C` | Mark body start; create first voice |
| `V:n` (body) | Switch current voice to index `n-1` |

`unit_ticks = unit_p * 4 * TICKS_PER_BEAT / unit_q`
(A whole note = 4 * 480 = 1920 ticks; L:1/16 → 1920/16 = 120 ticks/unit ✓)

### 2. Bar-boundary snapping

**Problem:** `render_voice` does not fill trailing rests at the end of a bar, so a bar with one 1-unit note followed by a `|` looks like `c | e`. A naïve parser would place `e` at tick=120, not tick=1920.

**Solution:** Track `bar_count` (number of `|` symbols seen). On each `|`:
```
current_tick = bar_count * bar_ticks
bar_count += 1
```
where `bar_ticks = units_per_bar * unit_ticks` and `units_per_bar = meter_num * unit_q / (meter_den * unit_p)`.

### 3. Note / chord parsing (character-by-character)

Token grammar (simplified):

```
body      := (note | chord | rest | barline | space | comment)*
note      := accidental* letter octave* duration?
chord     := '[' (accidental* letter octave*)+ ']' duration?
rest      := ('z'|'Z') octave* duration?
barline   := '|' | '||' | '|]' | '|:' | ':|'
duration  := num? ('/' num?)?   -- no suffix = 1; '/' alone = ÷2
accidental:= '^' | '_' | '='   -- multiple allowed; '=' resets to 0
octave    := '\'' | ','         -- ' = +1 octave, , = -1 octave
```

Pitch conversion:
- Uppercase `C`–`B` → base MIDI octave 4 (48–59)
- Lowercase `c`–`b` → base MIDI octave 5 (60–71)
- `'` adds 12; `,` subtracts 12
- `^` adds 1; `_` subtracts 1; `=` resets accidental to 0
- Clamp result to `[0, 127]`; out-of-range notes are silently dropped

Duration:
```
unit * num / den
```
where `num=1` if no leading digits, `den=1` if no `/`, `den=2` if bare `/`.

### 4. Output `Score`

- `ticks_per_beat = 480`
- One `Track` per non-empty voice, notes sorted by `start_tick`
- Single `TempoChange { tick: 0, us_per_beat }` from Q: field
- Single `TimeSignatureChange` from M: field (denominator stored as power-of-2 exponent)
- Velocity fixed at 64 (not encoded in ABC)

---

## Files to create / modify

| File | Action |
|------|--------|
| `crates/core/src/tokenizer/abc/parser.rs` | Create — full parser implementation |
| `crates/core/src/tokenizer/mod.rs` | Add `pub mod abc;` |
| `crates/cli/src/main.rs` | Add `"abc"` scheme arm |
| `docs/tokenizers/abc.md` | Create — companion doc |

---

## Open questions

- None blocking — the output format is fully defined by `AbcConverter`.
- Future: handle key-signature accidentals (e.g. `K:G` implies F#). Currently ignored; all accidentals are explicit sharps from the encoder.
