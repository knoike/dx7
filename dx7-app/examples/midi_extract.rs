//! Extract a single track from a MIDI file, optionally cutting at a tick.
//! Usage: cargo run --example midi_extract -- input.mid track_num [end_tick] [output.mid] [--pedal]
//!
//! --pedal: Add sustain pedal with legato changes every 2 beats (starting at beat 8)
//! Also fixes notes shorter than half a beat by extending them.

use midly::{Smf, Header, Format, Timing, TrackEvent, TrackEventKind, MidiMessage};
use std::fs;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: midi_extract <input.mid> <track#> [end_tick] [output.mid] [--pedal]");
        std::process::exit(1);
    }

    let input = &args[1];
    let track_num: usize = args[2].parse().expect("track# must be a number");
    let end_tick: Option<u64> = args.get(3).and_then(|s| s.parse().ok());
    let output = args.get(4)
        .filter(|s| !s.starts_with("--"))
        .map(|s| s.as_str())
        .unwrap_or("extracted.mid");
    let add_pedal = args.iter().any(|a| a == "--pedal");

    let data = fs::read(input).unwrap();
    let smf = Smf::parse(&data).unwrap();

    let tpb = match smf.header.timing {
        Timing::Metrical(t) => t.as_int() as u64,
        _ => 384,
    };

    // Track 0 is usually the tempo map
    let tempo_track = &smf.tracks[0];
    let source_track = &smf.tracks[track_num];

    // Filter tempo track events up to end_tick
    let mut new_tempo: Vec<TrackEvent<'_>> = Vec::new();
    let mut tick: u64 = 0;
    for event in tempo_track {
        tick += event.delta.as_int() as u64;
        if let Some(end) = end_tick {
            if tick > end { break; }
        }
        new_tempo.push(event.clone());
    }

    // Collect source track events as (absolute_tick, raw_bytes)
    let mut raw_events: Vec<(u64, Vec<u8>)> = Vec::new();
    tick = 0;
    for event in source_track {
        tick += event.delta.as_int() as u64;
        if let Some(end) = end_tick {
            if tick > end { break; }
        }
        let bytes = event_to_bytes(&event.kind);
        if !bytes.is_empty() {
            raw_events.push((tick, bytes));
        }
    }

    // Fix short notes: extend any note shorter than half a beat
    let min_dur = tpb / 2;
    fix_short_notes(&mut raw_events, min_dur);

    // Add sustain pedal events
    if add_pedal {
        add_sustain_pedal(&mut raw_events, tpb, 8, 1);
    }

    // Sort by tick (stable sort preserves order of simultaneous events)
    raw_events.sort_by_key(|e| e.0);

    let timing = smf.header.timing;
    write_midi_file(output, timing, &new_tempo, &raw_events);

    eprintln!("Extracted track {} to {}", track_num, output);
    if let Some(end) = end_tick {
        eprintln!("Cut at tick {} (~{:.1} beats)", end, end as f64 / tpb as f64);
    }
    if add_pedal {
        eprintln!("Added sustain pedal (legato changes every 2 beats from beat 8)");
    }
    eprintln!("Track events: {}", raw_events.len());
}

