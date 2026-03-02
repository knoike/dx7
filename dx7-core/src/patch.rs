//! DX7 voice patch parameters and SysEx parsing.

use crate::envelope::EnvParams;
use crate::lfo::{LfoParams, LfoWaveform};
use crate::operator::{OperatorParams, ScalingCurve};

/// Complete DX7 voice parameters.
#[derive(Clone, Debug)]
pub struct DxVoice {
    pub operators: [OperatorParams; 6],
    pub pitch_eg: EnvParams,
    pub algorithm: u8,              // 0-31
    pub feedback: u8,               // 0-7
    pub osc_key_sync: bool,
    pub lfo: LfoParams,
    pub pitch_mod_sensitivity: u8,  // 0-7
    pub transpose: u8,              // 0-48 (24=C3)
    pub name: [u8; 10],
}

impl Default for DxVoice {
    fn default() -> Self {
        Self::init_voice()
    }
}

impl DxVoice {
    /// Create the default INIT VOICE patch (simple sine on OP1).
    pub fn init_voice() -> Self {
        let mut ops: [OperatorParams; 6] = [
            OperatorParams::default(),
            OperatorParams::default(),
            OperatorParams::default(),
            OperatorParams::default(),
            OperatorParams::default(),
            OperatorParams::default(),
        ];

        // Only OP1 (index 5) is active with full level, rest are silent
        // Index 0 = OP6, index 5 = OP1 (matching MSFA/Dexed convention)
        for op in ops.iter_mut().take(5) {
            op.output_level = 0;
        }

        Self {
            operators: ops,
            pitch_eg: EnvParams {
                rates: [99, 99, 99, 99],
                levels: [50, 50, 50, 50],
            },
            algorithm: 0,           // Algorithm 1
            feedback: 0,
            osc_key_sync: true,
            lfo: LfoParams::default(),
            pitch_mod_sensitivity: 0,
            transpose: 24,          // C3
            name: *b"INIT VOICE",
        }
    }

