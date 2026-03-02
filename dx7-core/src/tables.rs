//! Sine, Exp2, and Frequency lookup tables for FM synthesis.
//!
//! Ported from Dexed/MSFA (Apache 2.0, Google Inc. / Pascal Gauthier).
//! Tables must be initialized by calling `init_tables(sample_rate)` before use.

use core::f64::consts::PI;

/// Block size exponent: N = 2^LG_N = 64 samples per block.
pub const LG_N: i32 = 6;
/// Block size for sub-sampled processing.
pub const N: usize = 1 << LG_N as usize;

// --- Sine table ---
const SIN_LG_N_SAMPLES: i32 = 10;
const SIN_N_SAMPLES: usize = 1 << SIN_LG_N_SAMPLES as usize;

// --- Exp2 table ---
const EXP2_LG_N_SAMPLES: i32 = 10;
const EXP2_N_SAMPLES: usize = 1 << EXP2_LG_N_SAMPLES as usize;

// --- Frequency LUT ---
const FREQ_LG_N_SAMPLES: i32 = 10;
const FREQ_N_SAMPLES: usize = 1 << FREQ_LG_N_SAMPLES as usize;
const FREQ_SAMPLE_SHIFT: i32 = 24 - FREQ_LG_N_SAMPLES;
const FREQ_MAX_LOGFREQ_INT: i32 = 20;

// Delta-encoded tables (pairs of [delta, value] for linear interpolation).
static mut SINTAB: [i32; SIN_N_SAMPLES << 1] = [0; SIN_N_SAMPLES << 1];
static mut EXP2TAB: [i32; EXP2_N_SAMPLES << 1] = [0; EXP2_N_SAMPLES << 1];
static mut FREQLUT: [i32; FREQ_N_SAMPLES + 1] = [0; FREQ_N_SAMPLES + 1];

// --- MkI (Mark I) engine tables ---
// OPL-style log-domain sine/exp lookup for DX7-accurate FM synthesis.
pub const ENV_BITDEPTH: u16 = 14;
pub const ENV_MAX: u16 = 1 << ENV_BITDEPTH; // 16384

const SINLOG_TABLESIZE: usize = 1024;
const SINEXP_TABLESIZE: usize = 1024;
const NEGATIVE_BIT: u16 = 0x8000;

static mut SINLOG_TABLE: [u16; SINLOG_TABLESIZE] = [0; SINLOG_TABLESIZE];
static mut SINEXP_TABLE: [u16; SINEXP_TABLESIZE] = [0; SINEXP_TABLESIZE];

/// Sample rate multiplier for envelope/LFO rate compensation (Q24).
/// sr_multiplier = (44100 / sample_rate) * (1 << 24)
static mut SR_MULTIPLIER: u32 = 1 << 24;

/// Initialize all lookup tables. Must be called once at startup before any
/// audio rendering. Not thread-safe — call from a single thread.
pub fn init_tables(sample_rate: f64) {
    unsafe {
        init_sin_table();
        init_exp2_table();
        init_freq_table(sample_rate);
        init_mki_tables();
        SR_MULTIPLIER = ((44100.0 / sample_rate) * ((1u64 << 24) as f64)) as u32;
    }
}

/// Get the sample rate multiplier (Q24). Used by envelope and LFO for
/// rate compensation relative to 44100 Hz.
#[inline]
pub fn sr_multiplier() -> u32 {
    unsafe { SR_MULTIPLIER }
}

// --- Table initialization (ported from sin.cc, exp2.cc, freqlut.cc) ---

unsafe fn init_sin_table() {
    let dphase = 2.0 * PI / SIN_N_SAMPLES as f64;
    let c = (dphase.cos() * (1i64 << 30) as f64 + 0.5).floor() as i32;
    let s = (dphase.sin() * (1i64 << 30) as f64 + 0.5).floor() as i32;
    let r: i64 = 1 << 29;
    let mut u: i32 = 1 << 30;
    let mut v: i32 = 0;

    for i in 0..(SIN_N_SAMPLES / 2) {
        SINTAB[(i << 1) + 1] = (v + 32) >> 6;
        SINTAB[((i + SIN_N_SAMPLES / 2) << 1) + 1] = -((v + 32) >> 6);
        let t = ((u as i64 * s as i64 + v as i64 * c as i64 + r) >> 30) as i32;
        u = ((u as i64 * c as i64 - v as i64 * s as i64 + r) >> 30) as i32;
        v = t;
    }

    for i in 0..(SIN_N_SAMPLES - 1) {
        SINTAB[i << 1] = SINTAB[(i << 1) + 3] - SINTAB[(i << 1) + 1];
    }
    SINTAB[(SIN_N_SAMPLES << 1) - 2] = -SINTAB[(SIN_N_SAMPLES << 1) - 1];
}

