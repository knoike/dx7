//! DX7 FM Synthesizer — Desktop Application
//!
//! Usage:
//!   dx7-app                          # Interactive mode with keyboard/MIDI
//!   dx7-app --render output.wav      # Offline WAV rendering
//!   dx7-app --sysex sysex/rom1a.syx   # Load patches from SysEx file
//!   dx7-app --list-midi              # List MIDI ports

mod audio;
mod keyboard;
mod midi;

use clap::Parser;
use crossterm::terminal;
use dx7_core::{get_rom1a_preset, DxVoice, SynthCommand};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "dx7", about = "DX7 FM Synthesizer Emulator")]
struct Args {
    /// Render to WAV file instead of real-time playback
    #[arg(long)]
    render: Option<String>,

    /// SysEx file to load patches from
    #[arg(long)]
    sysex: Option<String>,

    /// Patch index to load (0-31)
    #[arg(long, default_value = "3")]
    patch: usize,

    /// MIDI note for WAV rendering
    #[arg(long, default_value = "60")]
    note: u8,

    /// Velocity for WAV rendering
    #[arg(long, default_value = "100")]
    velocity: u8,

    /// Duration in seconds for WAV rendering
    #[arg(long, default_value = "3")]
    duration: f64,

    /// Sample rate
    #[arg(long, default_value = "44100")]
    sample_rate: u32,

    /// MIDI port name (partial match)
    #[arg(long)]
    midi_port: Option<String>,

    /// List available MIDI ports and exit
    #[arg(long)]
    list_midi: bool,

    /// Render a MIDI file to WAV
    #[arg(long)]
    midi_file: Option<String>,

    /// MIDI track index to render (default: all tracks)
    #[arg(long)]
    track: Option<Vec<usize>>,

}

fn main() {
    let args = Args::parse();

    // List MIDI ports
    if args.list_midi {
        let ports = midi::MidiHandler::list_ports();
        if ports.is_empty() {
            println!("No MIDI input ports found.");
        } else {
            println!("Available MIDI input ports:");
            for (i, name) in ports.iter().enumerate() {
                println!("  {}: {}", i, name);
            }
        }
        return;
    }

    // Load patches
    let patches = load_patches(&args);
    let patch_idx = args.patch.min(patches.len() - 1);
    let initial_patch = patches[patch_idx].clone();

    println!("DX7 FM Synthesizer");
    println!("Patch: {} ({})", patch_idx, initial_patch.name_str());

    // MIDI file rendering mode
    if let Some(ref midi_path) = args.midi_file {
        let output_path = args.render.as_deref().unwrap_or("output.wav");
        render_midi_file(midi_path, output_path, &initial_patch, &patches, &args);
        return;
    }

    // WAV rendering mode (single note)
    if let Some(ref output_path) = args.render {
        render_wav(output_path, &initial_patch, &args);
        return;
    }

    // Interactive mode
    run_interactive(initial_patch, patches, &args);
}

fn load_patches(args: &Args) -> Vec<DxVoice> {
    if let Some(ref syx_path) = args.sysex {
        match std::fs::read(syx_path) {
            Ok(data) => match DxVoice::parse_bulk_dump(&data) {
                Ok(voices) => {
                    println!("Loaded {} patches from {}", voices.len(), syx_path);
                    for (i, v) in voices.iter().enumerate() {
                        println!("  {}: {}", i, v.name_str());
                    }
                    return voices;
                }
                Err(e) => {
                    eprintln!("Failed to parse SysEx file: {e}");
                }
            },
            Err(e) => {
                eprintln!("Failed to read SysEx file: {e}");
            }
        }
    }

    // Fall back to built-in presets
    (0..32).map(get_rom1a_preset).collect()
}

