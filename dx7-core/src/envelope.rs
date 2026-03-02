//! DX7 integer log-domain envelope generator.
//!
//! Ported from Dexed/MSFA env.cc (Apache 2.0, Google Inc. / Pascal Gauthier).
//! Level is stored in Q24/doubling log format. Output is subsampled once per
//! N=64 sample block.

use crate::tables;

/// DX7 envelope parameters: 4 rates and 4 levels (all 0-99).
#[derive(Clone, Copy, Debug)]
pub struct EnvParams {
    pub rates: [u8; 4],
    pub levels: [u8; 4],
}

impl Default for EnvParams {
    fn default() -> Self {
        Self {
            rates: [99, 99, 99, 99],
            levels: [99, 99, 99, 0],
        }
    }
}

/// Level scaling lookup (maps 0..19 output levels to scaled values).
const LEVEL_LUT: [i32; 20] = [
    0, 5, 9, 13, 17, 20, 23, 25, 27, 29, 31, 33, 35, 37, 39, 41, 42, 43, 45, 46,
];

/// Scale an output level (0..99) to envelope units.
pub fn scaleoutlevel(outlevel: i32) -> i32 {
    if outlevel >= 20 {
        28 + outlevel
    } else {
        LEVEL_LUT[outlevel as usize]
    }
}

/// DX7 envelope generator — integer log-domain.
#[derive(Clone, Debug)]
pub struct Envelope {
    rates: [i32; 4],
    levels: [i32; 4],
    outlevel: i32,
    rate_scaling: i32,
    /// Level in Q24/doubling log format (16 more fractional bits than DX7).
    level: i32,
    targetlevel: i32,
    rising: bool,
    /// Current stage index (0..3 = attack/decay1/decay2/release, 4 = done).
    ix: i32,
    /// Level increment per N-sample block.
    inc: i32,
    /// Key state: true = held, false = released.
    down: bool,
}

impl Envelope {
    pub fn new() -> Self {
        Self {
            rates: [0; 4],
            levels: [0; 4],
            outlevel: 0,
            rate_scaling: 0,
            level: 0,
            targetlevel: 0,
            rising: false,
            ix: 0,
            inc: 0,
            down: true,
        }
    }

    /// Initialize envelope with DX7 parameters.
    /// `rates`/`levels`: 0..99 values from patch.
    /// `outlevel`: pre-computed in microsteps (output_level scaled + velocity + level_scaling).
    /// `rate_scaling`: qRate delta from keyboard position.
    pub fn init(&mut self, rates: &[i32; 4], levels: &[i32; 4], outlevel: i32, rate_scaling: i32) {
        for i in 0..4 {
            self.rates[i] = rates[i];
            self.levels[i] = levels[i];
        }
        self.outlevel = outlevel;
        self.rate_scaling = rate_scaling;
        self.level = 0;
        self.down = true;
        self.advance(0);
    }

    /// Get one sample of envelope output (call once per N-sample block).
    /// Returns level in Q24/doubling log format.
    #[inline]
    pub fn getsample(&mut self) -> i32 {
        if self.ix < 3 || (self.ix < 4 && !self.down) {
            if self.rising {
                const JUMPTARGET: i32 = 1716;
                if self.level < (JUMPTARGET << 16) {
                    self.level = JUMPTARGET << 16;
                }
                self.level +=
                    (((17 << 24) - self.level) >> 24) * self.inc;
                if self.level >= self.targetlevel {
                    self.level = self.targetlevel;
                    self.advance(self.ix + 1);
                }
            } else {
                self.level -= self.inc;
                if self.level <= self.targetlevel {
                    self.level = self.targetlevel;
                    self.advance(self.ix + 1);
                }
            }
        }
        self.level
    }

    /// Handle key down/up transitions.
    pub fn keydown(&mut self, d: bool) {
        if self.down != d {
            self.down = d;
            self.advance(if d { 0 } else { 3 });
        }
    }

    /// Returns true if the envelope is active (not yet finished or L4 > 0).
    pub fn is_active(&self) -> bool {
        self.ix < 4 || self.levels[3] > 0
    }

    fn advance(&mut self, newix: i32) {
        self.ix = newix;
        if self.ix < 4 {
            let newlevel = self.levels[self.ix as usize];
            let actuallevel = scaleoutlevel(newlevel) >> 1;
            let mut actuallevel = (actuallevel << 6) + self.outlevel - 4256;
            if actuallevel < 16 {
                actuallevel = 16;
            }
            self.targetlevel = actuallevel << 16;
            self.rising = self.targetlevel > self.level;

            // Rate computation
            let mut qrate = (self.rates[self.ix as usize] * 41) >> 6;
            qrate += self.rate_scaling;
            if qrate > 63 {
                qrate = 63;
            }

            self.inc = (4 + (qrate & 3)) << (2 + tables::LG_N + (qrate >> 2));
            // Sample rate compensation
            let sr_mul = tables::sr_multiplier() as i64;
            self.inc = (((self.inc as i64) * sr_mul) >> 24) as i32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ensure_init() {
        use std::sync::Once;
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            tables::init_tables(44100.0);
        });
    }

    #[test]
    fn test_envelope_attack_decay() {
        ensure_init();
        let rates = [99, 99, 99, 99];
        let levels = [99, 99, 99, 0];

        // Compute outlevel for output_level=99, no velocity/scaling
        let outlevel = scaleoutlevel(99) << 5;

        let mut env = Envelope::new();
        env.init(&rates, &levels, outlevel, 0);
        env.keydown(true);

        // Run through many blocks
        let mut last_level = 0i32;
        for _ in 0..1000 {
            last_level = env.getsample();
        }
        // With fast rates, should reach a high level (log domain)
        assert!(last_level > 1 << 22, "Level should be high after fast attack, got {last_level}");

        // Key off
        env.keydown(false);
        for _ in 0..1000 {
            last_level = env.getsample();
        }
        // In log domain, the minimum floor is 16 << 16 = 1048576.
        // After release, level should reach the floor (L4=0 target).
        let floor = 16 << 16; // MSFA envelope minimum
        assert!(
            last_level <= floor,
            "Level should be at floor after release, got {last_level} (floor={floor})"
        );
    }

    #[test]
    fn test_scaleoutlevel() {
        assert_eq!(scaleoutlevel(0), 0);
        assert_eq!(scaleoutlevel(20), 48);
        assert_eq!(scaleoutlevel(99), 127);
    }
}
