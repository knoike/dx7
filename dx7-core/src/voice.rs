//! Single DX7 voice: 6 operators + bus-flag algorithm routing + LFO + pitch env.
//!
//! Ported from Dexed/MSFA fm_core.cc and dx7note.cc
//! (Apache 2.0, Google Inc. / Pascal Gauthier).

use crate::envelope::{self, Envelope};
use crate::lfo::Lfo;
use crate::operator::{self, FmOpParams, AMPMODSENSTAB, PITCHMODSENSTAB};
use crate::patch::DxVoice;
use crate::pitchenv::PitchEnv;
use crate::tables::{self, N, LG_N};

// --- Algorithm bus flags (from fm_core.h) ---

const _OUT_BUS_ONE: i32 = 1 << 0;
const _OUT_BUS_TWO: i32 = 1 << 1;
const OUT_BUS_ADD: i32 = 1 << 2;
const _IN_BUS_ONE: i32 = 1 << 4;
const _IN_BUS_TWO: i32 = 1 << 5;
const _FB_IN: i32 = 1 << 6;
const _FB_OUT: i32 = 1 << 7;

/// Algorithm routing table: 32 algorithms, 6 operators each.
/// Each byte encodes input bus, output bus, and feedback flags.
/// Ported directly from fm_core.cc.
const ALGORITHMS: [[i32; 6]; 32] = [
    [0xc1, 0x11, 0x11, 0x14, 0x01, 0x14], // 1
    [0x01, 0x11, 0x11, 0x14, 0xc1, 0x14], // 2
    [0xc1, 0x11, 0x14, 0x01, 0x11, 0x14], // 3
    [0xc1, 0x11, 0x94, 0x01, 0x11, 0x14], // 4
    [0xc1, 0x14, 0x01, 0x14, 0x01, 0x14], // 5
    [0xc1, 0x94, 0x01, 0x14, 0x01, 0x14], // 6
    [0xc1, 0x11, 0x05, 0x14, 0x01, 0x14], // 7
    [0x01, 0x11, 0xc5, 0x14, 0x01, 0x14], // 8
    [0x01, 0x11, 0x05, 0x14, 0xc1, 0x14], // 9
    [0x01, 0x05, 0x14, 0xc1, 0x11, 0x14], // 10
    [0xc1, 0x05, 0x14, 0x01, 0x11, 0x14], // 11
    [0x01, 0x05, 0x05, 0x14, 0xc1, 0x14], // 12
    [0xc1, 0x05, 0x05, 0x14, 0x01, 0x14], // 13
    [0xc1, 0x05, 0x11, 0x14, 0x01, 0x14], // 14
    [0x01, 0x05, 0x11, 0x14, 0xc1, 0x14], // 15
    [0xc1, 0x11, 0x02, 0x25, 0x05, 0x14], // 16
    [0x01, 0x11, 0x02, 0x25, 0xc5, 0x14], // 17
    [0x01, 0x11, 0x11, 0xc5, 0x05, 0x14], // 18
    [0xc1, 0x14, 0x14, 0x01, 0x11, 0x14], // 19
    [0x01, 0x05, 0x14, 0xc1, 0x14, 0x14], // 20
    [0x01, 0x14, 0x14, 0xc1, 0x14, 0x14], // 21
    [0xc1, 0x14, 0x14, 0x14, 0x01, 0x14], // 22
    [0xc1, 0x14, 0x14, 0x01, 0x14, 0x04], // 23
    [0xc1, 0x14, 0x14, 0x14, 0x04, 0x04], // 24
    [0xc1, 0x14, 0x14, 0x04, 0x04, 0x04], // 25
    [0xc1, 0x05, 0x14, 0x01, 0x14, 0x04], // 26
    [0x01, 0x05, 0x14, 0xc1, 0x14, 0x04], // 27
    [0x04, 0xc1, 0x11, 0x14, 0x01, 0x14], // 28
    [0xc1, 0x14, 0x01, 0x14, 0x04, 0x04], // 29
    [0x04, 0xc1, 0x11, 0x14, 0x04, 0x04], // 30
    [0xc1, 0x14, 0x04, 0x04, 0x04, 0x04], // 31
    [0xc4, 0x04, 0x04, 0x04, 0x04, 0x04], // 32
];