/// Convert a TrackEventKind to raw MIDI bytes.
fn event_to_bytes(kind: &TrackEventKind) -> Vec<u8> {
    let mut bytes = Vec::new();
    match kind {
        TrackEventKind::Midi { channel, message } => {
            let ch = channel.as_int();
            match message {
                MidiMessage::NoteOff { key, vel } => {
                    bytes.push(0x80 | ch);
                    bytes.push(key.as_int());
                    bytes.push(vel.as_int());
                }
                MidiMessage::NoteOn { key, vel } => {
                    bytes.push(0x90 | ch);
                    bytes.push(key.as_int());
                    bytes.push(vel.as_int());
                }
                MidiMessage::Aftertouch { key, vel } => {
                    bytes.push(0xA0 | ch);
                    bytes.push(key.as_int());
                    bytes.push(vel.as_int());
                }
                MidiMessage::Controller { controller, value } => {
                    bytes.push(0xB0 | ch);
                    bytes.push(controller.as_int());
                    bytes.push(value.as_int());
                }
                MidiMessage::ProgramChange { program } => {
                    bytes.push(0xC0 | ch);
                    bytes.push(program.as_int());
                }
                MidiMessage::ChannelAftertouch { vel } => {
                    bytes.push(0xD0 | ch);
                    bytes.push(vel.as_int());
                }
                MidiMessage::PitchBend { bend } => {
                    bytes.push(0xE0 | ch);
                    let raw = (bend.as_int() as i32 + 8192) as u16;
                    bytes.push((raw & 0x7F) as u8);
                    bytes.push(((raw >> 7) & 0x7F) as u8);
                }
            }
        }
        TrackEventKind::Meta(meta) => {
            bytes.push(0xFF);
            match meta {
                midly::MetaMessage::Tempo(t) => {
                    bytes.push(0x51);
                    bytes.push(3);
                    let v = t.as_int();
                    bytes.push((v >> 16) as u8);
                    bytes.push((v >> 8) as u8);
                    bytes.push(v as u8);
                }
                midly::MetaMessage::TrackName(name) => {
                    bytes.push(0x03);
                    write_vlq(&mut bytes, name.len() as u32);
                    bytes.extend_from_slice(name);
                }
                midly::MetaMessage::EndOfTrack => {
                    bytes.push(0x2F);
                    bytes.push(0);
                }
                midly::MetaMessage::TimeSignature(n, d, c, b) => {
                    bytes.push(0x58);
                    bytes.push(4);
                    bytes.push(*n);
                    bytes.push(*d);
                    bytes.push(*c);
                    bytes.push(*b);
                }
                midly::MetaMessage::KeySignature(sf, mi) => {
                    bytes.push(0x59);
                    bytes.push(2);
                    bytes.push(*sf as u8);
                    bytes.push(*mi as u8);
                }
                _ => {
                    bytes.clear(); // skip unknown meta events
                }
            }
        }
        TrackEventKind::SysEx(_) | TrackEventKind::Escape(_) => {}
    }
    bytes
}

/// Fix notes shorter than min_dur ticks by moving their note-off later.
fn fix_short_notes(events: &mut Vec<(u64, Vec<u8>)>, min_dur: u64) {
    let len = events.len();
    for i in 0..len {
        let (on_tick, ref on_bytes) = events[i];
        // Check if this is a NoteOn with velocity > 0
        if on_bytes.len() == 3 && (on_bytes[0] & 0xF0) == 0x90 && on_bytes[2] > 0 {
            let note = on_bytes[1];
            let ch = on_bytes[0] & 0x0F;
            // Find the matching note-off
            for j in (i + 1)..len {
                let (off_tick, ref off_bytes) = events[j];
                let is_note_off = if off_bytes.len() == 3 {
                    // Explicit NoteOff
                    (off_bytes[0] & 0xF0) == 0x80
                        && (off_bytes[0] & 0x0F) == ch
                        && off_bytes[1] == note
                } else {
                    false
                } || if off_bytes.len() == 3 {
                    // NoteOn with vel=0
                    (off_bytes[0] & 0xF0) == 0x90
                        && (off_bytes[0] & 0x0F) == ch
                        && off_bytes[1] == note
                        && off_bytes[2] == 0
                } else {
                    false
                };

                if is_note_off {
                    let dur = off_tick - on_tick;
                    if dur < min_dur {
                        let new_tick = on_tick + min_dur;
                        eprintln!(
                            "  Fixed short note {} (ch{}) at tick {}: {} -> {} ticks",
                            note, ch, on_tick, dur, min_dur
                        );
                        events[j].0 = new_tick;
                    }
                    break;
                }
            }
        }
    }
}

