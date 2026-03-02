//! Polyphonic DX7 synthesizer engine.

use crate::patch::DxVoice;
use crate::tables::{self, N};
use crate::voice::{Voice, VoiceState};

const MAX_VOICES: usize = 32;

/// Commands that can be sent to the synth from other threads.
#[derive(Clone, Debug)]
pub enum SynthCommand {
    NoteOn { note: u8, velocity: u8 },
    NoteOff { note: u8 },
    PitchBend { value: i16 },
    ControlChange { cc: u8, value: u8 },
    LoadPatch(Box<DxVoice>),
}

/// The main polyphonic synthesizer.
pub struct Synth {
    voices: Vec<Voice>,
    current_patch: DxVoice,
    _sample_rate: f64,
    sustain: bool,
    sustained_notes: Vec<u8>,
    master_volume: f32,
    expression: f32,
    pitch_bend_semitones: f64,
    /// Internal scratch buffer for mixing (one N-sample block).
    mix_buffer: [i32; N],
    /// Residue buffer for sub-block alignment in render_mono.
    /// Voices always advance by N samples per block; when the caller
    /// requests a non-N-aligned number of frames, we buffer the
    /// leftover samples here so no audio is lost.
    mono_residue: [f32; N],
    mono_residue_len: usize,
}

impl Synth {
    pub fn new(sample_rate: f64) -> Self {
        // Initialize all lookup tables (must be done once before any rendering)
        tables::init_tables(sample_rate);
        crate::lfo::init_lfo(sample_rate);
        crate::pitchenv::init_pitchenv(sample_rate);

        let mut voices = Vec::with_capacity(MAX_VOICES);
        for _ in 0..MAX_VOICES {
            voices.push(Voice::new());
        }

        Self {
            voices,
            current_patch: DxVoice::init_voice(),
            _sample_rate: sample_rate,
            sustain: false,
            sustained_notes: Vec::new(),
            master_volume: 0.5,
            expression: 1.0,
            pitch_bend_semitones: 0.0,
            mix_buffer: [0i32; N],
            mono_residue: [0.0; N],
            mono_residue_len: 0,
        }
    }

    /// Load a new patch.
    pub fn load_patch(&mut self, patch: DxVoice) {
        self.current_patch = patch;
    }

    /// Set master volume (0.0 to 1.0).
    pub fn set_master_volume(&mut self, vol: f32) {
        self.master_volume = vol;
    }

    /// Get the current patch name.
    pub fn patch_name(&self) -> &str {
        self.current_patch.name_str()
    }

    /// Trigger a note on.
    pub fn note_on(&mut self, note: u8, velocity: u8) {
        if velocity == 0 {
            self.note_off(note);
            return;
        }

        let voice_idx = self.allocate_voice();
        self.voices[voice_idx].note_on(&self.current_patch, note, velocity);
    }

    /// Trigger a note off.
    pub fn note_off(&mut self, note: u8) {
        if self.sustain {
            self.sustained_notes.push(note);
            return;
        }

        for voice in &mut self.voices {
            if voice.state == VoiceState::Active && voice.note == note {
                voice.note_off();
            }
        }
    }

    /// Process a raw MIDI message.
    pub fn process_midi(&mut self, msg: &[u8]) {
        if msg.is_empty() {
            return;
        }

        let status = msg[0] & 0xF0;
        match status {
            0x90 => {
                if msg.len() >= 3 {
                    self.note_on(msg[1], msg[2]);
                }
            }
            0x80 => {
                if msg.len() >= 3 {
                    self.note_off(msg[1]);
                }
            }
            0xB0 => {
                if msg.len() >= 3 {
                    self.control_change(msg[1], msg[2]);
                }
            }
            0xE0 => {
                if msg.len() >= 3 {
                    let bend = ((msg[2] as i16) << 7 | msg[1] as i16) - 8192;
                    self.pitch_bend(bend);
                }
            }
            _ => {}
        }
    }

    /// Process a synth command.
    pub fn process_command(&mut self, cmd: SynthCommand) {
        match cmd {
            SynthCommand::NoteOn { note, velocity } => self.note_on(note, velocity),
            SynthCommand::NoteOff { note } => self.note_off(note),
            SynthCommand::PitchBend { value } => self.pitch_bend(value),
            SynthCommand::ControlChange { cc, value } => self.control_change(cc, value),
            SynthCommand::LoadPatch(patch) => self.load_patch(*patch),
        }
    }

