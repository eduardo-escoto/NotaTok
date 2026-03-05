"""Type stubs for the notatok native extension module."""

def encode(midi_bytes: bytes, scheme: str = "remi") -> list[int]:
    """Tokenize raw MIDI bytes into a flat list of integer token IDs.

    Parameters
    ----------
    midi_bytes:
        Raw content of a ``.mid`` file.
    scheme:
        Tokenization scheme. One of ``"remi"`` (default), ``"abc"``,
        ``"midi-like"``, or ``"compound"``.

    Returns
    -------
    list[int]
        Flat sequence of token IDs.

    Raises
    ------
    ValueError
        If the bytes are not a valid MIDI file, or the scheme is unknown.
    """
    ...

def decode(tokens: list[int], scheme: str = "remi") -> bytes:
    """Decode a token sequence back into raw MIDI bytes.

    Decoding is approximate: quantisation and velocity binning are not
    reversible, and multi-track information is not preserved. The returned
    MIDI file uses 480 ticks/beat and 4/4 time.

    Parameters
    ----------
    tokens:
        Flat sequence of token IDs produced by :func:`encode`.
    scheme:
        Tokenization scheme used during encoding. Must match the scheme
        passed to :func:`encode`. Defaults to ``"remi"``.

    Returns
    -------
    bytes
        Raw content of a valid ``.mid`` file (Format 0, 480 ticks/beat).

    Raises
    ------
    ValueError
        If any token ID is out of range for the scheme, or the scheme is unknown.
    """
    ...
