# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Build core + CLI
cargo build
cargo build --release

# Run all tests
cargo test

# Run tests for a specific crate
cargo test -p notatok-core

# Run a single test by name
cargo test -p notatok-core vocab_size_is_241_for_defaults

# Python bindings (requires maturin)
maturin develop
pip install -e .

# CLI usage
./target/debug/notatok tokenize <file.mid>
./target/debug/notatok tokenize <file.mid> --output tokens.json
```

## Architecture

Three-crate workspace with a clean separation of concerns:

- **`crates/core`** — library with MIDI parsing, Score IR, and tokenizer implementations
- **`crates/cli`** — thin binary wrapping core via `clap`; single `tokenize` subcommand
- **`crates/python`** — PyO3 `cdylib` that exposes `encode(bytes) -> Vec<u32>` to Python

The workspace's `default-members` excludes `crates/python`; Python bindings must be built separately via `maturin`.

### Core Data Flow

```
.mid bytes → load_midi() → Score IR → Tokenizer::encode() → Vec<u32>
```

**Score IR** (`crates/core/src/midi/`): `Note`, `Track`, `Score`, `TempoChange`, `TimeSignatureChange`. All timing is absolute ticks (delta times are accumulated during parsing). NoteOn/NoteOff pairs are matched into `Note` structs. Only PPQN timing is supported — SMPTE files return `CoreError::MidiParse`.

**Tokenizer trait** (`crates/core/src/tokenizer/mod.rs`): `encode`, `decode`, `vocab_size`. Implementations must be `Send + Sync` for PyO3 compatibility. Decoding is lossy by design — quantization and velocity binning are not reversible.

**REMI tokenizer** (`crates/core/src/tokenizer/remi/`): Bar-beat quantized token sequence. Token order within a bar: `Bar → Position → [Tempo] → Pitch → Velocity → Duration`. The `Vocabulary` struct manages contiguous integer ID layout; default config yields 241 IDs (see `vocab.rs` for the ID layout table). `RemiConfig` is the entry point for tuning resolution, bins, and pitch range.

### Error Handling

`CoreError` in `crates/core/src/lib.rs` is the single error type. Use `thiserror` for new variants. CLI uses `anyhow` for context wrapping. Python bindings convert `CoreError` → `PyValueError`.

### Adding a New Tokenizer

1. Create `crates/core/src/tokenizer/<name>/`
2. Implement `Tokenizer` trait
3. Re-export from `crates/core/src/tokenizer/mod.rs`
4. Add a new `--scheme` arm in `crates/cli/src/main.rs`

## Key Decisions

- **MIDI parser**: `midly` (see `docs/decisions/001-midi-parser.md`). It is the only actively maintained SMF-capable Rust crate. Do not replace it without reading the ADR.
- `crates/python` is excluded from default workspace builds to avoid requiring Python/maturin for normal Rust development.

## Documentation and Planning Workflow

**Before building:** Read existing docs in `docs/` and this file. For any non-trivial change, write a plan to `docs/plans/` before touching code.

**Plans:** Save plans as `docs/plans/<NNN>-<slug>.md` (e.g. `002-remi-decoder.md`). A plan should cover: goal, approach, files to change, and open questions. Plans are living documents — update them as work progresses.

**Decisions:** Record architectural and dependency decisions as ADRs in `docs/decisions/` (see `001-midi-parser.md` as the template). Number them sequentially. An ADR should cover: context, alternatives considered, decision, and consequences.

**Code docs:** Add doc comments (`///`) to all public items — structs, traits, functions, and non-obvious fields. Document invariants, units (e.g. ticks vs. ms), and loss of information (e.g. decode is lossy). See existing doc comments in `midi/mod.rs` and `tokenizer/mod.rs` as the style reference.

**Markdown docs:** For each significant module or feature, create a companion markdown file in `docs/` explaining what it does, key design choices, and usage examples. Mirror the code structure (e.g. `docs/tokenizers/remi.md` for `crates/core/src/tokenizer/remi/`).

**As you work:** Refer back to existing plans, ADRs, and doc comments before making changes that touch documented decisions. Update docs when behaviour changes.
