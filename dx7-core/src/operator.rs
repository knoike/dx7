//! FM operator kernel and DX7 scaling functions.
//!
//! Ported from Dexed/MSFA fm_op_kernel.cc and dx7note.cc
//! (Apache 2.0, Google Inc. / Pascal Gauthier).

use crate::tables::{self, N, LG_N};

// --- Operator parameter types ---

/// Keyboard level scaling curve types.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScalingCurve {
    NegLin,
    NegExp,
    PosExp,
    PosLin,
}

impl ScalingCurve {
    pub fn from_u8(v: u8) -> Self {
        match v & 0x03 {
            0 => ScalingCurve::NegLin,
            1 => ScalingCurve::NegExp,
            2 => ScalingCurve::PosExp,
            3 => ScalingCurve::PosLin,
            _ => unreachable!(),
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            ScalingCurve::NegLin => 0,
            ScalingCurve::NegExp => 1,
            ScalingCurve::PosExp => 2,
            ScalingCurve::PosLin => 3,
        }
    }
}

/// All per-operator DX7 parameters (21 params per operator in SysEx).
#[derive(Clone, Copy, Debug)]
pub struct OperatorParams {
    pub eg: crate::envelope::EnvParams,
    pub kbd_level_scaling_break_point: u8,
    pub kbd_level_scaling_left_depth: u8,
    pub kbd_level_scaling_right_depth: u8,
    pub kbd_level_scaling_left_curve: ScalingCurve,
    pub kbd_level_scaling_right_curve: ScalingCurve,
    pub kbd_rate_scaling: u8,
    pub amp_mod_sensitivity: u8,
    pub key_velocity_sensitivity: u8,
    pub output_level: u8,
    pub osc_mode: u8,
    pub osc_freq_coarse: u8,
    pub osc_freq_fine: u8,
    pub osc_detune: u8,
}

impl Default for OperatorParams {
    fn default() -> Self {
        Self {
            eg: crate::envelope::EnvParams {
                rates: [99, 99, 99, 99],
                levels: [99, 99, 99, 0],
            },
            kbd_level_scaling_break_point: 39,
            kbd_level_scaling_left_depth: 0,
            kbd_level_scaling_right_depth: 0,
            kbd_level_scaling_left_curve: ScalingCurve::NegLin,
            kbd_level_scaling_right_curve: ScalingCurve::NegLin,
            kbd_rate_scaling: 0,
            amp_mod_sensitivity: 0,
            key_velocity_sensitivity: 0,
            output_level: 99,
            osc_mode: 0,
            osc_freq_coarse: 1,
            osc_freq_fine: 0,
            osc_detune: 7,
        }
    }
}

/// Per-voice runtime state for one operator.
pub struct FmOpParams {
    /// Envelope level (log domain input for gain computation).
    pub level_in: i32,
    /// Previous block's gain: MkI log attenuation (0 = loud, ENV_MAX = silent).
    pub gain_out: u16,
    /// Phase increment per sample.
    pub freq: i32,
    /// Current phase accumulator.
    pub phase: i32,
}

impl FmOpParams {
    pub fn new() -> Self {
        Self {
            level_in: 0,
            gain_out: 0,
            freq: 0,
            phase: 0,
        }
    }
}

// --- MkI FM operator kernel functions (ported from EngineMkI.cpp) ---

/// MkI log-domain sine: combines phase and envelope in log domain,
/// converts back to linear via exp table. Output range ~[-2^26, +2^26].
#[inline]
pub fn mki_sin(phase: i32, env: u16) -> i32 {
    let exp_val = tables::sin_log((phase >> 12) as u16).wrapping_add(env);
    let is_signed = (exp_val & 0x8000) != 0;
    let exp_val = exp_val & 0x7FFF;
    let result = 4096u32 + tables::sin_exp((exp_val & 0x3FF) ^ 0x3FF) as u32;
    let result = result >> (exp_val >> 10);
    if is_signed {
        (-(result as i32) - 1) << 13
    } else {
        (result as i32) << 13
    }
}

