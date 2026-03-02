//! DX7 integer LFO with 6 waveforms.
//!
//! Ported from Dexed/MSFA lfo.cc (Apache 2.0, Google Inc.).
//! All waveform outputs are in Q24 range (0..1<<24).

use crate::tables;

/// LFO waveform types.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LfoWaveform {
    Triangle,
    SawDown,
    SawUp,
    Square,
    Sine,
    SampleAndHold,
}

impl LfoWaveform {
    pub fn from_u8(v: u8) -> Self {
        match v % 6 {
            0 => LfoWaveform::Triangle,
            1 => LfoWaveform::SawDown,
            2 => LfoWaveform::SawUp,
            3 => LfoWaveform::Square,
            4 => LfoWaveform::Sine,
            5 => LfoWaveform::SampleAndHold,
            _ => unreachable!(),
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            LfoWaveform::Triangle => 0,
            LfoWaveform::SawDown => 1,
            LfoWaveform::SawUp => 2,
            LfoWaveform::Square => 3,
            LfoWaveform::Sine => 4,
            LfoWaveform::SampleAndHold => 5,
        }
    }
}

/// LFO parameters (from DX7 patch).
#[derive(Clone, Copy, Debug)]
pub struct LfoParams {
    pub speed: u8,           // 0-99
    pub delay: u8,           // 0-99
    pub pitch_mod_depth: u8, // 0-99
    pub amp_mod_depth: u8,   // 0-99
    pub key_sync: bool,
    pub waveform: LfoWaveform,
}

impl Default for LfoParams {
    fn default() -> Self {
        Self {
            speed: 35,
            delay: 0,
            pitch_mod_depth: 0,
            amp_mod_depth: 0,
            key_sync: true,
            waveform: LfoWaveform::Triangle,
        }
    }
}

/// LFO frequency source table (100 entries, Hz values measured from DX7).
static LFO_SOURCE: [f64; 100] = [
    0.062541, 0.125031, 0.312393, 0.437120, 0.624610,
    0.750694, 0.936330, 1.125302, 1.249609, 1.436782,
    1.560915, 1.752081, 1.875117, 2.062494, 2.247191,
    2.374451, 2.560492, 2.686728, 2.873976, 2.998950,
    3.188013, 3.369840, 3.500175, 3.682224, 3.812065,
    4.000800, 4.186202, 4.310716, 4.501260, 4.623209,
    4.814636, 4.930480, 5.121901, 5.315191, 5.434783,
    5.617346, 5.750431, 5.946717, 6.062811, 6.248438,
    6.431695, 6.564264, 6.749460, 6.868132, 7.052186,
    7.250580, 7.375719, 7.556294, 7.687577, 7.877738,
    7.993605, 8.181967, 8.372405, 8.504848, 8.685079,
    8.810573, 8.986341, 9.122423, 9.300595, 9.500285,
    9.607994, 9.798158, 9.950249, 10.117361, 11.251125,
    11.384335, 12.562814, 13.676149, 13.904338, 15.092062,
    16.366612, 16.638935, 17.869907, 19.193858, 19.425019,
    20.833333, 21.034918, 22.502250, 24.003841, 24.260068,
    25.746653, 27.173913, 27.578599, 29.052876, 30.693677,
    31.191516, 32.658393, 34.317090, 34.674064, 36.416606,
    38.197097, 38.550501, 40.387722, 40.749796, 42.625746,
    44.326241, 44.883303, 46.772685, 48.590865, 49.261084,
];

/// Static LFO parameters derived from sample rate (shared across all voices).
static mut LFO_UNIT: u32 = 0;
static mut LFO_RATIO: u32 = 0;

/// Initialize LFO statics (called once from init_tables flow).
pub fn init_lfo(sample_rate: f64) {
    unsafe {
        LFO_UNIT = (tables::N as f64 * 25190424.0 / sample_rate + 0.5) as u32;
        let ratio = 4437500000.0 * tables::N as f64;
        LFO_RATIO = (ratio / sample_rate) as u32;
    }
}

/// DX7 integer LFO state.
pub struct Lfo {
    phase: u32,
    delta: u32,
    waveform: u8,
    randstate: u8,
    sync: bool,
    delaystate: u32,
    delayinc: u32,
    delayinc2: u32,
}

impl Lfo {
    pub fn new() -> Self {
        Self {
            phase: 0,
            delta: 0,
            waveform: 0,
            randstate: 0,
            sync: false,
            delaystate: 0,
            delayinc: 0,
            delayinc2: 0,
        }
    }

    /// Reset LFO with patch parameters.
    /// `params` layout: [speed, delay, pmd, amd, sync, waveform]
    pub fn reset(&mut self, lfo_params: &LfoParams) {
        let rate = lfo_params.speed.min(99) as usize;
        let lforatio = unsafe { LFO_RATIO };
        self.delta = (LFO_SOURCE[rate] * lforatio as f64) as u32;

        let a_raw = 99i32 - lfo_params.delay as i32;
        let unit = unsafe { LFO_UNIT };
        if a_raw == 99 {
            self.delayinc = !0u32;
            self.delayinc2 = !0u32;
        } else {
            let mut a = ((16 + (a_raw & 15)) << (1 + (a_raw >> 4))) as u32;
            self.delayinc = unit.wrapping_mul(a);
            a &= 0xff80;
            if a < 0x80 {
                a = 0x80;
            }
            self.delayinc2 = unit.wrapping_mul(a);
        }

        self.waveform = lfo_params.waveform.to_u8();
        self.sync = lfo_params.key_sync;
    }

    /// Get one LFO sample. Result is 0..1 in Q24.
    #[inline]
    pub fn getsample(&mut self) -> i32 {
        self.phase = self.phase.wrapping_add(self.delta);

        match self.waveform {
            0 => {
                // Triangle
                let mut x = (self.phase >> 7) as i32;
                x ^= -((self.phase >> 31) as i32);
                x & ((1 << 24) - 1)
            }
            1 => {
                // Sawtooth down
                ((!self.phase ^ (1u32 << 31)) >> 8) as i32
            }
            2 => {
                // Sawtooth up
                ((self.phase ^ (1u32 << 31)) >> 8) as i32
            }
            3 => {
                // Square
                (((!self.phase) >> 7) & (1u32 << 24)) as i32
            }
            4 => {
                // Sine
                (1 << 23) + (tables::sin_lookup((self.phase >> 8) as i32) >> 1)
            }
            5 => {
                // Sample & Hold
                if self.phase < self.delta {
                    self.randstate = ((self.randstate as u32 * 179 + 17) & 0xff) as u8;
                }
                let x = (self.randstate ^ 0x80) as i32;
                (x + 1) << 16
            }
            _ => 1 << 23,
        }
    }

    /// Get delay ramp value. Result is 0..1 in Q24.
    #[inline]
    pub fn getdelay(&mut self) -> i32 {
        let delta = if self.delaystate < (1u32 << 31) {
            self.delayinc
        } else {
            self.delayinc2
        };
        let d = self.delaystate as u64 + delta as u64;
        if d > u32::MAX as u64 {
            return 1 << 24;
        }
        self.delaystate = d as u32;
        if (d as u32) < (1u32 << 31) {
            0
        } else {
            ((d as u32) >> 7) as i32 & ((1 << 24) - 1)
        }
    }

    /// Handle key-down event (reset phase if sync, reset delay).
    pub fn keydown(&mut self) {
        if self.sync {
            self.phase = (1u32 << 31) - 1;
        }
        self.delaystate = 0;
    }
}