fn render_wav(output_path: &str, patch: &DxVoice, args: &Args) {
    let sample_rate = args.sample_rate;
    let mut synth = dx7_core::Synth::new(sample_rate as f64);
    synth.load_patch(patch.clone());

    let note_samples = (args.duration * sample_rate as f64) as usize;
    let tail_samples = (2.0 * sample_rate as f64) as usize;
    let total_samples = note_samples + tail_samples;

    println!(
        "Rendering: note={}, vel={}, duration={}s, tail=2s",
        args.note, args.velocity, args.duration
    );

    synth.note_on(args.note, args.velocity);

    let mut all_samples = vec![0.0f32; total_samples];

    synth.render_mono(&mut all_samples[..note_samples]);
    synth.note_off(args.note);
    synth.render_mono(&mut all_samples[note_samples..]);

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer =
        hound::WavWriter::create(output_path, spec).expect("Failed to create WAV file");

    for &sample in &all_samples {
        let i16_sample = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
        writer.write_sample(i16_sample).unwrap();
    }

    writer.finalize().unwrap();
    println!("Written {} samples to {}", total_samples, output_path);
}

/// A MIDI event parsed from a Standard MIDI File.
#[derive(Debug, Clone)]
struct MidiEvent {
    tick: u64,
    channel: u8,
    kind: MidiEventKind,
}

#[derive(Debug, Clone)]
enum MidiEventKind {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
    ControlChange { cc: u8, value: u8 },
    PitchBend { value: i16 },
    ProgramChange { program: u8 },
    Tempo(u32), // microseconds per beat
}