/// Check if an operator is a carrier in the given algorithm.
pub fn is_carrier(algorithm: usize, op: usize) -> bool {
    (ALGORITHMS[algorithm][op] & OUT_BUS_ADD) != 0
}

/// State of a voice slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VoiceState {
    Inactive,
    Active,
    Released,
}

/// A single DX7 voice instance.
pub struct Voice {
    pub state: VoiceState,
    pub note: u8,
    pub velocity: u8,
    pub age: u32,

    // Per-operator state
    params: [FmOpParams; 6],
    env: [Envelope; 6],
    basepitch: [i32; 6],
    ampmodsens: [u32; 6],
    op_mode: [i32; 6],

    // Voice-level state
    pitchenv: PitchEnv,
    lfo: Lfo,
    algorithm: i32,
    fb_buf: [i32; 2],
    fb_shift: i32,
    pitchmoddepth: i32,
    pitchmodsens: i32,
    ampmoddepth: i32,

    /// Pitch bend offset in the log-frequency domain (set by Synth each block).
    pub pitch_bend: i32,
    /// Mod wheel value 0–127 (set by Synth each block). Scales LFO mod depths.
    pub mod_wheel: i32,

    // Intermediate buffers for bus rendering
    buf: [[i32; N]; 2],
}

impl Voice {
    pub fn new() -> Self {
        Self {
            state: VoiceState::Inactive,
            note: 0,
            velocity: 0,
            age: 0,
            params: [
                FmOpParams::new(),
                FmOpParams::new(),
                FmOpParams::new(),
                FmOpParams::new(),
                FmOpParams::new(),
                FmOpParams::new(),
            ],
            env: [
                Envelope::new(),
                Envelope::new(),
                Envelope::new(),
                Envelope::new(),
                Envelope::new(),
                Envelope::new(),
            ],
            basepitch: [0; 6],
            ampmodsens: [0; 6],
            op_mode: [0; 6],
            pitchenv: PitchEnv::new(),
            lfo: Lfo::new(),
            algorithm: 0,
            fb_buf: [0; 2],
            fb_shift: 16,
            pitchmoddepth: 0,
            pitchmodsens: 0,
            ampmoddepth: 0,
            pitch_bend: 0,
            mod_wheel: 0,
            buf: [[0; N]; 2],
        }
    }

