//! Dump MIDI file structure — tracks, events, timing.
//! Usage: cargo run --example midi_dump -- file.mid

use midly::{Smf, TrackEventKind, MidiMessage, MetaMessage};

fn main() {
    let path = std::env::args().nth(1).expect("Usage: midi_dump <file.mid>");
    let data = std::fs::read(&path).unwrap();
    let smf = Smf::parse(&data).unwrap();

    println!("Format: {:?}, {} tracks, timing: {:?}", smf.header.format, smf.tracks.len(), smf.header.timing);

    for (t, track) in smf.tracks.iter().enumerate() {
        let mut note_count = 0u32;
        let mut first_note_tick = None;
        let mut last_note_tick = 0u64;
        let mut tick = 0u64;
        let mut channels = std::collections::BTreeSet::new();
        let mut programs = Vec::new();
        let mut track_name = String::new();

        for event in track {
            tick += event.delta.as_int() as u64;
            match event.kind {
                TrackEventKind::Midi { channel, message } => {
                    channels.insert(channel.as_int());
                    match message {
                        MidiMessage::NoteOn { vel, .. } if vel.as_int() > 0 => {
                            note_count += 1;
                            if first_note_tick.is_none() { first_note_tick = Some(tick); }
                            last_note_tick = tick;
                        }
                        MidiMessage::ProgramChange { program } => {
                            programs.push((tick, program.as_int()));
                        }
                        _ => {}
                    }
                }
                TrackEventKind::Meta(MetaMessage::TrackName(name)) => {
                    track_name = String::from_utf8_lossy(name).to_string();
                }
                TrackEventKind::Meta(MetaMessage::Tempo(t)) => {
                    let bpm = 60_000_000.0 / t.as_int() as f64;
                    println!("  Track {}: tempo change at tick {} -> {:.1} BPM", t, tick, bpm);
                }
                _ => {}
            }
        }

        if note_count > 0 || !track_name.is_empty() {
            let ch_str: Vec<String> = channels.iter().map(|c| format!("{}", c)).collect();
            println!("Track {:>2}: \"{}\" ch=[{}] notes={} ticks={}-{} programs={:?}",
                t, track_name, ch_str.join(","), note_count,
                first_note_tick.unwrap_or(0), last_note_tick, programs);
        }
    }

    // Show all note-on/off events of track 1
    if smf.tracks.len() > 1 {
        let note_names = ["C","C#","D","D#","E","F","F#","G","G#","A","A#","B"];
        println!("\n--- Track 1 all note events ---");
        let mut tick = 0u64;
        let tpb = match smf.header.timing {
            midly::Timing::Metrical(t) => t.as_int() as f64,
            _ => 384.0,
        };
        let mut active: std::collections::BTreeMap<u8, u64> = std::collections::BTreeMap::new();
        for event in &smf.tracks[1] {
            tick += event.delta.as_int() as u64;
            match event.kind {
                TrackEventKind::Midi { message: MidiMessage::NoteOn { key, vel }, .. } => {
                    let k = key.as_int();
                    let name = note_names[(k % 12) as usize];
                    let oct = (k as i32 / 12) - 1;
                    if vel.as_int() > 0 {
                        active.insert(k, tick);
                        println!("  tick {:>6} beat {:>6.1}  ON  {}{:<2} ({:>3}) vel {:>3}  active={}",
                            tick, tick as f64 / tpb, name, oct, k, vel.as_int(), active.len());
                    } else {
                        let dur = active.remove(&k).map(|s| tick - s).unwrap_or(0);
                        println!("  tick {:>6} beat {:>6.1}  OFF {}{:<2} ({:>3}) dur {:>5}  active={}",
                            tick, tick as f64 / tpb, name, oct, k, dur, active.len());
                    }
                }
                TrackEventKind::Midi { message: MidiMessage::NoteOff { key, .. }, .. } => {
                    let k = key.as_int();
                    let name = note_names[(k % 12) as usize];
                    let oct = (k as i32 / 12) - 1;
                    let dur = active.remove(&k).map(|s| tick - s).unwrap_or(0);
                    println!("  tick {:>6} beat {:>6.1}  OFF {}{:<2} ({:>3}) dur {:>5}  active={}",
                        tick, tick as f64 / tpb, name, oct, k, dur, active.len());
                }
                _ => {}
            }
        }
    }
}