/// Parse a Standard MIDI File, returning events sorted by tick.
fn parse_midi_file(data: &[u8], track_filter: &Option<Vec<usize>>) -> (u16, Vec<MidiEvent>) {
    // Header
    assert_eq!(&data[0..4], b"MThd");
    let _hdr_len = u32::from_be_bytes(data[4..8].try_into().unwrap());
    let _fmt = u16::from_be_bytes(data[8..10].try_into().unwrap());
    let ntracks = u16::from_be_bytes(data[10..12].try_into().unwrap());
    let division = u16::from_be_bytes(data[12..14].try_into().unwrap());

    let mut all_events = Vec::new();
    let mut pos = 14usize;

    for track_idx in 0..ntracks as usize {
        if &data[pos..pos + 4] != b"MTrk" {
            break;
        }
        let trk_len = u32::from_be_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let trk_start = pos + 8;
        let trk_end = trk_start + trk_len;

        // Always parse track 0 (tempo map), plus selected tracks
        let include = track_idx == 0
            || match track_filter {
                Some(tracks) => tracks.contains(&track_idx),
                None => true,
            };

        if include {
            let mut tpos = trk_start;
            let mut tick: u64 = 0;
            let mut running_status: u8 = 0;

            while tpos < trk_end {
                // Variable-length delta
                let mut delta: u64 = 0;
                loop {
                    let b = data[tpos];
                    tpos += 1;
                    delta = (delta << 7) | (b & 0x7F) as u64;
                    if b & 0x80 == 0 {
                        break;
                    }
                }
                tick += delta;

                let status = data[tpos];
                if status & 0x80 != 0 {
                    running_status = status;
                    tpos += 1;
                }
                let cmd = running_status & 0xF0;
                let channel = running_status & 0x0F;

                match cmd {
                    0x90 if tpos + 1 < trk_end => {
                        let note = data[tpos];
                        let vel = data[tpos + 1];
                        tpos += 2;
                        if vel > 0 {
                            all_events.push(MidiEvent {
                                tick,
                                channel,
                                kind: MidiEventKind::NoteOn { note, velocity: vel },
                            });
                        } else {
                            all_events.push(MidiEvent {
                                tick,
                                channel,
                                kind: MidiEventKind::NoteOff { note },
                            });
                        }
                    }
                    0x80 if tpos + 1 < trk_end => {
                        let note = data[tpos];
                        tpos += 2;
                        all_events.push(MidiEvent {
                            tick,
                            channel,
                            kind: MidiEventKind::NoteOff { note },
                        });
                    }
                    0xB0 if tpos + 1 < trk_end => {
                        let cc = data[tpos];
                        let val = data[tpos + 1];
                        tpos += 2;
                        all_events.push(MidiEvent {
                            tick,
                            channel,
                            kind: MidiEventKind::ControlChange { cc, value: val },
                        });
                    }
                    0xE0 if tpos + 1 < trk_end => {
                        let lsb = data[tpos];
                        let msb = data[tpos + 1];
                        tpos += 2;
                        let bend = ((msb as i16) << 7 | lsb as i16) - 8192;
                        all_events.push(MidiEvent {
                            tick,
                            channel,
                            kind: MidiEventKind::PitchBend { value: bend },
                        });
                    }
                    0xC0 => {
                        let program = data[tpos];
                        tpos += 1;
                        all_events.push(MidiEvent {
                            tick,
                            channel,
                            kind: MidiEventKind::ProgramChange { program },
                        });
                    }
                    0xA0 | 0xB0 | 0xE0 => {
                        tpos += 2;
                    }
                    0xD0 => {
                        tpos += 1;
                    }
                    _ if running_status == 0xFF => {
                        // Meta event — status byte was consumed, next is type
                        let meta_type = data[tpos - 1]; // we already advanced past it
                        // Actually: when running_status=0xFF, the flow is different.
                        // Let me re-parse: after 0xFF we read meta_type then var-len then data
                        // But our code set running_status=0xFF and tpos is past it.
                        // data[tpos-1] was already read. Let me handle this:
                        let mt = if status == 0xFF {
                            let mt = data[tpos];
                            tpos += 1;
                            mt
                        } else {
                            // running_status shouldn't be 0xFF in normal cases
                            0
                        };
                        let mut meta_len: usize = 0;
                        loop {
                            let b = data[tpos];
                            tpos += 1;
                            meta_len = (meta_len << 7) | (b & 0x7F) as usize;
                            if b & 0x80 == 0 {
                                break;
                            }
                        }
                        if mt == 0x51 && meta_len >= 3 {
                            // Tempo
                            let tempo = ((data[tpos] as u32) << 16)
                                | ((data[tpos + 1] as u32) << 8)
                                | data[tpos + 2] as u32;
                            all_events.push(MidiEvent {
                                tick,
                                channel: 0,
                                kind: MidiEventKind::Tempo(tempo),
                            });
                        }
                        tpos += meta_len;
                    }
                    _ if status == 0xF0 || status == 0xF7 => {
                        // SysEx
                        let mut syx_len: usize = 0;
                        loop {
                            let b = data[tpos];
                            tpos += 1;
                            syx_len = (syx_len << 7) | (b & 0x7F) as usize;
                            if b & 0x80 == 0 {
                                break;
                            }
                        }
                        tpos += syx_len;
                    }
                    _ => {
                        // Unknown, skip 2 bytes as best guess
                        tpos += 2;
                    }
                }
            }
        }
        pos = trk_end;
    }

    // Sort by tick (stable sort preserves order for same-tick events)
    all_events.sort_by_key(|e| e.tick);

    (division, all_events)
}

// ── Synthesized Drum Machine (GM channel 9) ──────────────────────────

struct DrumHit {
    age: f64,
    velocity: f32,
    tone_freq: f64,
    tone_freq_end: f64,
    tone_decay: f64,
    noise_level: f32,
    noise_decay: f64,
    noise_hp_alpha: f64,
    noise_hp_prev: f64,
    phase: f64,
}

struct DrumMachine {
    sample_rate: f64,
    hits: Vec<DrumHit>,
    rng_state: u32,
}

/// Synthesis parameters for a GM drum sound.
struct DrumParams {
    tone_freq: f64,
    tone_freq_end: f64,
    tone_decay: f64,
    noise_level: f32,
    noise_decay: f64,
    noise_hp_alpha: f64, // 0.0 = no HP filter, >0 = high-pass
}

