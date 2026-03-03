//! Show velocity statistics per channel for a MIDI file.
use midly::{Smf, TrackEventKind, MidiMessage};

fn main() {
    let path = std::env::args().nth(1).expect("Usage: midi_velocity <file.mid>");
    let data = std::fs::read(&path).unwrap();
    let smf = Smf::parse(&data).unwrap();

    let mut stats: Vec<Vec<u8>> = vec![Vec::new(); 16];

    for track in &smf.tracks {
        for event in track {
            if let TrackEventKind::Midi { channel, message } = event.kind {
                if let MidiMessage::NoteOn { vel, .. } = message {
                    let v = vel.as_int();
                    if v > 0 {
                        stats[channel.as_int() as usize].push(v);
                    }
                }
            }
        }
    }

    for (ch, vels) in stats.iter().enumerate() {
        if vels.is_empty() { continue; }
        let min = *vels.iter().min().unwrap();
        let max = *vels.iter().max().unwrap();
        let avg = vels.iter().map(|&v| v as f64).sum::<f64>() / vels.len() as f64;
        println!("ch{:>2}: {:>5} notes, vel min={:>3} max={:>3} avg={:.0}", ch, vels.len(), min, max, avg);
    }
}
