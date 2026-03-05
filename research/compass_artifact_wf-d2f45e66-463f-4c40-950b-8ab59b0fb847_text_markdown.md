# The MIDI and audio tokenizer landscape for music ML

**No single package unifies MIDI and raw audio tokenization today — this is the field's most glaring gap.** The ecosystem is split into three parallel worlds: symbolic MIDI tokenizers (dominated by MidiTok + symusic), neural audio codecs (led by EnCodec and DAC), and semantic tokenizers (HuBERT-derived). Each world has its own APIs, data formats, and integration patterns, forcing researchers to cobble together fragile multi-library pipelines. A Rust-based package that bridges these domains would fill a genuine vacuum, especially as the field converges toward unified single-codebook representations that blur the line between symbolic and acoustic tokens.

---

## Part 1: MIDI tokenizers beyond MidiTok

### REMI and its descendants remain the de facto standard

REMI (REvamped MIDI-derived, Huang & Yang 2020) introduced **bar-beat quantized positions** — replacing raw time-shift events with explicit Bar and Position tokens that capture metric structure. A typical REMI vocabulary contains **~250–500 base tokens** (pitch range, velocity bins, duration steps, positions per beat), expandable to ~30K with BPE. REMI+ (from FIGARO, von Rütte et al. 2022) added Program tokens and time-signature support for multi-track, multi-instrument generation. MidiTok implements both REMI and REMI+ as its most popular tokenizations, with symusic as its C++ backend.

