use midly::{
    Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
    num::{u15, u24, u28, u4, u7},
};

use crate::{CoreError, Result};

use super::Score;

/// Serialise a [`Score`] to raw SMF (`.mid`) bytes.
///
/// Writes Format 0 (single track). All note tracks are merged into one
/// MIDI track on their original channels. Tempo and time-signature changes
/// from the Score are preserved as meta events. The `ticks_per_beat` value
/// is written verbatim to the SMF header.
///
/// This is used by the CLI `decode` subcommand and the Python `decode`
/// binding to convert a decoded [`Score`] back to a playable MIDI file.
/// Because tokenizer decoding is lossy (quantisation, velocity binning,
/// track merging), the output MIDI is an approximation of the original.
pub fn save_midi(score: &Score) -> Result<Vec<u8>> {
    // Collect (abs_tick, sort_priority, kind) for all events.
    // Meta events (priority 0) sort before note events (priority 1) at the
    // same tick, matching the convention used by most DAWs.
    let mut raw: Vec<(u64, u8, TrackEventKind<'static>)> = Vec::new();

    for tc in &score.tempo_changes {
        let us = tc.us_per_beat.min((1u32 << 24) - 1);
        raw.push((tc.tick, 0, TrackEventKind::Meta(MetaMessage::Tempo(u24::from(us)))));
    }

    for ts in &score.time_signature_changes {
        raw.push((
            ts.tick,
            0,
            TrackEventKind::Meta(MetaMessage::TimeSignature(
                ts.numerator,
                ts.denominator,
                24, // MIDI clocks per metronome click (standard)
                8,  // 32nd notes per MIDI quarter note (standard)
            )),
        ));
    }

    for track in &score.tracks {
        for note in &track.notes {
            let ch = u4::from(note.channel.min(15));
            let key = u7::from(note.pitch.min(127));
            let vel_on = u7::from(note.velocity.min(127));
            let vel_off = u7::from(0);

            raw.push((
                note.start_tick,
                1,
                TrackEventKind::Midi {
                    channel: ch,
                    message: MidiMessage::NoteOn { key, vel: vel_on },
                },
            ));
            raw.push((
                note.start_tick + note.duration_ticks,
                2,
                TrackEventKind::Midi {
                    channel: ch,
                    message: MidiMessage::NoteOff { key, vel: vel_off },
                },
            ));
        }
    }

    // Sort: tick ascending; within same tick, meta < NoteOn < NoteOff
    raw.sort_by_key(|(tick, priority, _)| (*tick, *priority));

    // Convert absolute ticks → delta ticks
    const MAX_U28: u64 = (1u64 << 28) - 1;
    let mut track_events: Vec<TrackEvent<'static>> = Vec::with_capacity(raw.len() + 1);
    let mut prev_tick: u64 = 0;
    for (tick, _, kind) in raw {
        let delta = (tick - prev_tick).min(MAX_U28) as u32;
        track_events.push(TrackEvent { delta: u28::from(delta), kind });
        prev_tick = tick;
    }
    track_events.push(TrackEvent {
        delta: u28::from(0u32),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });

    let tpb = score.ticks_per_beat.min(0x7FFF);
    let smf = Smf {
        header: Header {
            format: Format::SingleTrack,
            timing: Timing::Metrical(u15::from(tpb)),
        },
        tracks: vec![track_events],
    };

    let mut buf = Vec::new();
    smf.write_std(&mut buf)
        .map_err(|e| CoreError::MidiParse(format!("failed to serialise MIDI: {e}")))?;

    Ok(buf)
}