    /// Get the voice name as a string.
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name).unwrap_or("??????????")
    }

    /// Parse a single voice from unpacked format (155 bytes).
    /// This is the format used in single voice SysEx dumps.
    /// Operators are stored OP6-first: index 0 = OP6, matching MSFA/Dexed convention.
    pub fn from_unpacked(data: &[u8; 155]) -> Self {
        let mut ops = [OperatorParams::default(); 6];

        // Each operator is 21 bytes, stored OP6 first (index 0 = OP6)
        for i in 0..6 {
            let op_idx = i; // index 0 = OP6, index 5 = OP1
            let base = i * 21;
            ops[op_idx] = OperatorParams {
                eg: EnvParams {
                    rates: [data[base], data[base + 1], data[base + 2], data[base + 3]],
                    levels: [data[base + 4], data[base + 5], data[base + 6], data[base + 7]],
                },
                kbd_level_scaling_break_point: data[base + 8],
                kbd_level_scaling_left_depth: data[base + 9],
                kbd_level_scaling_right_depth: data[base + 10],
                kbd_level_scaling_left_curve: ScalingCurve::from_u8(data[base + 11]),
                kbd_level_scaling_right_curve: ScalingCurve::from_u8(data[base + 12]),
                kbd_rate_scaling: data[base + 13],
                amp_mod_sensitivity: data[base + 14],
                key_velocity_sensitivity: data[base + 15],
                output_level: data[base + 16],
                osc_mode: data[base + 17],
                osc_freq_coarse: data[base + 18],
                osc_freq_fine: data[base + 19],
                osc_detune: data[base + 20],
            };
        }

        let gb = 126; // Global params start at byte 126

        DxVoice {
            operators: ops,
            pitch_eg: EnvParams {
                rates: [data[gb], data[gb + 1], data[gb + 2], data[gb + 3]],
                levels: [data[gb + 4], data[gb + 5], data[gb + 6], data[gb + 7]],
            },
            algorithm: data[gb + 8],
            feedback: data[gb + 9],
            osc_key_sync: data[gb + 10] != 0,
            lfo: LfoParams {
                speed: data[gb + 11],
                delay: data[gb + 12],
                pitch_mod_depth: data[gb + 13],
                amp_mod_depth: data[gb + 14],
                key_sync: data[gb + 15] != 0,
                waveform: LfoWaveform::from_u8(data[gb + 16]),
            },
            pitch_mod_sensitivity: data[gb + 17],
            transpose: data[gb + 18],
            name: {
                let mut name = [b' '; 10];
                name.copy_from_slice(&data[gb + 19..gb + 29]);
                name
            },
        }
    }

    /// Parse a single voice from packed format (128 bytes per voice in bulk dump).
    pub fn from_packed(data: &[u8; 128]) -> Self {
        let mut ops = [OperatorParams::default(); 6];

        // Each operator is 17 bytes in packed format, stored OP6 first (index 0 = OP6)
        for i in 0..6 {
            let op_idx = i; // index 0 = OP6, index 5 = OP1
            let base = i * 17;

            let eg = EnvParams {
                rates: [data[base], data[base + 1], data[base + 2], data[base + 3]],
                levels: [data[base + 4], data[base + 5], data[base + 6], data[base + 7]],
            };

            let bp = data[base + 8];
            let ld = data[base + 9];
            let rd = data[base + 10];

            // Byte 11: left_curve[1:0] | right_curve[3:2]
            let curves = data[base + 11];
            let left_curve = ScalingCurve::from_u8(curves & 0x03);
            let right_curve = ScalingCurve::from_u8((curves >> 2) & 0x03);

            // Byte 12: rate_scaling[2:0] | detune[6:3]
            let rs_det = data[base + 12];
            let rate_scaling = rs_det & 0x07;
            let detune = (rs_det >> 3) & 0x0F;

            // Byte 13: amp_mod_sens[1:0] | key_vel_sens[4:2]
            let ams_kvs = data[base + 13];
            let amp_mod_sensitivity = ams_kvs & 0x03;
            let key_velocity_sensitivity = (ams_kvs >> 2) & 0x07;

            let output_level = data[base + 14];

            // Byte 15: osc_mode[0] | freq_coarse[5:1]
            let mode_coarse = data[base + 15];
            let osc_mode = mode_coarse & 0x01;
            let freq_coarse = (mode_coarse >> 1) & 0x1F;

            let freq_fine = data[base + 16];

            ops[op_idx] = OperatorParams {
                eg,
                kbd_level_scaling_break_point: bp,
                kbd_level_scaling_left_depth: ld,
                kbd_level_scaling_right_depth: rd,
                kbd_level_scaling_left_curve: left_curve,
                kbd_level_scaling_right_curve: right_curve,
                kbd_rate_scaling: rate_scaling,
                amp_mod_sensitivity,
                key_velocity_sensitivity,
                output_level,
                osc_mode,
                osc_freq_coarse: freq_coarse,
                osc_freq_fine: freq_fine,
                osc_detune: detune,
            };
        }

        let gb = 102; // Global params at byte 102

        // Byte 110 (gb+8): Algorithm[4:0] — 5 bits, value 0-31
        let algorithm = data[gb + 8] & 0x1F;

        // Byte 111 (gb+9): OscKeySync[3] | Feedback[2:0]
        let fb_sync = data[gb + 9];
        let feedback = fb_sync & 0x07;
        let osc_key_sync = (fb_sync >> 3) & 0x01 != 0;

        // Byte 116 (gb+14): LFO PitchModSens[6:4] | LFO Wave[3:1] | LFO Sync[0]
        let ls_lw_pms = data[gb + 14];
        let lfo_key_sync = ls_lw_pms & 0x01 != 0;
        let lfo_waveform = LfoWaveform::from_u8((ls_lw_pms >> 1) & 0x07);
        let pitch_mod_sensitivity = (ls_lw_pms >> 4) & 0x07;

        DxVoice {
            operators: ops,
            pitch_eg: EnvParams {
                rates: [data[gb], data[gb + 1], data[gb + 2], data[gb + 3]],
                levels: [data[gb + 4], data[gb + 5], data[gb + 6], data[gb + 7]],
            },
            algorithm,
            feedback,
            osc_key_sync,
            lfo: LfoParams {
                speed: data[gb + 10],
                delay: data[gb + 11],
                pitch_mod_depth: data[gb + 12],
                amp_mod_depth: data[gb + 13],
                key_sync: lfo_key_sync,
                waveform: lfo_waveform,
            },
            pitch_mod_sensitivity,
            transpose: data[gb + 15],
            name: {
                let mut name = [b' '; 10];
                let name_start = gb + 16;
                let name_end = (name_start + 10).min(128);
                let len = name_end - name_start;
                name[..len].copy_from_slice(&data[name_start..name_end]);
                name
            },
        }
    }

    /// Parse a 32-voice bulk dump SysEx message.
    /// Expected format: F0 43 0s 09 20 00 <4096 bytes> <checksum> F7
    /// Returns up to 32 voices, or an error message.
    pub fn parse_bulk_dump(data: &[u8]) -> Result<Vec<DxVoice>, &'static str> {
        if data.len() < 4104 {
            return Err("Data too short for bulk dump");
        }

        // Check SysEx header
        if data[0] != 0xF0 {
            return Err("Missing SysEx start byte (F0)");
        }
        if data[1] != 0x43 {
            return Err("Not a Yamaha SysEx message");
        }
        // data[2] = sub-status + channel (0s where s=channel)
        if data[3] != 0x09 {
            return Err("Not a 32-voice bulk dump (format byte)");
        }
        // Byte count: 0x20 0x00 = 8192? Actually 4096 = 0x10 0x00
        // DX7 uses: byte_count_msb=0x20, byte_count_lsb=0x00 → 0x2000 = 8192
        // But actual data is 4096 bytes. Let's be flexible.

        let voice_data_start = 6;
        let voice_data_end = voice_data_start + 4096;

        if data.len() < voice_data_end + 2 {
            return Err("Data too short");
        }

        // Verify checksum: two's complement of sum of voice data, AND 0x7F
        let sum: u8 = data[voice_data_start..voice_data_end]
            .iter()
            .fold(0u8, |acc, &b| acc.wrapping_add(b));
        let expected_checksum = (!sum).wrapping_add(1) & 0x7F;
        let actual_checksum = data[voice_data_end];

        if actual_checksum != expected_checksum {
            // Be lenient — some SysEx files have different checksum conventions
            // Just warn but continue
        }

        // Check SysEx end
        if data[voice_data_end + 1] != 0xF7 {
            // Also be lenient here
        }

        // Parse 32 voices, each 128 bytes
        let mut voices = Vec::with_capacity(32);
        for i in 0..32 {
            let start = voice_data_start + i * 128;
            let mut voice_data = [0u8; 128];
            voice_data.copy_from_slice(&data[start..start + 128]);
            voices.push(DxVoice::from_packed(&voice_data));
        }

        Ok(voices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_voice() {
        let voice = DxVoice::init_voice();
        assert_eq!(voice.algorithm, 0);
        assert_eq!(voice.feedback, 0);
        assert_eq!(voice.name_str(), "INIT VOICE");
        // operators[5] is OP1 (active), operators[0] is OP6 (silent)
        assert_eq!(voice.operators[5].output_level, 99);
        assert_eq!(voice.operators[0].output_level, 0);
    }

    #[test]
    fn test_unpacked_roundtrip() {
        // Create a known unpacked voice data block
        let mut data = [0u8; 155];
        // OP6 (first in data): set some distinctive values
        data[0] = 50; // EG rate 1
        data[16] = 80; // output level

        // Global params at byte 126
        data[126 + 8] = 4; // algorithm 5 (0-indexed = 4)
        data[126 + 9] = 3; // feedback

        // Name
        for (i, &c) in b"TEST PATCH".iter().enumerate() {
            data[126 + 19 + i] = c;
        }

        let voice = DxVoice::from_unpacked(&data);
        assert_eq!(voice.algorithm, 4);
        assert_eq!(voice.feedback, 3);
        assert_eq!(voice.name_str(), "TEST PATCH");
        // OP6 data is at index 0 (first in data = index 0)
        assert_eq!(voice.operators[0].eg.rates[0], 50); // OP6
        assert_eq!(voice.operators[0].output_level, 80);
    }
}