The older **MIDI-Like / Performance Encoding** (Oore et al. 2018, used in Google's Music Transformer) uses a flat event vocabulary of just **388 tokens**: 128 NOTE_ON + 128 NOTE_OFF + 100 TIME_SHIFT (10ms resolution) + 32 velocity bins. No bar/beat structure, purely performance-oriented. About ~2K tokens per minute of piano music. Google's Music Transformer (Huang et al., ICLR 2019) paired this with relative self-attention for better long-range structure. The standalone `midi-neural-processor` package on PyPI provides this exact 388-token encoding.

### Compound and multi-attribute tokenizations compress sequences dramatically

Three major approaches represent each note as a fixed-width tuple, processing all attributes simultaneously via multiple embedding layers:

- **Compound Word / CP** (Hsiao et al., AAAI 2021): Groups related tokens into compounds — a note becomes `[Family, Bar, Position, Pitch, Velocity, Duration]`. Each attribute has its own sub-vocabulary. The model sees one embedding per compound, requiring multiple output heads. Converges **5–10x faster** than flat tokenizations but adds architectural complexity.

- **OctupleMIDI** (Zeng et al., ACL 2021, MusicBERT): Each note is an **8-tuple** — (Time, Bar, Position, Instrument, Pitch, Duration, Velocity, Tempo) at 1/64-note granularity. Sequence length equals note count. Powers MusicBERT for symbolic music understanding tasks.

- **MMT Sextuple** (Hao-Wen Dong, ICASSP 2023): A **6-tuple** (Type, Beat, Position, Pitch, Duration, Instrument) sorted by time. Uses Transformer-XL with multiple output heads. Handles multi-track natively with substantial speedups over REMI+ baselines. Available at `github.com/salu133445/mmt`.

These approaches yield the **shortest sequences** (length ≈ note count) but require custom architectures — you can't just drop them into a standard GPT-style pipeline without modification.

### ABC notation is the rising star for LLM-native music generation

ABC notation is text-based music notation using letters A–G for pitches, numbers for durations, and ASCII characters for musical structure. Its key advantage: it's already text, so **standard BPE tokenizers work directly** — no special music tokenizer architecture needed. ChatMusician (Yuan et al., 2024) simply continued pretraining LLaMA-2 on ABC notation. MuPT uses YouTokenToMe BPE with a **50K-token vocabulary**. NotaGen (IJCAI 2025) introduced interleaved ABC notation with voice indicators `[V:]` for multi-track support.

The compression is remarkable: ABC averages **~288 tokens per song** and **~5.16 tokens per second**, far shorter than any MIDI-based representation. The catch: ABC requires beat-synchronized data (no expressive timing), and MIDI→ABC conversion is non-trivial and lossy.

### Arrival-time encoding enables controllable infilling

The Anticipatory Music Transformer (Thickstun et al., Stanford/CRFM, NeurIPS 2023) uses **arrival-time triplets**: `(arrival_time, note_on/off, instrument+pitch)` with **~34K tokens** (expandable to ~55K with anticipation control tokens). Unlike bar-beat approaches, it uses absolute timestamps — no metrical assumptions required. The key innovation is interleaving future "control" tokens before their corresponding events, enabling infilling (e.g., generating accompaniment around a fixed melody) "for free." Available at `github.com/jthickstun/anticipation` with a clean Python API. Context length is ~1024 tokens (~15 seconds), a significant limitation.

### MusicLang takes a music-theory-first approach

MusicLang (`github.com/MusicLang/musiclang`) tokenizes notes **relative to chord/scale context** — encoding harmonic function and scale position rather than absolute pitch. Chord progressions and scale progressions are explicit first-class tokens. This normalized representation compresses well with BPE (~1024 tokens ≈ 25 seconds). It's niche but uniquely controllable: users can specify chord progressions directly. Available on HuggingFace (`musiclang/musiclang-v2`), BSD-3 license, actively maintained.

### The MIDI parsing speed problem is solved by symusic

**symusic** (`github.com/Yikai-Liao/symusic`) is a C++ MIDI parser with Python bindings that is **200–500x faster** than the alternatives: ~22ms for a complex MIDI file vs. ~5.6s (mido/pretty_midi), ~6.3s (miditoolkit), ~8.6s (music21). It is now **MidiTok's default backend**. It supports MIDI, ABC notation I/O, piano roll conversion, and SoundFont synthesis. Actively maintained, PyPI-installable, presented at ISMIR 2024.

Google's Magenta is **effectively deprecated** — the main repo is archived read-only since ~2023 (Google Brain dissolution). The note-seq package still exists but has dependency issues with modern Python. Most researchers have migrated to MidiTok + symusic.

### Other tools worth noting

**music21** (MIT, ~2K+ stars, actively maintained v9.9.1) handles MusicXML/MIDI/Humdrum/ABC with rich music-theory analysis (key detection, chord analysis, voice separation) but is slow (~8.6s per file) and isn't a tokenizer per se — it's a preprocessing layer. **Partitura** (CPJKU) is a lighter-weight alternative for MIR tasks. **MusPy** (Hao-Wen Dong, 2020) provides dataset management and evaluation for symbolic music generation but is less actively maintained.

| Approach | Vocab size | Tokens/min | Multi-track | Beat-aware | Status |
|----------|-----------|------------|-------------|------------|--------|
| MIDI-Like (388) | 388 | ~2,000 | Limited | No | Stable, legacy |
| REMI / REMI+ | 250–500 (30K w/ BPE) | ~500–1,000 | Yes (REMI+) | Yes | Active (MidiTok) |
| CP / Compound | Multiple sub-vocabs | Very short | Limited | Yes | Research |
| OctupleMIDI | 8 sub-vocabs | = note count | Yes | Yes | Research |
| MMT Sextuple | 6 sub-vocabs | = note count | Yes | Yes | Research |
| ABC Notation | 30–50K (BPE) | ~50–100 | Yes (interleaved) | Yes | Rising |
| Arrival-Time | 34–55K | ~4,000 | Yes | No | Active |
| MusicLang | Custom + BPE | ~2,400 | Yes | Yes | Active |

---

## Part 2: Neural audio codecs — the discrete audio token revolution

### EnCodec and DAC dominate, but for different reasons

**EnCodec** (Meta, Oct 2022) established the encoder→RVQ→decoder paradigm for neural audio compression. It uses a convolutional encoder-decoder (SEANet) with **codebook size 1024** across 2–32 codebooks. The 24kHz mono model runs at 75Hz frame rate supporting 1.5–24 kbps; the 48kHz stereo model supports 3–24 kbps at 150Hz. MIT licensed, pip-installable, integrated into HuggingFace Transformers. EnCodec's adoption is massive: **MusicGen, AudioGen, VALL-E, Bark** all use it. MusicGen specifically uses a custom 32kHz model with **4 codebooks of size 2048** — not the released checkpoint.

**DAC** (Descript Audio Codec, NeurIPS 2024) achieves **better perceptual quality** than EnCodec at comparable bitrates, using Snake activation functions (periodic inductive bias), projected codebook lookups (reducing codebook collapse), and a multi-scale multi-band STFT discriminator. Its standout feature: native **44.1kHz** support (9 codebooks, 1024 entries each, ~86Hz, ~8 kbps). Also available at 16kHz and 24kHz. MIT licensed, excellent Python API. Used by **Stability AI's Stable Audio** and **Parler-TTS**. Growing adoption as the higher-quality alternative to EnCodec.

**SoundStream** (Google, Jul 2021) is the foundational paper that introduced RVQ for neural audio codecs, quantizer dropout for variable bitrate, and the encoder-RVQ-decoder architecture all subsequent codecs build on. Used internally by Google for **AudioLM, MusicLM, Lyra V2**. However, **no official open-source weights exist** — only third-party reimplementations.

### SNAC and WavTokenizer represent the cutting edge

**SNAC** (Multi-Scale Neural Audio Codec, NeurIPS 2024 Workshop, `hubertsiuzdak/snac`, MIT license) introduces **multi-scale temporal resolution** — each quantizer operates at a different frame rate. The 24kHz model uses 3 codebooks at [12, 24, 48] tokens/sec with **4096-entry codebooks**, achieving ~0.98 kbps. The 32kHz and 44kHz models use 4 codebooks. Coarse codebooks at ~12Hz capture global structure (a 2048-token context covers ~3 minutes), while fine codebooks at higher rates add detail. Outperforms EnCodec and matches DAC quality at **significantly lower bitrates**. Clean pip-installable API.

**WavTokenizer** (ICLR 2025, `jishengpeng/WavTokenizer`) is arguably the most disruptive recent entry: **a single codebook with 4096 entries** producing just 40–75 tokens/second at 24kHz — achieving **0.5–0.9 kbps** with SOTA UTMOS scores. Uses k-means initialization and random awakening to maintain high codebook utilization. A single codebook means standard autoregressive LMs work directly — no delay patterns, no parallel decoding, no AR+NAR splitting. This dramatically simplifies downstream model architecture.

### Mimi pushes streaming and ultra-low frame rates

**Mimi** (Kyutai, Sept 2024, part of the Moshi conversational AI system) achieves an extraordinary **12.5Hz frame rate** — close to the text token rate of ~3–4Hz — via a large stride and Transformer blocks inside both encoder and decoder. It uses **8 codebooks of size 2048** at 24kHz, totaling just **1.1 kbps**. The first codebook is trained with WavLM semantic distillation (like SpeechTokenizer). Fully causal/streaming with 80ms latency. Critically for this audience: **Mimi has a Rust implementation** via the Candle backend (`pip install rustymimi`). Integrated into HuggingFace Transformers. Used by Moshi and Sesame's CSM.

### The rest of the codec zoo

- **HiFi-Codec** (`yangdongchao/AcademiCodec`): Introduces Group-Residual VQ (GRVQ) — splits latents into groups with separate RVQ per group. Achieves good quality with only **4 codebooks**. Not streamable.

- **FunCodec** (Alibaba/ModelScope, MIT): A toolkit providing SoundStream, EnCodec, and novel FreqCodec implementations with 7+ pretrained models. Kaldi-style pipelines, k-means codebook initialization, semantic token augmentation. Speech-focused (English/Chinese). Research-quality API.

- **AudioDec** (Meta, ICASSP 2023): Enhanced EnCodec with group convolutions for real-time streaming + HiFi-GAN vocoder. 48kHz/24kHz models, ~12 kbps, <6ms GPU latency. CC-BY-NC license. Speech-only training.

- **Vocos** (`gemelo-ai/vocos`): **Not a codec but a vocoder** — a drop-in replacement decoder for EnCodec tokens that produces **better quality** (UTMOS 3.88 vs EnCodec's 3.77) while being **70x faster** than BigVGAN. 7.9M parameters. Same author as SNAC. Used with Bark.

### How models handle multi-codebook RVQ

The fundamental challenge: RVQ produces a 2D matrix (time × codebook depth), but LMs want 1D sequences. Key approaches:

- **Delay pattern** (MusicGen default): Codebook *i* is delayed by *i* steps. At each autoregressive step, predict one token per codebook but each sees tokens from slightly earlier timesteps. Sequence length equals timestep count. Best quality-efficiency tradeoff.

- **Flattening**: Serialize all codebooks sequentially (Q1_t1, Q2_t1, Q3_t1, ..., Q1_t2, ...). Highest quality but multiplies sequence length by codebook count.

- **AR+NAR** (VALL-E pattern): Autoregressive Transformer generates first codebook; non-autoregressive Transformer generates remaining codebooks conditioned on the first. Dominant in TTS.

- **MaskGIT parallel** (SoundStorm): Bidirectional Conformer fills tokens level-by-level using iterative masking. Only **27 forward passes** for 30 seconds. 100x faster than AudioLM's approach.

| Codec | Year | Codebooks | CB size | Sample rates | Bitrate | Frame rate | License | Key users |
|-------|------|-----------|---------|-------------|---------|-----------|---------|-----------|
| SoundStream | 2021 | 3–80 | 1024 | 24kHz | 3–18 kbps | 75Hz | Closed | AudioLM, MusicLM |
| EnCodec | 2022 | 2–32 | 1024 | 24/48kHz | 1.5–24 kbps | 75/150Hz | MIT | MusicGen, VALL-E, Bark |
| DAC | 2023 | 9–32 | 1024 | 16/24/44.1kHz | 8–24 kbps | ~86Hz | MIT | Stable Audio, Parler-TTS |
| HiFi-Codec | 2023 | 4 | 1024 | 16/24kHz | ~2 kbps | 50Hz | Open | — |
| SpeechTokenizer | 2023 | 8 | 1024 | 16kHz | ~4 kbps | 50Hz | Apache 2.0 | USLM |
| SNAC | 2024 | 3–4 | 4096 | 24/32/44kHz | 0.98–3.5 kbps | Multi-scale | MIT | — |
| Mimi | 2024 | 8 | 2048 | 24kHz | 1.1 kbps | 12.5Hz | Apache 2.0 | Moshi, CSM |
| WavTokenizer | 2024 | **1** | 4096 | 24kHz | 0.5–0.9 kbps | 40–75Hz | Open | — |
| XCodec2 | 2025 | **1** (FSQ) | 65,536 | — | — | — | Open | LLaSA |

---

## Part 3: Semantic tokenizers and higher-level frameworks

### HuBERT discrete units — the original semantic tokens

The canonical approach: extract continuous embeddings from a pretrained HuBERT model's intermediate Transformer layer, then run **k-means clustering** (typically 50–2000 clusters) to produce discrete tokens at ~25–50Hz. Layer selection matters — **layer 9** (middle layers) provides the best content/phonetic information per SpeechTokenizer ablations, while higher layers capture more abstract features. Common cluster counts: 500 for speech language models, 1000–2000 for multi-task LLMs (per Interspeech 2024 recommendations).

**AudioLM** (Google, 2022) pioneered the **3-stage hierarchical approach**: (1) autoregressive generation of w2v-BERT semantic tokens at 25Hz for linguistic coherence, (2) coarse SoundStream codec tokens conditioned on semantics, (3) fine SoundStream tokens for fidelity. This decomposition showed that semantic tokens are essential — without them, generated audio sounds acoustically plausible but semantically incoherent.

### SpeechTokenizer and Mimi unify semantic + acoustic in one codec

**SpeechTokenizer** (Zhang et al., ICLR 2024, `pip install speechtokenizer`, Apache 2.0) was the first to **disentangle semantic and acoustic information across RVQ layers**: the first codebook is trained with HuBERT layer-9 distillation to capture content; codebooks 2–8 capture timbre, prosody, and acoustic detail. Eight codebooks of 1024 entries at 16kHz, ~50Hz, ~4 kbps. Clean Python API. This eliminates the need for separate semantic and acoustic tokenization models.

**Mimi** adopts the same principle with WavLM distillation but at a much lower frame rate (12.5Hz vs 50Hz) and higher codebook size (2048 vs 1024), achieving better quality at lower bitrate. Mimi's Rust backend makes it the only major codec with native high-performance non-Python support.

### WavTokenizer and XCodec2 push single-codebook unification

**XCodec2** (2025, `pip install xcodec2`) takes unification furthest: a **single FSQ (Finite Scalar Quantization) layer with 65,536 entries** — a vocabulary size comparable to text tokenizers like LLaMA-3's 128K. It fuses Wav2Vec2-BERT semantic features with acoustic encoder features before quantization. Trained on 150K hours of multilingual speech. Used by LLaSA (LLaMA-based speech synthesis). Single codebook + large vocabulary means audio tokens can be mixed directly into standard LLM vocabularies.

### SemantiCodec takes a diffusion-based approach

**SemantiCodec** (Liu et al., 2024) uses an AudioMAE semantic encoder + a residual acoustic encoder, both discretized, feeding into a **latent diffusion model decoder** (not a feedforward decoder). Achieves ultra-low bitrates of **0.31–1.40 kbps** at 25–100 tokens/second, significantly outperforming DAC on reconstruction quality while carrying richer semantic information. The tradeoff: diffusion decoding is much slower than feedforward.

### Jukebox and Stable Audio represent older paradigms

**Jukebox** (OpenAI, 2020) used a **3-level hierarchical VQ-VAE** on 44.1kHz raw audio with 2048-entry codebooks at each level — top level captured melody/singing, bottom level added fidelity. Three separate autoregressive Transformer priors generated codes hierarchically. Historically important but architecturally superseded and **archived/unmaintained** by OpenAI.

**Stable Audio** (Stability AI) represents the **continuous latent alternative**: a VAE (using DAC's architecture) compresses audio to continuous latent space where a diffusion U-Net operates — no discrete tokens at all. This sidesteps tokenization entirely, conditioned on CLAP text embeddings and timing information.

### AudioPaLM and SoundStorm show Google's approach

**AudioPaLM** (Google, June 2023) extends PaLM-2 (8B) by adding discrete audio tokens to the vocabulary: w2v-BERT or USM semantic tokens at 25Hz + SoundStream acoustic tokens. Text and audio tokens are interleaved in a single sequence. No public release. **SoundStorm** (Google, May 2023) uses MaskGIT-style parallel decoding to generate SoundStream tokens level-by-level, requiring only **27 forward passes for 30 seconds of audio** — 100x faster than AudioLM. No official code; community reimplementation at `lucidrains/soundstorm-pytorch`.

---

## Part 4: Gaps and opportunities for a Rust-based unified tokenizer

### The Python-only bottleneck is real

Nearly the entire audio ML tokenization stack is Python/PyTorch. The only Rust implementation of any significance is **Mimi's Candle backend** (`rustymimi`). symusic solved MIDI parsing speed with C++, but tokenization logic still runs in Python. There is no Rust-native MIDI tokenizer, no Rust audio codec inference library, and no unified Rust package for either. HuggingFace's text Tokenizers library proved that Rust backends with Python bindings can deliver **10–100x speedups** while maintaining Python ergonomics — the same pattern is ripe for audio/MIDI tokenization.

### No package bridges MIDI and audio tokenization

This is the ecosystem's most fundamental gap. A researcher working on a model that consumes both MIDI scores and audio recordings must use MidiTok (Python, symusic C++ backend) for MIDI, EnCodec or DAC (Python/PyTorch) for audio, and possibly HuBERT + scikit-learn k-means for semantic tokens — three completely separate libraries with incompatible data formats, different batching conventions, and no shared vocabulary or embedding interface. There is no `tokenizer.encode(midi_or_audio)` that produces compatible token sequences for joint training.

### Specific technical gaps a Rust package could address

- **Streaming tokenization**: Most codecs are non-causal. A Rust-native streaming encoder would enable real-time MIDI+audio tokenization for interactive applications. Only Mimi currently supports streaming, and only for audio.

- **Unified vocabulary construction**: No tool helps researchers build joint MIDI+audio vocabularies. XCodec2's 65K FSQ vocabulary is comparable to text tokenizer sizes, suggesting MIDI tokens could be merged into the same vocabulary space.

- **Fast BPE on codec tokens**: The `codec-bpe` package applies BPE to flattened RVQ codes for 2–5x compression, but it's a thin Python wrapper. A Rust implementation of codec-token BPE with the same speed as HuggingFace Tokenizers would be immediately useful.

- **Deterministic, reproducible tokenization**: Python float precision and library version differences cause subtle tokenization inconsistencies. A Rust implementation with explicit determinism guarantees would improve reproducibility.

- **Multi-scale / hierarchical token handling**: SNAC's multi-scale approach (different codebooks at different temporal resolutions) is powerful but awkward to implement with standard batching. Native support for hierarchical token sequences would be valuable.

- **Training pipeline integration**: No tokenizer package provides first-class support for modern training workflows (streaming datasets, on-the-fly tokenization, mixed MIDI+audio batches, HuggingFace datasets integration). Most researchers tokenize offline and save to disk.

### Where the field is heading

The convergence is clear: **single-codebook, large-vocabulary, semantically-informed audio tokens** that work like text tokens. WavTokenizer (1 codebook, 4096 entries), XCodec2 (1 codebook, 65K entries), and UniCodec (domain-adaptive single codebook) all point toward a future where audio tokens are as straightforward to model as text. For MIDI, ABC notation's text compatibility is driving a parallel convergence. A unified Rust tokenizer that supports both modalities with a single consistent API — producing compatible token sequences from either MIDI files or raw waveforms — would be positioned at exactly the right intersection of these trends.

The window of opportunity is open: the ecosystem is fragmented, performance matters for large-scale training, Rust is proven in the tokenizer space (HuggingFace Tokenizers), and the field is rapidly standardizing on simpler token representations that make unification feasible.