/// Add sustain pedal (CC64) with changes at bass note changes.
/// Detects when the lowest sounding note changes — that's a chord change,
/// so pedal lifts briefly to clear the old harmony.
fn add_sustain_pedal(
    events: &mut Vec<(u64, Vec<u8>)>,
    tpb: u64,
    start_beat: u64,
    _interval_beats: u64, // unused, kept for API compat
) {
    let ch: u8 = 0;
    let pedal_on = vec![0xB0 | ch, 64, 127];
    let pedal_off = vec![0xB0 | ch, 64, 0];
    let first_tick = start_beat * tpb;

    // Simulate note tracking to find bass note changes
    let mut active_notes: std::collections::BTreeSet<u8> = std::collections::BTreeSet::new();
    let mut current_bass: Option<u8> = None;
    let mut pedal_change_ticks: Vec<u64> = Vec::new();

    // Sort a copy by tick for scanning
    let mut sorted: Vec<(u64, Vec<u8>)> = events.iter().cloned().collect();
    sorted.sort_by_key(|e| e.0);

    for (tick, bytes) in &sorted {
        if bytes.len() != 3 { continue; }
        let status = bytes[0] & 0xF0;
        let note = bytes[1];

        match status {
            0x90 if bytes[2] > 0 => {
                // Note ON
                active_notes.insert(note);
            }
            0x80 | 0x90 => {
                // Note OFF (0x80 or 0x90 with vel=0)
                active_notes.remove(&note);
            }
            _ => continue,
        }

        // Check if bass note changed
        let new_bass = active_notes.iter().next().copied();
        if *tick >= first_tick && new_bass != current_bass {
            if let Some(_old) = current_bass {
                if new_bass.is_some() {
                    pedal_change_ticks.push(*tick);
                }
            }
            current_bass = new_bass;
        }
    }

    // Deduplicate close changes (within half a beat)
    let min_gap = tpb / 2;
    let mut filtered: Vec<u64> = Vec::new();
    for t in &pedal_change_ticks {
        if filtered.last().map_or(true, |&prev| *t - prev >= min_gap) {
            filtered.push(*t);
        }
    }

    eprintln!("  Pedal changes at {} points (bass note changes)", filtered.len());
    let last_tick = events.iter().map(|e| e.0).max().unwrap_or(0);

    // Initial pedal ON
    events.push((first_tick, pedal_on.clone()));

    // Legato pedal changes at each bass note change
    for tick in &filtered {
        // Brief lift: OFF 15 ticks before, ON at the change
        events.push((tick.saturating_sub(15), pedal_off.clone()));
        events.push((*tick, pedal_on.clone()));
    }

    // Final pedal OFF
    events.push((last_tick, pedal_off));
}

fn write_vlq(buf: &mut Vec<u8>, mut val: u32) {
    let mut bytes = Vec::new();
    bytes.push((val & 0x7F) as u8);
    val >>= 7;
    while val > 0 {
        bytes.push((val & 0x7F) as u8 | 0x80);
        val >>= 7;
    }
    bytes.reverse();
    buf.extend_from_slice(&bytes);
}

fn write_midi_file(
    path: &str,
    timing: Timing,
    tempo_track: &[TrackEvent],
    music_events: &[(u64, Vec<u8>)],
) {
    let mut out = Vec::new();

    // MThd header
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&6u32.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes()); // format 1
    out.extend_from_slice(&2u16.to_be_bytes()); // 2 tracks
    let tpb = match timing {
        Timing::Metrical(t) => t.as_int(),
        _ => 384,
    };
    out.extend_from_slice(&tpb.to_be_bytes());

    // Write tempo track (unchanged)
    {
        let mut trk_data = Vec::new();
        for event in tempo_track {
            write_vlq(&mut trk_data, event.delta.as_int());
            let bytes = event_to_bytes(&event.kind);
            trk_data.extend_from_slice(&bytes);
        }
        if !trk_data.ends_with(&[0xFF, 0x2F, 0x00]) {
            trk_data.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
        }
        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(trk_data.len() as u32).to_be_bytes());
        out.extend_from_slice(&trk_data);
    }

    // Write music track from absolute-tick raw events
    {
        let mut trk_data = Vec::new();
        let mut prev_tick: u64 = 0;
        for (tick, bytes) in music_events {
            let delta = tick.saturating_sub(prev_tick);
            write_vlq(&mut trk_data, delta as u32);
            trk_data.extend_from_slice(bytes);
            prev_tick = *tick;
        }
        if !trk_data.ends_with(&[0xFF, 0x2F, 0x00]) {
            trk_data.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
        }
        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(trk_data.len() as u32).to_be_bytes());
        out.extend_from_slice(&trk_data);
    }

    fs::write(path, &out).unwrap();
}
