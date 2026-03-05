use std::collections::HashMap;

use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

use crate::{CoreError, Result};

use super::{Note, Score, TempoChange, TimeSignatureChange, Track};

/// Parse raw `.mid` bytes into a [`Score`].
///
/// Supports SMF Format 0 and Format 1. Format 2 is handled the same as
/// Format 1 (each track processed independently). SMPTE timecode files
/// return [`CoreError::MidiParse`] — only PPQN (metrical) timing is
/// supported.
pub fn load_midi(bytes: &[u8]) -> Result<Score> {
    let smf = Smf::parse(bytes).map_err(|e| CoreError::MidiParse(e.to_string()))?;

    let ticks_per_beat = match smf.header.timing {
        Timing::Metrical(tpb) => tpb.as_int() as u16,
        Timing::Timecode(_, _) => {
            return Err(CoreError::MidiParse(
                "SMPTE timecode timing is not supported".into(),
            ));
        }
    };

    let mut all_tempo_changes: Vec<TempoChange> = Vec::new();
    let mut all_timesig_changes: Vec<TimeSignatureChange> = Vec::new();
    let mut tracks: Vec<Track> = Vec::new();

    for raw_track in &smf.tracks {
        let mut abs_tick: u64 = 0;
        // (channel, pitch) → (start_tick, velocity) for open NoteOn events
        let mut active_notes: HashMap<(u8, u8), (u64, u8)> = HashMap::new();
        let mut notes: Vec<Note> = Vec::new();
        let mut program_per_channel: HashMap<u8, u8> = HashMap::new();
        let mut track_name: Option<String> = None;

        for event in raw_track.iter() {
            abs_tick += event.delta.as_int() as u64;

            match &event.kind {
                TrackEventKind::Meta(MetaMessage::Tempo(us)) => {
                    all_tempo_changes.push(TempoChange {
                        tick: abs_tick,
                        us_per_beat: us.as_int(),
                    });
                }
                TrackEventKind::Meta(MetaMessage::TimeSignature(num, den, _, _)) => {
                    all_timesig_changes.push(TimeSignatureChange {
                        tick: abs_tick,
                        numerator: *num,
                        denominator: *den,
                    });
                }
                TrackEventKind::Meta(MetaMessage::TrackName(name_bytes)) => {
                    // Convert to owned string so we don't hold a ref into `bytes`
                    track_name = Some(String::from_utf8_lossy(name_bytes).into_owned());
                }
                TrackEventKind::Midi { channel, message } => {
                    let ch = channel.as_int();
                    match message {
                        MidiMessage::ProgramChange { program } => {
                            program_per_channel.insert(ch, program.as_int());
                        }
                        MidiMessage::NoteOn { key, vel } => {
                            let pitch = key.as_int();
                            let velocity = vel.as_int();
                            if velocity == 0 {
                                // NoteOn with vel=0 is a NoteOff per MIDI spec
                                close_note(&mut active_notes, &mut notes, ch, pitch, abs_tick);
                            } else {
                                // Overwrite any previously open NoteOn for the same key/channel
                                active_notes.insert((ch, pitch), (abs_tick, velocity));
                            }
                        }
                        MidiMessage::NoteOff { key, .. } => {
                            close_note(&mut active_notes, &mut notes, ch, key.as_int(), abs_tick);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        // Flush notes that never received a NoteOff (truncated files)
        for ((ch, pitch), (start, vel)) in active_notes.drain() {
            notes.push(Note {
                pitch,
                velocity: vel,
                start_tick: start,
                duration_ticks: abs_tick.saturating_sub(start),
                channel: ch,
            });
        }

        notes.sort_by_key(|n| (n.start_tick, n.pitch));

        // Prefer channel 0's program; fall back to any channel
        let program = program_per_channel
            .get(&0)
            .copied()
            .or_else(|| program_per_channel.values().copied().next());

        // Suppress empty conductor tracks (Format 1 track 0 has only meta events)
        if !notes.is_empty() || track_name.is_some() {
            tracks.push(Track { notes, program, name: track_name });
        }
    }

    all_tempo_changes.sort_by_key(|t| t.tick);
    all_timesig_changes.sort_by_key(|t| t.tick);

    // Insert MIDI defaults when the file omits them
    if all_tempo_changes.is_empty() {
        all_tempo_changes.push(TempoChange { tick: 0, us_per_beat: 500_000 }); // 120 BPM
    }
    if all_timesig_changes.is_empty() {
        all_timesig_changes.push(TimeSignatureChange {
            tick: 0,
            numerator: 4,
            denominator: 2, // 2^2 = 4 → 4/4
        });
    }

    Ok(Score {
        tracks,
        tempo_changes: all_tempo_changes,
        time_signature_changes: all_timesig_changes,
        ticks_per_beat,
    })
}

#[inline]
fn close_note(
    active: &mut HashMap<(u8, u8), (u64, u8)>,
    notes: &mut Vec<Note>,
    channel: u8,
    pitch: u8,
    abs_tick: u64,
) {
    if let Some((start_tick, velocity)) = active.remove(&(channel, pitch)) {
        notes.push(Note {
            pitch,
            velocity,
            start_tick,
            duration_ticks: abs_tick.saturating_sub(start_tick),
            channel,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bytes_returns_error() {
        assert!(load_midi(&[]).is_err());
    }

    #[test]
    fn defaults_inserted_when_absent() {
        // Minimal Format 0 SMF: MThd + empty MTrk, no tempo/timesig meta events
        #[rustfmt::skip]
        let bytes: &[u8] = &[
            // MThd
            0x4D, 0x54, 0x68, 0x64,  // "MThd"
            0x00, 0x00, 0x00, 0x06,  // chunk length = 6
            0x00, 0x00,              // format 0
            0x00, 0x01,              // 1 track
            0x01, 0xE0,              // 480 ticks/beat
            // MTrk
            0x4D, 0x54, 0x72, 0x6B,  // "MTrk"
            0x00, 0x00, 0x00, 0x04,  // chunk length = 4
            0x00, 0xFF, 0x2F, 0x00,  // delta=0, EndOfTrack meta
        ];
        let score = load_midi(bytes).unwrap();
        assert_eq!(score.ticks_per_beat, 480);
        assert_eq!(score.tempo_changes.len(), 1);
        assert_eq!(score.tempo_changes[0].us_per_beat, 500_000);
        assert_eq!(score.time_signature_changes.len(), 1);
        assert_eq!(score.time_signature_changes[0].numerator, 4);
    }

    #[test]
    fn note_on_vel0_is_note_off() {
        // Format 0, 480 tpb: NoteOn(60, 80) at tick 0, NoteOn(60, 0) at delta 480
        #[rustfmt::skip]
        let bytes: &[u8] = &[
            0x4D, 0x54, 0x68, 0x64, 0x00, 0x00, 0x00, 0x06,
            0x00, 0x00, 0x00, 0x01, 0x01, 0xE0,
            0x4D, 0x54, 0x72, 0x6B, 0x00, 0x00, 0x00, 0x0A,
            0x00, 0x90, 0x3C, 0x50,  // delta=0,  NoteOn ch0 pitch=60 vel=80
            0x83, 0x60, 0x90, 0x3C, 0x00,  // delta=480 (var-len), NoteOn ch0 pitch=60 vel=0
            0x00, 0xFF, 0x2F, 0x00,  // EndOfTrack
        ];
        let score = load_midi(bytes).unwrap();
        assert_eq!(score.tracks.len(), 1);
        let note = &score.tracks[0].notes[0];
        assert_eq!(note.pitch, 60);
        assert_eq!(note.duration_ticks, 480);
    }
}