unsafe fn init_exp2_table() {
    let inc = (1.0f64 / EXP2_N_SAMPLES as f64).exp2();
    let mut y: f64 = (1u64 << 30) as f64;

    for i in 0..EXP2_N_SAMPLES {
        EXP2TAB[(i << 1) + 1] = (y + 0.5).floor() as i32;
        y *= inc;
    }

    for i in 0..(EXP2_N_SAMPLES - 1) {
        EXP2TAB[i << 1] = EXP2TAB[(i << 1) + 3] - EXP2TAB[(i << 1) + 1];
    }
    // Last delta wraps to 2^31 (use wrapping to match C++ unsigned arithmetic)
    EXP2TAB[(EXP2_N_SAMPLES << 1) - 2] =
        ((1u32 << 31).wrapping_sub(EXP2TAB[(EXP2_N_SAMPLES << 1) - 1] as u32)) as i32;
}

unsafe fn init_freq_table(sample_rate: f64) {
    let mut y: f64 = ((1i64 << (24 + FREQ_MAX_LOGFREQ_INT)) as f64) / sample_rate;
    let inc = (1.0f64 / FREQ_N_SAMPLES as f64).exp2();

    for i in 0..=FREQ_N_SAMPLES {
        FREQLUT[i] = (y + 0.5).floor() as i32;
        y *= inc;
    }
}

unsafe fn init_mki_tables() {
    for i in 0..SINLOG_TABLESIZE {
        let x = ((0.5 + i as f64) / SINLOG_TABLESIZE as f64 * PI / 2.0).sin();
        SINLOG_TABLE[i] = (-1024.0 * x.log2()).round() as u16;
    }
    for i in 0..SINEXP_TABLESIZE {
        let x = ((i as f64 / SINEXP_TABLESIZE as f64).exp2() - 1.0) * 4096.0;
        SINEXP_TABLE[i] = x.round() as u16;
    }
}

// --- Lookup functions (hot path, ported from sin.h, exp2.h, freqlut.cc) ---

/// Sine lookup with linear interpolation. Q24 phase in, Q24 amplitude out.
/// Phase wraps naturally at 24 bits (0..2^24 = one full cycle).
#[inline]
pub fn sin_lookup(phase: i32) -> i32 {
    const SHIFT: i32 = 24 - SIN_LG_N_SAMPLES; // 14
    let lowbits = phase & ((1 << SHIFT) - 1);
    let phase_int =
        ((phase >> (SHIFT - 1)) & (((SIN_N_SAMPLES as i32) - 1) << 1)) as usize;

    unsafe {
        let dy = SINTAB[phase_int];
        let y0 = SINTAB[phase_int + 1];
        y0 + (((dy as i64) * (lowbits as i64)) >> SHIFT) as i32
    }
}

/// Exp2 lookup: Q24 log input → Q24 linear output.
/// Computes 2^(x / 2^24) scaled to Q24.
#[inline]
pub fn exp2_lookup(x: i32) -> i32 {
    const SHIFT: i32 = 24 - EXP2_LG_N_SAMPLES; // 14
    let lowbits = x & ((1 << SHIFT) - 1);
    let x_int =
        ((x >> (SHIFT - 1)) & (((EXP2_N_SAMPLES as i32) - 1) << 1)) as usize;

    unsafe {
        let dy = EXP2TAB[x_int];
        let y0 = EXP2TAB[x_int + 1];
        let y = y0 + (((dy as i64) * (lowbits as i64)) >> SHIFT) as i32;
        let shift = 6 - (x >> 24);
        if shift < 0 {
            // Would shift left — clamp to max (very loud, shouldn't happen)
            y << (-shift).min(31)
        } else if shift >= 32 {
            // Very quiet — effectively zero
            0
        } else {
            y >> shift
        }
    }
}

/// Frequency lookup: Q24 logfreq → phase increment per sample.
/// Logfreq is log2(freq) in Q24 format (1.0 in Q24 = one octave).
#[inline]
pub fn freqlut_lookup(logfreq: i32) -> i32 {
    let ix = ((logfreq & 0xffffff) >> FREQ_SAMPLE_SHIFT) as usize;

    unsafe {
        let y0 = FREQLUT[ix];
        let y1 = FREQLUT[ix + 1];
        let lowbits = logfreq & ((1 << FREQ_SAMPLE_SHIFT) - 1);
        let y = y0 + ((((y1 - y0) as i64) * (lowbits as i64)) >> FREQ_SAMPLE_SHIFT) as i32;
        let hibits = logfreq >> 24;
        y >> (FREQ_MAX_LOGFREQ_INT - hibits)
    }
}

