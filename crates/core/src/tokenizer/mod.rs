pub mod remi;

use crate::midi::Score;
use crate::Result;

/// Unified tokenization interface for all notatok schemes.
///
/// Implementations must be `Send + Sync` so they can be shared across
/// threads and used in PyO3 contexts where the GIL may be released.
pub trait Tokenizer: Send + Sync {
    /// Encode a [`Score`] into a flat sequence of integer token IDs.
    fn encode(&self, score: &Score) -> Result<Vec<u32>>;

    /// Decode a token sequence back into a [`Score`].
    ///
    /// **Note:** Decoding is approximate. REMI and most other schemes are
    /// designed for generation, not lossless reconstruction. The returned
    /// `Score` will have:
    /// - A single merged track (multi-track information is not preserved)
    /// - Quantized tick positions (sub-grid timing is lost)
    /// - Approximate velocities (binned during encoding)
    fn decode(&self, tokens: &[u32]) -> Result<Score>;

    /// Total number of token IDs in this tokenizer's vocabulary.
    fn vocab_size(&self) -> usize;
}