/// FM operator with modulation input (no feedback). MkI version.
pub fn compute(
    output: &mut [i32; N],
    input: &[i32; N],
    phase0: i32,
    freq: i32,
    gain1: u16,
    gain2: u16,
    add: bool,
) {
    let dgain = (gain2 as i32 - gain1 as i32 + (N as i32 >> 1)) >> LG_N;
    let mut gain = gain1 as i32;
    let mut phase = phase0;
    if add {
        for i in 0..N {
            gain += dgain;
            let y = mki_sin(phase.wrapping_add(input[i]), gain as u16);
            output[i] += y;
            phase = phase.wrapping_add(freq);
        }
    } else {
        for i in 0..N {
            gain += dgain;
            let y = mki_sin(phase.wrapping_add(input[i]), gain as u16);
            output[i] = y;
            phase = phase.wrapping_add(freq);
        }
    }
}

/// Pure sine generator, no modulation input. MkI version.
pub fn compute_pure(
    output: &mut [i32; N],
    phase0: i32,
    freq: i32,
    gain1: u16,
    gain2: u16,
    add: bool,
) {
    let dgain = (gain2 as i32 - gain1 as i32 + (N as i32 >> 1)) >> LG_N;
    let mut gain = gain1 as i32;
    let mut phase = phase0;
    if add {
        for i in 0..N {
            gain += dgain;
            let y = mki_sin(phase, gain as u16);
            output[i] += y;
            phase = phase.wrapping_add(freq);
        }
    } else {
        for i in 0..N {
            gain += dgain;
            let y = mki_sin(phase, gain as u16);
            output[i] = y;
            phase = phase.wrapping_add(freq);
        }
    }
}

/// Self-feedback operator. MkI version.
pub fn compute_fb(
    output: &mut [i32; N],
    phase0: i32,
    freq: i32,
    gain1: u16,
    gain2: u16,
    fb_buf: &mut [i32; 2],
    fb_shift: i32,
    add: bool,
) {
    let dgain = (gain2 as i32 - gain1 as i32 + (N as i32 >> 1)) >> LG_N;
    let mut gain = gain1 as i32;
    let mut phase = phase0;
    let mut y0 = fb_buf[0];
    let mut y = fb_buf[1];
    if add {
        for i in 0..N {
            gain += dgain;
            let scaled_fb = (y0 + y) >> (fb_shift + 1);
            y0 = y;
            y = mki_sin(phase.wrapping_add(scaled_fb), gain as u16);
            output[i] += y;
            phase = phase.wrapping_add(freq);
        }
    } else {
        for i in 0..N {
            gain += dgain;
            let scaled_fb = (y0 + y) >> (fb_shift + 1);
            y0 = y;
            y = mki_sin(phase.wrapping_add(scaled_fb), gain as u16);
            output[i] = y;
            phase = phase.wrapping_add(freq);
        }
    }
    fb_buf[0] = y0;
    fb_buf[1] = y;
}

/// Fused 2-operator feedback chain for algorithm 6.
/// Op 0 feeds back to itself, its output modulates op 1, op 1 is the carrier.
pub fn compute_fb2(
    output: &mut [i32; N],
    phase0_0: i32, freq0: i32, gain1_0: u16, gain2_0: u16,
    phase0_1: i32, freq1: i32, gain1_1: u16, gain2_1: u16,
    fb_buf: &mut [i32; 2],
    fb_shift: i32,
) {
    let dgain0 = (gain2_0 as i32 - gain1_0 as i32 + (N as i32 >> 1)) >> LG_N;
    let dgain1 = (gain2_1 as i32 - gain1_1 as i32 + (N as i32 >> 1)) >> LG_N;
    let mut gain0 = gain1_0 as i32;
    let mut gain1 = gain1_1 as i32;
    let mut phase0 = phase0_0;
    let mut phase1 = phase0_1;
    let mut y0 = fb_buf[0];
    let mut y = fb_buf[1];

    for i in 0..N {
        let scaled_fb = (y0 + y) >> (fb_shift + 1);
        // op 0: feedback operator
        gain0 += dgain0;
        y0 = y;
        y = mki_sin(phase0.wrapping_add(scaled_fb), gain0 as u16);
        phase0 = phase0.wrapping_add(freq0);
        // op 1: modulated by op 0
        gain1 += dgain1;
        y = mki_sin(phase1.wrapping_add(y), gain1 as u16);
        phase1 = phase1.wrapping_add(freq1);

        output[i] = y;
    }
    fb_buf[0] = y0;
    fb_buf[1] = y;
}