// --- MkI lookup functions (ported from EngineMkI.cpp) ---

/// Log-sine lookup with quadrant handling. Input `phi` uses lower 12 bits:
/// bits 9..0 = table index, bits 11..10 = quadrant. Returns log attenuation
/// with bit 15 as sign flag.
#[inline]
pub fn sin_log(phi: u16) -> u16 {
    let index = (phi & 0x3FF) as usize;
    unsafe {
        match (phi >> 10) & 3 {
            0 => SINLOG_TABLE[index],
            1 => SINLOG_TABLE[index ^ 0x3FF],
            2 => SINLOG_TABLE[index] | NEGATIVE_BIT,
            _ => SINLOG_TABLE[index ^ 0x3FF] | NEGATIVE_BIT,
        }
    }
}

/// Exp table lookup for MkI. Returns mantissa value (0..4095).
#[inline]
pub fn sin_exp(index: u16) -> u16 {
    unsafe { SINEXP_TABLE[(index & 0x3FF) as usize] }
}

/// Convert MIDI note number to Q24 logfreq (standard 12-TET, A4=440Hz).
/// logfreq = log2(freq) * (1 << 24).
pub fn midinote_to_logfreq(note: i32) -> i32 {
    // log2(440) * (1<<24) computed at f64 precision
    let log2_440: f64 = 440.0f64.log2(); // 8.78135971...
    let logfreq = (log2_440 + (note as f64 - 69.0) / 12.0) * ((1i64 << 24) as f64);
    (logfreq + 0.5).floor() as i32
}

// --- Legacy compat (kept during migration) ---

/// 14-bit amplitude range: +/- 8191 (used by old sine table).
pub const AMP_14BIT: i32 = 8191;

#[cfg(test)]
mod tests {
    use super::*;

    fn ensure_init() {
        use std::sync::Once;
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            init_tables(44100.0);
            crate::lfo::init_lfo(44100.0);
            crate::pitchenv::init_pitchenv(44100.0);
        });
    }

    #[test]
    fn test_sin_lookup_zero() {
        ensure_init();
        let val = sin_lookup(0);
        assert!(val.abs() <= 1, "sin(0) should be ~0, got {val}");
    }

    #[test]
    fn test_sin_lookup_quarter() {
        ensure_init();
        // Phase at 1/4 cycle = 2^24 / 4 = 2^22
        let quarter = 1 << 22;
        let val = sin_lookup(quarter);
        // Should be near max (~2^24 / (2*pi) scale... actually Q24 output)
        // Peak of MSFA sine is ~(1<<24) = 16777216
        assert!(val > 1 << 23, "sin(pi/2) should be large positive, got {val}");
    }

    #[test]
    fn test_sin_lookup_half() {
        ensure_init();
        let half = 1 << 23;
        let val = sin_lookup(half);
        assert!(val.abs() < 1000, "sin(pi) should be ~0, got {val}");
    }

    #[test]
    fn test_exp2_lookup_zero() {
        ensure_init();
        // exp2(0) should be 2^0 = 1.0 in Q24 = (1<<24) = 16777216
        // But shifted by >> 6 in the lookup, so the base is (1<<30) >> 6 = (1<<24)
        let val = exp2_lookup(0);
        let expected = 1 << 24;
        let diff = (val - expected).abs();
        assert!(diff < 100, "exp2(0) should be ~{expected}, got {val}");
    }

    #[test]
    fn test_freqlut_basic() {
        ensure_init();
        // A4 = 440 Hz, logfreq = log2(440) * (1<<24) ≈ 147M
        let logfreq = midinote_to_logfreq(69);
        let phase_inc = freqlut_lookup(logfreq);
        // phase_inc should be positive and reasonable
        assert!(phase_inc > 0, "Phase increment should be positive, got {phase_inc}");
    }

    #[test]
    fn test_midinote_to_logfreq_octave() {
        // One octave up should add exactly (1<<24) to logfreq
        let a4 = midinote_to_logfreq(69);
        let a5 = midinote_to_logfreq(81); // 69 + 12
        let diff = a5 - a4;
        let octave = 1 << 24;
        assert!(
            (diff - octave).abs() < 2,
            "Octave should be exactly 1<<24={octave}, got diff={diff}"
        );
    }
}