impl DrumMachine {
    fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            hits: Vec::new(),
            rng_state: 0xDEAD_BEEF,
        }
    }

    /// Map a GM drum note to synthesis parameters.
    fn drum_params(&self, note: u8) -> Option<DrumParams> {
        // High-pass alpha for hi-hats / cymbals at different cutoffs
        let hp_high = 1.0 - (-2.0 * std::f64::consts::PI * 8000.0 / self.sample_rate).exp();
        let hp_mid = 1.0 - (-2.0 * std::f64::consts::PI * 4000.0 / self.sample_rate).exp();

        match note {
            // Kick drum
            35 | 36 => Some(DrumParams {
                tone_freq: 150.0, tone_freq_end: 45.0, tone_decay: 0.15,
                noise_level: 0.15, noise_decay: 0.04, noise_hp_alpha: 0.0,
            }),
            // Side Stick
            37 => Some(DrumParams {
                tone_freq: 400.0, tone_freq_end: 400.0, tone_decay: 0.02,
                noise_level: 0.5, noise_decay: 0.02, noise_hp_alpha: 0.0,
            }),
            // Snare
            38 | 40 => Some(DrumParams {
                tone_freq: 180.0, tone_freq_end: 150.0, tone_decay: 0.08,
                noise_level: 0.7, noise_decay: 0.15, noise_hp_alpha: 0.0,
            }),
            // Clap
            39 => Some(DrumParams {
                tone_freq: 0.0, tone_freq_end: 0.0, tone_decay: 0.0,
                noise_level: 1.0, noise_decay: 0.12, noise_hp_alpha: 0.0,
            }),
            // Closed Hi-Hat
            42 | 44 => Some(DrumParams {
                tone_freq: 0.0, tone_freq_end: 0.0, tone_decay: 0.0,
                noise_level: 0.6, noise_decay: 0.04, noise_hp_alpha: hp_high,
            }),
            // Open Hi-Hat
            46 => Some(DrumParams {
                tone_freq: 0.0, tone_freq_end: 0.0, tone_decay: 0.0,
                noise_level: 0.6, noise_decay: 0.18, noise_hp_alpha: hp_high,
            }),
            // Toms — wide pitch spread so low vs high is distinct
            41 => Some(DrumParams {
                tone_freq: 80.0, tone_freq_end: 55.0, tone_decay: 0.22,
                noise_level: 0.2, noise_decay: 0.06, noise_hp_alpha: 0.0,
            }),
            43 => Some(DrumParams {
                tone_freq: 110.0, tone_freq_end: 80.0, tone_decay: 0.19,
                noise_level: 0.2, noise_decay: 0.06, noise_hp_alpha: 0.0,
            }),
            45 => Some(DrumParams {
                tone_freq: 150.0, tone_freq_end: 110.0, tone_decay: 0.16,
                noise_level: 0.2, noise_decay: 0.06, noise_hp_alpha: 0.0,
            }),
            47 => Some(DrumParams {
                tone_freq: 200.0, tone_freq_end: 155.0, tone_decay: 0.14,
                noise_level: 0.2, noise_decay: 0.06, noise_hp_alpha: 0.0,
            }),
            48 => Some(DrumParams {
                tone_freq: 260.0, tone_freq_end: 210.0, tone_decay: 0.12,
                noise_level: 0.2, noise_decay: 0.06, noise_hp_alpha: 0.0,
            }),
            50 => Some(DrumParams {
                tone_freq: 320.0, tone_freq_end: 270.0, tone_decay: 0.10,
                noise_level: 0.2, noise_decay: 0.06, noise_hp_alpha: 0.0,
            }),
            // Crash cymbals
            49 | 52 | 55 | 57 => Some(DrumParams {
                tone_freq: 0.0, tone_freq_end: 0.0, tone_decay: 0.0,
                noise_level: 0.5, noise_decay: 0.6, noise_hp_alpha: hp_mid,
            }),
            // Ride cymbals
            51 | 53 | 59 => Some(DrumParams {
                tone_freq: 0.0, tone_freq_end: 0.0, tone_decay: 0.0,
                noise_level: 0.4, noise_decay: 0.35, noise_hp_alpha: hp_mid,
            }),
            _ => None,
        }
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        if let Some(p) = self.drum_params(note) {
            self.hits.push(DrumHit {
                age: 0.0,
                velocity: velocity as f32 / 127.0,
                tone_freq: p.tone_freq,
                tone_freq_end: p.tone_freq_end,
                tone_decay: p.tone_decay,
                noise_level: p.noise_level,
                noise_decay: p.noise_decay,
                noise_hp_alpha: p.noise_hp_alpha,
                noise_hp_prev: 0.0,
                phase: 0.0,
            });
        }
    }

    fn render(&mut self, output: &mut [f32]) {
        let dt = 1.0 / self.sample_rate;
        let level = 0.02f32; // drum mix level (below synth 0.05)

        for sample in output.iter_mut() {
            let mut sum = 0.0f32;

            for hit in self.hits.iter_mut() {
                let vel = hit.velocity;

                // Tone component (sine with pitch envelope)
                let mut tone_out = 0.0f32;
                if hit.tone_decay > 0.0 {
                    let pitch_t = (hit.age / hit.tone_decay).min(1.0);
                    let freq = hit.tone_freq + (hit.tone_freq_end - hit.tone_freq) * pitch_t;
                    hit.phase += freq * dt;
                    if hit.phase > 1.0 { hit.phase -= 1.0; }
                    let tone_env = (-hit.age / hit.tone_decay).exp() as f32;
                    tone_out = (hit.phase * 2.0 * std::f64::consts::PI).sin() as f32
                        * vel * tone_env;
                }

                // Noise component
                let mut noise_out = 0.0f32;
                if hit.noise_decay > 0.0 && hit.noise_level > 0.0 {
                    // xorshift32 PRNG
                    self.rng_state ^= self.rng_state << 13;
                    self.rng_state ^= self.rng_state >> 17;
                    self.rng_state ^= self.rng_state << 5;
                    let noise_raw = (self.rng_state as f64 / u32::MAX as f64) * 2.0 - 1.0;

                    let noise_env = (-hit.age / hit.noise_decay).exp() as f32;
                    noise_out = noise_raw as f32 * vel * hit.noise_level * noise_env;

                    // Optional high-pass filter
                    if hit.noise_hp_alpha > 0.0 {
                        let filtered = noise_out as f64 - hit.noise_hp_prev;
                        hit.noise_hp_prev += hit.noise_hp_alpha * filtered;
                        noise_out = filtered as f32;
                    }
                }

                sum += (tone_out + noise_out) * level;
                hit.age += dt;
            }

            *sample += sum;
        }

        // Prune dead hits (both envelopes decayed below threshold)
        self.hits.retain(|h| {
            let tone_alive = h.tone_decay > 0.0 && (-h.age / h.tone_decay).exp() > 0.001;
            let noise_alive = h.noise_decay > 0.0
                && h.noise_level > 0.0
                && (-h.age / h.noise_decay).exp() > 0.001;
            tone_alive || noise_alive
        });
    }
}