    pub fn control_change(&mut self, cc: u8, value: u8) {
        match cc {
            7 => {
                self.master_volume = value as f32 / 127.0;
            }
            11 => {
                self.expression = value as f32 / 127.0;
            }
            64 => {
                self.sustain = value >= 64;
                if !self.sustain {
                    let notes: Vec<u8> = self.sustained_notes.drain(..).collect();
                    for note in notes {
                        for voice in &mut self.voices {
                            if voice.state == VoiceState::Active && voice.note == note {
                                voice.note_off();
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub fn pitch_bend(&mut self, _value: i16) {
        self.pitch_bend_semitones = _value as f64 / 8192.0 * 2.0;
    }

    /// Render audio into the output buffer (interleaved stereo f32, -1.0..1.0).
    pub fn render(&mut self, output: &mut [f32]) {
        let channels = 2;
        let total_frames = output.len() / channels;
        let mut frame_offset = 0;

        while frame_offset < total_frames {
            let block_frames = (total_frames - frame_offset).min(N);

            // Clear mix buffer
            self.mix_buffer = [0i32; N];

            // Render all active voices into mix buffer
            for voice in &mut self.voices {
                if voice.state != VoiceState::Inactive {
                    let mut voice_buf = [0i32; N];
                    voice.render(&mut voice_buf);
                    for i in 0..block_frames {
                        self.mix_buffer[i] += voice_buf[i] >> 4;
                    }
                }
            }

            // Convert i32 mix to f32 stereo output
            // MkI output peaks at ~2^26 per carrier. The >>4 above brings it
            // to ~2^22, and the /2^24 below gives final float scaling.
            let volume = self.master_volume * self.expression;
            for i in 0..block_frames {
                let sample_i32 = self.mix_buffer[i];
                // Scale from Q24 to -1.0..1.0 range
                // With up to 6 carriers, max could be ~6 * (1<<24).
                // Using (1<<24) as unity gives reasonable headroom.
                let sample_f32 = (sample_i32 as f64 / (1i64 << 24) as f64) as f32 * volume;
                let clamped = sample_f32.clamp(-1.0, 1.0);
                let out_idx = (frame_offset + i) * channels;
                output[out_idx] = clamped;
                output[out_idx + 1] = clamped;
            }

            frame_offset += block_frames;
        }
    }

    /// Render mono f32 samples (for WAV rendering).
    /// Handles non-N-aligned buffer sizes by buffering residue samples
    /// so voices always advance in exact N-sample blocks.
    pub fn render_mono(&mut self, output: &mut [f32]) {
        let num_frames = output.len();
        let mut frame_offset = 0;

        // First, drain any residue from a previous partial block
        if self.mono_residue_len > 0 {
            let to_copy = self.mono_residue_len.min(num_frames);
            output[..to_copy].copy_from_slice(&self.mono_residue[..to_copy]);
            if to_copy < self.mono_residue_len {
                // Still more residue than output can hold — shift remainder
                self.mono_residue.copy_within(to_copy..self.mono_residue_len, 0);
                self.mono_residue_len -= to_copy;
                return;
            }
            self.mono_residue_len = 0;
            frame_offset = to_copy;
        }

        let volume = self.master_volume * self.expression;

        while frame_offset < num_frames {
            // Always render a full N-sample block
            self.mix_buffer = [0i32; N];

            for voice in &mut self.voices {
                if voice.state != VoiceState::Inactive {
                    let mut voice_buf = [0i32; N];
                    voice.render(&mut voice_buf);
                    for i in 0..N {
                        self.mix_buffer[i] += voice_buf[i] >> 4;
                    }
                }
            }

            // Convert full block to f32
            let mut block_f32 = [0.0f32; N];
            for i in 0..N {
                block_f32[i] = (self.mix_buffer[i] as f64 / (1i64 << 24) as f64) as f32 * volume;
            }

            let remaining = num_frames - frame_offset;
            if remaining >= N {
                // Full block fits
                output[frame_offset..frame_offset + N].copy_from_slice(&block_f32);
                frame_offset += N;
            } else {
                // Partial block — copy what fits, buffer the rest
                output[frame_offset..].copy_from_slice(&block_f32[..remaining]);
                let residue_count = N - remaining;
                self.mono_residue[..residue_count].copy_from_slice(&block_f32[remaining..]);
                self.mono_residue_len = residue_count;
                frame_offset = num_frames;
            }
        }
    }

    /// Allocate a voice slot using priority: inactive > oldest released > oldest active.
    fn allocate_voice(&mut self) -> usize {
        // 1. Find an inactive voice
        for (i, voice) in self.voices.iter().enumerate() {
            if voice.state == VoiceState::Inactive {
                return i;
            }
        }

        // 2. Find the oldest released voice
        let mut oldest_released: Option<(usize, u32)> = None;
        for (i, voice) in self.voices.iter().enumerate() {
            if voice.state == VoiceState::Released {
                if oldest_released.is_none() || voice.age > oldest_released.unwrap().1 {
                    oldest_released = Some((i, voice.age));
                }
            }
        }
        if let Some((idx, _)) = oldest_released {
            return idx;
        }

        // 3. Steal the oldest active voice
        let mut oldest_active: (usize, u32) = (0, 0);
        for (i, voice) in self.voices.iter().enumerate() {
            if voice.age > oldest_active.1 {
                oldest_active = (i, voice.age);
            }
        }
        oldest_active.0
    }
}