    /// Initialize voice for a new note with the given patch.
    /// Ported from dx7note.cc Dx7Note::init().
    pub fn note_on(&mut self, patch: &DxVoice, note: u8, velocity: u8) {
        self.note = note;
        self.velocity = velocity;
        self.state = VoiceState::Active;
        self.age = 0;

        // Transpose
        let midinote = (note as i32 + patch.transpose as i32 - 24).clamp(0, 127);

        // Per-operator initialization
        for op in 0..6 {
            let p = &patch.operators[op];

            let rates = [
                p.eg.rates[0] as i32,
                p.eg.rates[1] as i32,
                p.eg.rates[2] as i32,
                p.eg.rates[3] as i32,
            ];
            let levels = [
                p.eg.levels[0] as i32,
                p.eg.levels[1] as i32,
                p.eg.levels[2] as i32,
                p.eg.levels[3] as i32,
            ];

            // Output level with scaling
            let mut outlevel = envelope::scaleoutlevel(p.output_level as i32);
            let level_scaling = operator::scale_level(
                midinote,
                p.kbd_level_scaling_break_point as i32,
                p.kbd_level_scaling_left_depth as i32,
                p.kbd_level_scaling_right_depth as i32,
                p.kbd_level_scaling_left_curve as i32,
                p.kbd_level_scaling_right_curve as i32,
            );
            outlevel += level_scaling;
            outlevel = outlevel.min(127);
            outlevel <<= 5;
            outlevel += operator::scale_velocity(velocity as i32, p.key_velocity_sensitivity as i32);
            outlevel = outlevel.max(0);

            let rate_scaling = operator::scale_rate(midinote, p.kbd_rate_scaling as i32);
            self.env[op].init(&rates, &levels, outlevel, rate_scaling);
            self.env[op].keydown(true);

            // Frequency
            let freq = operator::osc_freq(
                midinote,
                p.osc_mode as i32,
                p.osc_freq_coarse as i32,
                p.osc_freq_fine as i32,
                p.osc_detune as i32,
            );
            self.basepitch[op] = freq;
            self.op_mode[op] = p.osc_mode as i32;
            self.ampmodsens[op] = AMPMODSENSTAB[(p.amp_mod_sensitivity & 3) as usize];

            // Reset operator state
            if patch.osc_key_sync {
                self.params[op].phase = 0;
                self.params[op].gain_out = 0;
            }
        }

        // Pitch envelope
        let pe_rates = [
            patch.pitch_eg.rates[0] as i32,
            patch.pitch_eg.rates[1] as i32,
            patch.pitch_eg.rates[2] as i32,
            patch.pitch_eg.rates[3] as i32,
        ];
        let pe_levels = [
            patch.pitch_eg.levels[0] as i32,
            patch.pitch_eg.levels[1] as i32,
            patch.pitch_eg.levels[2] as i32,
            patch.pitch_eg.levels[3] as i32,
        ];
        self.pitchenv.set(&pe_rates, &pe_levels);

        // Voice globals
        self.algorithm = patch.algorithm as i32;
        let feedback = patch.feedback as i32;
        self.fb_shift = if feedback != 0 {
            operator::FEEDBACK_BITDEPTH - feedback
        } else {
            16
        };
        self.pitchmoddepth = (patch.lfo.pitch_mod_depth as i32 * 165) >> 6;
        self.pitchmodsens = PITCHMODSENSTAB[(patch.pitch_mod_sensitivity & 7) as usize] as i32;
        self.ampmoddepth = (patch.lfo.amp_mod_depth as i32 * 165) >> 6;

        // LFO
        self.lfo.reset(&patch.lfo);
        self.lfo.keydown();
        self.fb_buf = [0; 2];
    }

    /// Release the voice (key off).
    pub fn note_off(&mut self) {
        if self.state == VoiceState::Active {
            self.state = VoiceState::Released;
            for op in 0..6 {
                self.env[op].keydown(false);
            }
            self.pitchenv.keydown(false);
        }
    }

    /// Check if the voice is completely finished.
    pub fn is_finished(&self) -> bool {
        if self.state == VoiceState::Inactive {
            return true;
        }
        if self.state == VoiceState::Released {
            let alg_idx = (self.algorithm as usize).min(31);
            for op in 0..6 {
                if is_carrier(alg_idx, op) && self.env[op].is_active() {
                    return false;
                }
            }
            return true;
        }
        false
    }

    /// Render one N-sample block into the output buffer.
    /// Ported from dx7note.cc compute() + fm_core.cc render().
    pub fn render(&mut self, output: &mut [i32; N]) {
        if self.state == VoiceState::Inactive {
            return;
        }

        self.age += 1;

        // --- LFO ---
        let lfo_val = self.lfo.getsample();
        let lfo_delay = self.lfo.getdelay();

        // --- Pitch modulation (scaled by mod wheel) ---
        let effective_pmd = (self.pitchmoddepth as i64 * self.mod_wheel as i64 / 127) as i32;
        let pmd = effective_pmd as u32 as u64 * lfo_delay as u32 as u64;
        let senslfo = self.pitchmodsens * (lfo_val - (1 << 23));
        let pmod_1 = ((pmd as i64 * senslfo as i64) >> 39).unsigned_abs() as i32;
        let pitch_mod = self.pitchenv.getsample()
            + pmod_1 * if senslfo < 0 { -1 } else { 1 }
            + self.pitch_bend;

        // --- Amplitude modulation (scaled by mod wheel) ---
        let effective_amd = (self.ampmoddepth as i64 * self.mod_wheel as i64 / 127) as i32;
        let lfo_val_inv = (1 << 24) - lfo_val;
        let amod_1 = ((effective_amd as i64 * lfo_delay as i64) >> 8) as u32;
        let amod_1 = ((amod_1 as u64 * lfo_val_inv as u64) >> 24) as u32;
        let amd_mod = amod_1;

        // --- Per-operator: compute level and frequency ---
        for op in 0..6 {
            let level = self.env[op].getsample();

            if self.ampmodsens[op] != 0 {
                let sensamp = ((amd_mod as u64 * self.ampmodsens[op] as u64) >> 24) as u32;
                let pt = ((sensamp as f64 / 262144.0 * 0.07 + 12.2).exp()) as u32;
                let ldiff = ((level as u64 * ((pt as u64) << 4)) >> 28) as i32;
                self.params[op].level_in = level - ldiff;
            } else {
                self.params[op].level_in = level;
            }

            let basepitch = self.basepitch[op];
            if self.op_mode[op] != 0 {
                // Fixed frequency mode: no pitch mod
                self.params[op].freq = tables::freqlut_lookup(basepitch);
            } else {
                self.params[op].freq = tables::freqlut_lookup(basepitch + pitch_mod);
            }
        }

        // --- Render via bus-flag routing (fm_core.cc render()) ---
        self.render_core(output);

        // Check if voice is finished
        if self.is_finished() {
            self.state = VoiceState::Inactive;
        }
    }