/// Fused 3-operator feedback chain for algorithm 4.
/// Op 0 feeds back, its output → op 1 → op 2 (carrier).
pub fn compute_fb3(
    output: &mut [i32; N],
    phase0_0: i32, freq0: i32, gain1_0: u16, gain2_0: u16,
    phase0_1: i32, freq1: i32, gain1_1: u16, gain2_1: u16,
    phase0_2: i32, freq2: i32, gain1_2: u16, gain2_2: u16,
    fb_buf: &mut [i32; 2],
    fb_shift: i32,
) {
    let dgain0 = (gain2_0 as i32 - gain1_0 as i32 + (N as i32 >> 1)) >> LG_N;
    let dgain1 = (gain2_1 as i32 - gain1_1 as i32 + (N as i32 >> 1)) >> LG_N;
    let dgain2 = (gain2_2 as i32 - gain1_2 as i32 + (N as i32 >> 1)) >> LG_N;
    let mut g0 = gain1_0 as i32;
    let mut g1 = gain1_1 as i32;
    let mut g2 = gain1_2 as i32;
    let mut phase0 = phase0_0;
    let mut phase1 = phase0_1;
    let mut phase2 = phase0_2;
    let mut y0 = fb_buf[0];
    let mut y = fb_buf[1];

    for i in 0..N {
        let scaled_fb = (y0 + y) >> (fb_shift + 1);
        // op 0: feedback operator
        g0 += dgain0;
        y0 = y;
        y = mki_sin(phase0.wrapping_add(scaled_fb), g0 as u16);
        phase0 = phase0.wrapping_add(freq0);
        // op 1: modulated by op 0
        g1 += dgain1;
        y = mki_sin(phase1.wrapping_add(y), g1 as u16);
        phase1 = phase1.wrapping_add(freq1);
        // op 2: modulated by op 1
        g2 += dgain2;
        y = mki_sin(phase2.wrapping_add(y), g2 as u16);
        phase2 = phase2.wrapping_add(freq2);

        output[i] = y;
    }
    fb_buf[0] = y0;
    fb_buf[1] = y;
}

// --- DX7 scaling functions (ported from dx7note.cc) ---

/// Coarse frequency multiplier table (log2 domain, Q24).
/// Index 0 = 0.5x (-1 octave), index 1 = 1x, index 2 = 2x, etc.
pub const COARSEMUL: [i32; 32] = [
    -16777216, 0, 16777216, 26591258, 33554432, 38955489, 43368474, 47099600,
    50331648, 53182516, 55732705, 58039632, 60145690, 62083076, 63876816,
    65546747, 67108864, 68576247, 69959732, 71268397, 72509921, 73690858,
    74816848, 75892776, 76922906, 77910978, 78860292, 79773775, 80654032,
    81503396, 82323963, 83117622,
];

/// Velocity sensitivity table (64 entries).
pub const VELOCITY_DATA: [u8; 64] = [
    0, 70, 86, 97, 106, 114, 121, 126, 132, 138, 142, 148, 152, 156, 160, 163,
    166, 170, 173, 174, 178, 181, 184, 186, 189, 190, 194, 196, 198, 200, 202,
    205, 206, 209, 211, 214, 216, 218, 220, 222, 224, 225, 227, 229, 230, 232,
    233, 235, 237, 238, 240, 241, 242, 243, 244, 246, 246, 248, 249, 250, 251,
    252, 253, 254,
];

