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

## Alternatives considered

### Rust SMF parsing crates

| Crate | SMF parsing | Last release | 90-day DLs | Verdict |
|-------|-------------|-------------|------------|---------|
| `midly` | Yes — full | Jan 2023 | 40,069 | **Chosen** |
| `rimd` | Yes — partial | Nov 2017 | 1,329 | Abandoned |
| `ghakuf` | Yes — only | Oct 2020 | 1,033 | Dormant |
| `nom-midi` | Yes — limited | Jul 2019 | 81 | Abandoned |
| `nodi` | No (wraps midly) | Jan 2025 | 1,198 | Playback layer, not a parser |
| `wmidi` | No | Sep 2024 | 14,023 | Real-time messages only |
| `midir` | No | Oct 2025 | 65,511 | OS MIDI I/O driver only |

**`rimd`** — abandoned since 2017, still at version `0.0.1`. Known to hang
or freeze on large files (a 24 MB benchmark caused an indefinite hang).
Raw `Vec<u8>` API with no typed event hierarchy.

**`ghakuf`** — dormant since 2020. Uses a callback/visitor pattern
(`Handler` trait) that is awkward for building data structures. No
zero-copy, no `no_std`.

**`nom-midi`** — abandoned since 2019, built on nom 5.x (two major versions
out of date). Errors on valid files; the source code contains the comment
"I don't understand this, but I should be decoding it correctly" on SMPTE
offset handling. 81 downloads in 90 days.

**`nodi`** — not a parser. It is a playback scheduling layer that re-exports
and wraps `midly`. Choosing nodi without midly is not an option.

**`wmidi` / `midir`** — both frequently appear in MIDI discussions but
address orthogonal concerns. `midir` is a cross-platform OS-level hardware
I/O driver; `wmidi` is a real-time individual-message codec. Neither touches
`.mid` file parsing. Both are appropriate complements to `midly` in a
complete MIDI application (e.g. `midir` for live input → `wmidi` or `midly`
for message decoding), not alternatives for SMF file reading.

### Hand-rolling a parser

The spec is public, but the gap between "passes the spec" and "handles
real-world DAW output robustly" is large — running status alone has caused
bugs in every major MIDI library across all languages. Writing and
maintaining a correct parser would involve weeks of work with no research
value. notatok's contribution is the tokenization algorithm, not binary
parsing.

---

## Decision

Use [`midly`](https://crates.io/crates/midly) as the sole MIDI parsing
dependency in `notatok-core`.

`midly` is the only actively maintained SMF-capable crate in the Rust
ecosystem by a significant margin (40k downloads/90 days vs. ≤1,330 for all
alternatives). Key properties that matter for notatok:

| Criterion | Notes |
|-----------|-------|
| **Correctness** | Handles running status, all three SMF formats, both time division types, and malformed files gracefully. `strict` feature flag available when we want hard errors on invalid input. |
| **API quality** | Events surface as typed Rust enums — no manual byte inspection needed downstream in the tokenizer. |
| **Zero-copy** | Parses directly from a `&[u8]` slice; no unnecessary allocation during the parse phase. |
| **Performance** | 60 ms on a 24 MB file vs. 20,575 ms (rimd) and 253 ms (nom-midi) in published benchmarks. |
| **`no_std`** | Compatible with bare-metal / embedded targets if notatok's scope ever expands there. |
| **Encode + decode** | Can write `.mid` files as well as read them; useful if we later add a detokenize → MIDI export path. |

---

## Consequences

- `notatok-core` gains a dependency on `midly`.
- All MIDI I/O goes through `midly`'s types; the tokenizer operates on those
  types, not raw bytes. If we ever need to swap parsers, the boundary is
  clean.
- We do not need MIDI output for the initial tokenization milestone; `midly`
  supports it if that changes.
- `midly` has not had a release since January 2023. It appears intentionally
  in maintenance mode (feature-complete, one open documentation issue). If
  it becomes truly unmaintained and we need features beyond its current
  scope, the clean API boundary makes swapping straightforward.