    /// Compute MkI log-attenuation gain from the previous gain_out value.
    /// Maps gain_out == 0 (uninitialised) to near-silence.
    #[inline]
    fn mki_gain1(gain_out: u16) -> u16 {
        if gain_out == 0 { tables::ENV_MAX - 1 } else { gain_out }
    }

    /// Compute MkI log-attenuation gain from the envelope level.
    #[inline]
    fn mki_gain2(level_in: i32) -> u16 {
        (tables::ENV_MAX as i32 - (level_in >> 14)).clamp(0, tables::ENV_MAX as i32) as u16
    }

    /// Core rendering using MkI log-domain FM with bus-flag algorithm routing.
    fn render_core(&mut self, output: &mut [i32; N]) {
        const K_LEVEL_THRESH: u16 = tables::ENV_MAX - 100; // 16284
        let alg_idx = (self.algorithm as usize).min(31);
        let mut alg = ALGORITHMS[alg_idx];
        let mut has_contents = [true, false, false];
        let fb_on = self.fb_shift < 16;

        // For algorithms 4 and 6 with feedback, redirect op 0 output to carrier bus
        if fb_on && (alg_idx == 3 || alg_idx == 5) {
            alg[0] = 0xc4;
        }

        let mut skip_count: usize = 0;

        for op in 0..6 {
            if skip_count > 0 {
                skip_count -= 1;
                continue;
            }

            let flags = alg[op];
            let mut add = (flags & OUT_BUS_ADD) != 0;
            let inbus = (flags >> 4) & 3;
            let outbus = flags & 3;

            let gain1 = Self::mki_gain1(self.params[op].gain_out);
            let gain2 = Self::mki_gain2(self.params[op].level_in);
            self.params[op].gain_out = gain2;

            if gain1 <= K_LEVEL_THRESH || gain2 <= K_LEVEL_THRESH {
                if !has_contents[outbus as usize] {
                    add = false;
                }

                if inbus == 0 || !has_contents[inbus as usize] {
                    if (flags & 0xc0) == 0xc0 && fb_on {
                        // Feedback operator — algorithm-specific handling
                        let phase = self.params[op].phase;
                        let freq = self.params[op].freq;
                        match alg_idx {
                            3 => {
                                // Algorithm 4: fused 3-operator feedback chain
                                let gain2_1 = Self::mki_gain2(self.params[1].level_in);
                                self.params[1].gain_out = gain2_1;
                                let gain1_1 = if gain2_1 == 0 { tables::ENV_MAX - 1 } else { gain2_1 };
                                let gain2_2 = Self::mki_gain2(self.params[2].level_in);
                                self.params[2].gain_out = gain2_2;
                                let gain1_2 = if gain2_2 == 0 { tables::ENV_MAX - 1 } else { gain2_2 };

                                operator::compute_fb3(
                                    output,
                                    phase, freq, gain1, gain2,
                                    self.params[1].phase, self.params[1].freq, gain1_1, gain2_1,
                                    self.params[2].phase, self.params[2].freq, gain1_2, gain2_2,
                                    &mut self.fb_buf,
                                    (self.fb_shift + 2).min(16),
                                );
                                self.params[1].phase = self.params[1].phase.wrapping_add(
                                    self.params[1].freq << LG_N,
                                );
                                self.params[2].phase = self.params[2].phase.wrapping_add(
                                    self.params[2].freq << LG_N,
                                );
                                skip_count = 2;
                            }
                            5 => {
                                // Algorithm 6: fused 2-operator feedback chain
                                let gain2_1 = Self::mki_gain2(self.params[1].level_in);
                                self.params[1].gain_out = gain2_1;
                                let gain1_1 = if gain2_1 == 0 { tables::ENV_MAX - 1 } else { gain2_1 };

                                operator::compute_fb2(
                                    output,
                                    phase, freq, gain1, gain2,
                                    self.params[1].phase, self.params[1].freq, gain1_1, gain2_1,
                                    &mut self.fb_buf,
                                    (self.fb_shift + 2).min(16),
                                );
                                self.params[1].phase = self.params[1].phase.wrapping_add(
                                    self.params[1].freq << LG_N,
                                );
                                skip_count = 1;
                            }
                            31 => {
                                // Algorithm 32: single FB with fb_shift+2
                                if outbus == 0 {
                                    operator::compute_fb(
                                        output, phase, freq, gain1, gain2,
                                        &mut self.fb_buf, (self.fb_shift + 2).min(16), add,
                                    );
                                } else {
                                    let buf_idx = (outbus - 1) as usize;
                                    let mut tmp = self.buf[buf_idx];
                                    operator::compute_fb(
                                        &mut tmp, phase, freq, gain1, gain2,
                                        &mut self.fb_buf, (self.fb_shift + 2).min(16), add,
                                    );
                                    self.buf[buf_idx] = tmp;
                                }
                            }
                            _ => {
                                // Standard feedback
                                if outbus == 0 {
                                    operator::compute_fb(
                                        output, phase, freq, gain1, gain2,
                                        &mut self.fb_buf, self.fb_shift, add,
                                    );
                                } else {
                                    let buf_idx = (outbus - 1) as usize;
                                    let mut tmp = self.buf[buf_idx];
                                    operator::compute_fb(
                                        &mut tmp, phase, freq, gain1, gain2,
                                        &mut self.fb_buf, self.fb_shift, add,
                                    );
                                    self.buf[buf_idx] = tmp;
                                }
                            }
                        }
                    } else {
                        // Pure sine (no input)
                        let phase = self.params[op].phase;
                        let freq = self.params[op].freq;
                        if outbus == 0 {
                            operator::compute_pure(
                                output, phase, freq, gain1, gain2, add,
                            );
                        } else {
                            let buf_idx = (outbus - 1) as usize;
                            let mut tmp = self.buf[buf_idx];
                            operator::compute_pure(
                                &mut tmp, phase, freq, gain1, gain2, add,
                            );
                            self.buf[buf_idx] = tmp;
                        }
                    }
                } else {
                    // Modulated by input bus
                    let phase = self.params[op].phase;
                    let freq = self.params[op].freq;
                    let input = self.buf[(inbus - 1) as usize];
                    if outbus == 0 {
                        operator::compute(
                            output, &input, phase, freq, gain1, gain2, add,
                        );
                    } else {
                        let buf_idx = (outbus - 1) as usize;
                        let mut tmp = if buf_idx == (inbus - 1) as usize {
                            input
                        } else {
                            self.buf[buf_idx]
                        };
                        operator::compute(
                            &mut tmp, &input, phase, freq, gain1, gain2, add,
                        );
                        self.buf[buf_idx] = tmp;
                    }
                }
                has_contents[outbus as usize] = true;
            } else if !add {
                has_contents[outbus as usize] = false;
            }

            self.params[op].phase = self.params[op].phase.wrapping_add(
                self.params[op].freq << LG_N,
            );
        }
    }
}
