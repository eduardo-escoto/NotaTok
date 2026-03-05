# ADR 001: Use `midly` for MIDI parsing

**Status:** Accepted

---

## Context

notatok needs to parse Standard MIDI Files (SMF) as the first step of its
tokenization pipeline. The MIDI binary format has enough edge cases that a
hand-rolled parser carries real risk:

- **Variable-length quantities (VLQ)** — delta times use a variable-width
  big-endian encoding where the MSB of each byte signals continuation.
- **Running status** — a bandwidth-saving trick where the status byte is
  omitted when it matches the previous event. Widespread in real-world DAW
  output; reading such files incorrectly produces silently corrupted data.
- **SMF format variants** — Format 0 (single merged track), Format 1
  (multiple synchronised tracks), Format 2 (multiple independent tracks)
  all require distinct handling.
- **Time division ambiguity** — the header encodes time either as PPQN
  (ticks per quarter note) or SMPTE frames + sub-frames; the two are
  structurally incompatible.
- **Meta events and SysEx** — arbitrary-length blobs with their own framing
  that must be skipped or decoded depending on relevance.

The Python ecosystem solved this with **symusic** (a C++ parser, 200–500x
faster than mido/pretty_midi — see `research/` notes). The Rust equivalent
is `midly`.

---

## Decision

Use [`midly`](https://crates.io/crates/midly) as the sole MIDI parsing
dependency in `notatok-core`.

### Why `midly`

| Criterion | Notes |
|-----------|-------|
| **Correctness** | Handles running status, all three SMF formats, both time division types, and malformed files gracefully |
| **API quality** | Events come out as typed Rust enums — no manual byte inspection needed downstream |
| **Zero-copy** | Parses directly from a `&[u8]` slice; no unnecessary allocation during the parse phase |
| **Maintenance** | The most actively maintained and widely used MIDI crate in the Rust ecosystem |
| **Scope fit** | Covers exactly what notatok needs: reading SMF files into structured events; it is not opinionated about playback or synthesis |

### Why not write our own

The MIDI binary format is specified, but the delta between "passes the spec"
and "handles real-world DAW output robustly" is significant (running status
alone has caused bugs in every major MIDI library). Writing and maintaining
a correct parser would be weeks of work with no research value — notatok's
contribution is the tokenization algorithm, not binary parsing.

---

## Consequences

- `notatok-core` gains a dependency on `midly`.
- All MIDI I/O goes through `midly`'s types; our tokenizer works on those
  types, not raw bytes. If we ever need to swap parsers, the boundary is
  clean.
- We do not need to handle MIDI output (encoding/writing) for the initial
  MIDI tokenization milestone; `midly` supports it if that changes.