/// Exponential curve scale data (33 entries).
pub const EXP_SCALE_DATA: [u8; 33] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 14, 16, 19, 23, 27, 33, 39, 47, 56, 66,
    80, 94, 110, 126, 142, 158, 174, 190, 206, 222, 238, 250,
];

/// Pitch modulation sensitivity table (8 entries).
pub const PITCHMODSENSTAB: [u8; 8] = [0, 10, 20, 33, 55, 92, 153, 255];

/// Amplitude modulation sensitivity table (4 entries, Q24 scaled).
pub const AMPMODSENSTAB: [u32; 4] = [0, 4342338, 7171437, 16777216];

/// Scale velocity to microstep delta.
pub fn scale_velocity(velocity: i32, sensitivity: i32) -> i32 {
    let clamped_vel = velocity.clamp(0, 127);
    let vel_value = VELOCITY_DATA[(clamped_vel >> 1) as usize] as i32 - 239;
    let scaled_vel = ((sensitivity * vel_value + 7) >> 3) << 4;
    scaled_vel
}

/// Scale envelope rate by keyboard position.
pub fn scale_rate(midinote: i32, sensitivity: i32) -> i32 {
    let x = (midinote / 3 - 7).clamp(0, 31);
    (sensitivity * x) >> 3
}

/// Scale level using curve type.
pub fn scale_curve(group: i32, depth: i32, curve: i32) -> i32 {
    let scale = if curve == 0 || curve == 3 {
        // Linear
        (group * depth * 329) >> 12
    } else {
        // Exponential
        let raw_exp = EXP_SCALE_DATA[group.min(32) as usize] as i32;
        (raw_exp * depth * 329) >> 15
    };
    if curve < 2 { -scale } else { scale }
}

/// Compute keyboard level scaling for an operator.
pub fn scale_level(
    midinote: i32,
    break_pt: i32,
    left_depth: i32,
    right_depth: i32,
    left_curve: i32,
    right_curve: i32,
) -> i32 {
    let offset = midinote - break_pt - 17;
    if offset >= 0 {
        scale_curve((offset + 1) / 3, right_depth, right_curve)
    } else {
        scale_curve(-(offset - 1) / 3, left_depth, left_curve)
    }
}

/// Compute operator log-frequency from DX7 patch parameters.
/// Returns Q24 logfreq (log2(freq) * (1<<24)).
pub fn osc_freq(midinote: i32, mode: i32, coarse: i32, fine: i32, detune: i32) -> i32 {
    if mode == 0 {
        // Ratio mode
        let mut logfreq = tables::midinote_to_logfreq(midinote);

        // Detune (empirically measured from DX7 hardware, matches Dexed dx7note.cc)
        let logfreq_frac = logfreq as f64 / (1i64 << 24) as f64;
        logfreq += ((detune - 7) as f64 * 0.0209 * (-0.396 * logfreq_frac).exp() * (1i64 << 24) as f64) as i32;

        logfreq += COARSEMUL[(coarse & 31) as usize];
        if fine != 0 {
            // (1 << 24) / log(2) = 24204406.323123
            logfreq += (24204406.323123 * (1.0 + 0.01 * fine as f64).ln() + 0.5).floor() as i32;
        }
        logfreq
    } else {
        // Fixed frequency mode
        // ((1 << 24) * log(10) / log(2) * 0.01) << 3 = 4458616
        let mut logfreq = (4458616i64 * ((coarse & 3) * 100 + fine) as i64 >> 3) as i32;
        if detune > 7 {
            logfreq += 13457 * (detune - 7);
        }
        logfreq
    }
}

/// Feedback bit depth constant.
pub const FEEDBACK_BITDEPTH: i32 = 8;