fn render_midi_file(midi_path: &str, output_path: &str, patch: &DxVoice, patches: &[DxVoice], args: &Args) {
    let data = std::fs::read(midi_path).expect("Failed to read MIDI file");
    let (division, events) = parse_midi_file(&data, &args.track);

    println!(
        "MIDI: {} events, {} ticks/beat",
        events.len(),
        division
    );

    let sample_rate = args.sample_rate;

    // Create 16 independent synth instances (one per MIDI channel)
    let mut synths: Vec<dx7_core::Synth> = (0..16)
        .map(|_| {
            let mut s = dx7_core::Synth::new(sample_rate as f64);
            s.load_patch(patch.clone());
            s.set_master_volume(0.05);
            s
        })
        .collect();

    // Drum machine for GM channel 10 (index 9)
    let mut drums = DrumMachine::new(sample_rate as f64);

    // Render by walking through events, converting tick→sample position
    let mut tempo: u32 = 500_000; // default 120 BPM (standard MIDI default)
    let mut current_tick: u64 = 0;
    let mut current_sample: u64 = 0;
    let mut samples: Vec<f32> = Vec::new();

    for event in &events {
        if event.tick > current_tick {
            // Render samples between current position and this event
            let delta_ticks = event.tick - current_tick;
            let delta_samples = (delta_ticks as f64 * tempo as f64 * sample_rate as f64)
                / (division as f64 * 1_000_000.0);
            let delta_samples = delta_samples as u64;

            if delta_samples > 0 {
                let start = samples.len();
                samples.resize(start + delta_samples as usize, 0.0);
                let slice = &mut samples[start..];

                // Mix all 16 synths into the output
                let mut temp = vec![0.0f32; slice.len()];
                for synth in &mut synths {
                    temp.iter_mut().for_each(|s| *s = 0.0);
                    synth.render_mono(&mut temp);
                    for (out, t) in slice.iter_mut().zip(temp.iter()) {
                        *out += *t;
                    }
                }

                // Mix drums
                drums.render(slice);
            }
            current_tick = event.tick;
            current_sample += delta_samples;
        }

        let ch = event.channel as usize;

        // Route drum channel (GM channel 10 = index 9) to drum machine
        if ch == 9 {
            if let MidiEventKind::NoteOn { note, velocity } = &event.kind {
                drums.note_on(*note, *velocity);
            }
            // Handle tempo changes even on ch9
            if let MidiEventKind::Tempo(t) = &event.kind {
                tempo = *t;
            }
            continue;
        }

        match &event.kind {
            MidiEventKind::NoteOn { note, velocity } => {
                synths[ch].note_on(*note, *velocity);
            }
            MidiEventKind::NoteOff { note } => {
                synths[ch].note_off(*note);
            }
            MidiEventKind::ControlChange { cc, value } => {
                // Skip CC7 (volume) — we use fixed master_volume + normalization
                if *cc != 7 {
                    synths[ch].control_change(*cc, *value);
                }
            }
            MidiEventKind::PitchBend { value } => {
                synths[ch].pitch_bend(*value);
            }
            MidiEventKind::ProgramChange { program } => {
                if let Some(p) = patches.get(*program as usize) {
                    synths[ch].load_patch(p.clone());
                }
            }
            MidiEventKind::Tempo(t) => {
                tempo = *t;
            }
        }
    }

    // Render 4s tail for reverb decay
    let tail = (4.0 * sample_rate as f64) as usize;
    let start = samples.len();
    samples.resize(start + tail, 0.0);
    let slice = &mut samples[start..];
    let mut temp = vec![0.0f32; slice.len()];
    for synth in &mut synths {
        temp.iter_mut().for_each(|s| *s = 0.0);
        synth.render_mono(&mut temp);
        for (out, t) in slice.iter_mut().zip(temp.iter()) {
            *out += *t;
        }
    }
    drums.render(slice);

    // Apply DX7-style output low-pass filter (4th-order Butterworth at 10.5 kHz)
    // Simulates the reconstruction filter in the DX7's analog output stage
    let mut lpf = dx7_core::effects::LowPassFilter4::new(sample_rate as f64, 10500.0);
    lpf.process(&mut samples);

    // Apply soft saturation for analog warmth
    for s in samples.iter_mut() {
        *s = dx7_core::effects::soft_saturate(*s);
    }

    // Mono → stereo (no chorus)
    let num = samples.len();
    let mut left = samples.clone();
    let mut right = samples.clone();

    // Apply reverb (mono sum → stereo reverb, blended in)
    let mut reverb = dx7_core::Reverb::new(sample_rate as f32);
    reverb.set_params(0.88, 0.35, 0.13);
    let mono_for_rev: Vec<f32> = (0..num).map(|i| (left[i] + right[i]) * 0.5).collect();
    let mut rev_l = vec![0.0f32; num];
    let mut rev_r = vec![0.0f32; num];
    reverb.process_mono_to_stereo(&mono_for_rev, &mut rev_l, &mut rev_r);
    for i in 0..num {
        left[i] = left[i] * 0.85 + rev_l[i] * 0.15;
        right[i] = right[i] * 0.85 + rev_r[i] * 0.15;
    }

    // Normalize stereo output to -1 dB headroom
    let peak = left
        .iter()
        .chain(right.iter())
        .map(|s| s.abs())
        .fold(0.0f32, f32::max);
    if peak > 0.0001 {
        let target = 0.891; // -1 dB headroom
        let scale = target / peak;
        for s in left.iter_mut() {
            *s *= scale;
        }
        for s in right.iter_mut() {
            *s *= scale;
        }
        eprintln!(
            "Normalized: peak {:.4} -> {:.3} (scale {:.3})",
            peak, target, scale
        );
    }

    let duration_secs = num as f64 / sample_rate as f64;
    println!(
        "Rendered {:.1}s ({} samples, stereo) to {}",
        duration_secs, num, output_path
    );

    // Write stereo WAV
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::create(output_path, spec).expect("Failed to create WAV file");
    for i in 0..num {
        let l = (left[i] * 32767.0).clamp(-32768.0, 32767.0) as i16;
        let r = (right[i] * 32767.0).clamp(-32768.0, 32767.0) as i16;
        writer.write_sample(l).unwrap();
        writer.write_sample(r).unwrap();
    }
    writer.finalize().unwrap();
}

