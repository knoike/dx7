//! Show GM program numbers used by MIDI files.
//! Usage: cargo run --example midi_programs -- file1.mid file2.mid ...

use midly::{Smf, TrackEventKind, MidiMessage};
use std::collections::BTreeSet;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("Usage: cargo run --example midi_programs -- file1.mid ...");
        std::process::exit(1);
    }

    let gm_names = [
        "Acoustic Grand Piano", "Bright Acoustic Piano", "Electric Grand Piano", "Honky-tonk Piano",
        "Electric Piano 1", "Electric Piano 2", "Harpsichord", "Clavinet",
        "Celesta", "Glockenspiel", "Music Box", "Vibraphone",
        "Marimba", "Xylophone", "Tubular Bells", "Dulcimer",
        "Drawbar Organ", "Percussive Organ", "Rock Organ", "Church Organ",
        "Reed Organ", "Accordion", "Harmonica", "Tango Accordion",
        "Acoustic Guitar (nylon)", "Acoustic Guitar (steel)", "Jazz Electric Guitar", "Clean Electric Guitar",
        "Muted Guitar", "Overdriven Guitar", "Distortion Guitar", "Guitar Harmonics",
        "Acoustic Bass", "Electric Bass (finger)", "Electric Bass (pick)", "Fretless Bass",
        "Slap Bass 1", "Slap Bass 2", "Synth Bass 1", "Synth Bass 2",
        "Violin", "Viola", "Cello", "Contrabass",
        "Tremolo Strings", "Pizzicato Strings", "Orchestral Harp", "Timpani",
        "String Ensemble 1", "String Ensemble 2", "Synth Strings 1", "Synth Strings 2",
        "Choir Aahs", "Voice Oohs", "Synth Voice", "Orchestra Hit",
        "Trumpet", "Trombone", "Tuba", "Muted Trumpet",
        "French Horn", "Brass Section", "Synth Brass 1", "Synth Brass 2",
        "Soprano Sax", "Alto Sax", "Tenor Sax", "Baritone Sax",
        "Oboe", "English Horn", "Bassoon", "Clarinet",
        "Piccolo", "Flute", "Recorder", "Pan Flute",
        "Blown Bottle", "Shakuhachi", "Whistle", "Ocarina",
        "Square Lead", "Sawtooth Lead", "Calliope Lead", "Chiff Lead",
        "Charang Lead", "Voice Lead", "Fifths Lead", "Bass + Lead",
        "New Age", "Warm Pad", "Polysynth", "Choir Pad",
        "Bowed Pad", "Metallic Pad", "Halo Pad", "Sweep Pad",
        "Rain", "Soundtrack", "Crystal", "Atmosphere",
        "Brightness", "Goblins", "Echoes", "Sci-fi",
        "Sitar", "Banjo", "Shamisen", "Koto",
        "Kalimba", "Bagpipe", "Fiddle", "Shanai",
        "Tinkle Bell", "Agogo", "Steel Drums", "Woodblock",
        "Taiko Drum", "Melodic Tom", "Synth Drum", "Reverse Cymbal",
        "Guitar Fret Noise", "Breath Noise", "Seashore", "Bird Tweet",
        "Telephone Ring", "Helicopter", "Applause", "Gunshot",
    ];

    // Collect all programs across all files
    let mut all_programs: BTreeSet<u8> = BTreeSet::new();

    for path in &args {
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) => { eprintln!("Cannot read {}: {}", path, e); continue; }
        };
        let smf = match Smf::parse(&data) {
            Ok(s) => s,
            Err(e) => { eprintln!("Cannot parse {}: {}", path, e); continue; }
        };

        let mut programs: BTreeSet<(u8, u8)> = BTreeSet::new(); // (channel, program)
        for track in &smf.tracks {
            for event in track {
                if let TrackEventKind::Midi { channel, message } = event.kind {
                    if let MidiMessage::ProgramChange { program } = message {
                        programs.insert((channel.as_int(), program.as_int()));
                    }
                }
            }
        }

        let fname = std::path::Path::new(path).file_name().unwrap().to_str().unwrap();
        println!("=== {} ===", fname);
        for &(ch, prog) in &programs {
            if ch == 9 {
                println!("  ch{:>2}: (drums)", ch);
            } else {
                let name = if (prog as usize) < gm_names.len() { gm_names[prog as usize] } else { "?" };
                println!("  ch{:>2}: {:>3} {}", ch, prog, name);
                all_programs.insert(prog);
            }
        }
        println!();
    }

    println!("=== All non-drum programs used ===");
    for prog in &all_programs {
        let name = if (*prog as usize) < gm_names.len() { gm_names[*prog as usize] } else { "?" };
        println!("  {:>3}: {}", prog, name);
    }
}
