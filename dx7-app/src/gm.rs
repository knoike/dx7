//! General MIDI sound set — 128 programs compiled from DX7 patches.
//!
//! Patch data is compiled into the binary (see `gm_rom.rs`).
//! To regenerate: `cargo run --example gen_gm_rom > dx7-app/src/gm_rom.rs`

use dx7_core::DxVoice;

/// Per-program gain compensation.
///
/// DX7 patches have wildly different output levels — bass patches produce ~4x
/// less output than piano/brass.  A real GM sound module normalises levels;
/// these multipliers do the same for our DX7-based GM set.
pub fn program_gain(program: u8) -> f32 {
    match program {
        // Bass — custom FM bass patch, boosted for presence
        32..=39 => 4.0,
        // Solo strings (violin, viola, cello, contrabass)
        40..=43 => 1.5,
        // Tremolo/pizzicato strings, harp, timpani
        44..=47 => 1.3,
        // String ensemble, synth strings
        48..=51 => 1.2,
        // Reed instruments (sax, oboe, bassoon, clarinet)
        64..=71 => 1.3,
        // Pipe instruments (flute, recorder, pan flute)
        72..=79 => 1.3,
        _ => 1.0,
    }
}

/// Returns true if this GM program is a bass instrument (32-39).
pub fn is_bass_program(program: u8) -> bool {
    (32..=39).contains(&program)
}

/// Preloaded GM sound set — 128 DxVoice patches from compiled ROM.
pub struct GmSoundSet {
    patches: [DxVoice; 128],
}

impl GmSoundSet {
    /// Load all 128 GM patches from the compiled-in ROM data.
    /// No sysex files needed at runtime.
    pub fn load(_sysex_dir: &str) -> Self {
        let patches: [DxVoice; 128] = std::array::from_fn(|i| {
            crate::gm_rom::gm_voice(i as u8)
        });
        Self { patches }
    }

    /// Get the DX7 patch for a GM program number (0–127).
    pub fn get(&self, program: u8) -> Option<&DxVoice> {
        Some(&self.patches[program as usize])
    }
}