fn run_interactive(initial_patch: DxVoice, patches: Vec<DxVoice>, args: &Args) {
    let engine = match audio::AudioEngine::start(initial_patch) {
        Ok(e) => e,
        Err(err) => {
            eprintln!("Failed to start audio: {err}");
            return;
        }
    };

    println!("Audio: {} Hz", engine.sample_rate);

    // Try to connect MIDI using the shared command channel
    let midi_handler = match midi::MidiHandler::connect(
        args.midi_port.as_deref(),
        engine.command_sender(),
    ) {
        Ok(h) => {
            println!("MIDI: {}", h.port_name);
            Some(h)
        }
        Err(e) => {
            println!("MIDI: not connected ({e})");
            None
        }
    };
    let _ = midi_handler; // Keep connection alive

    println!();
    println!("Controls:");
    println!("  ASDFGHJKL; = white keys (C4-E5)");
    println!("  WETYUOP    = black keys");
    println!("  Z/X        = octave down/up");
    println!("  1-9,0      = select patch");
    println!("  Q/Esc      = quit");
    println!();

    terminal::enable_raw_mode().expect("Failed to enable raw mode");

    let mut kbd = keyboard::KeyboardHandler::new();

    loop {
        let events = kbd.poll(Duration::from_millis(10));

        for event in &events {
            match event {
                keyboard::KeyboardEvent::NoteOn(note, vel) => {
                    engine.send_command(SynthCommand::NoteOn {
                        note: *note,
                        velocity: *vel,
                    });
                }
                keyboard::KeyboardEvent::NoteOff(note) => {
                    engine.send_command(SynthCommand::NoteOff { note: *note });
                }
                keyboard::KeyboardEvent::OctaveChange(oct) => {
                    print!("\r\x1b[K  Octave: {:+}\r", oct);
                }
                keyboard::KeyboardEvent::PatchChange(idx) => {
                    let idx = *idx as usize;
                    if idx < patches.len() {
                        let patch = patches[idx].clone();
                        let name = patch.name_str().to_string();
                        engine.send_command(SynthCommand::LoadPatch(Box::new(patch)));
                        print!("\r\x1b[K  Patch {}: {}\r", idx, name);
                    }
                }
                keyboard::KeyboardEvent::Quit => {
                    terminal::disable_raw_mode().ok();
                    println!("\r\nBye!");
                    return;
                }
            }
        }
    }
}
