//! Audio effects: Freeverb-style reverb and soft saturation.

// --- Freeverb-style reverb ---
// Based on Jezar's Freeverb (public domain).

const NUM_COMBS: usize = 8;
const NUM_ALLPASS: usize = 4;
const COMB_TUNINGS: [usize; NUM_COMBS] = [1557, 1617, 1491, 1422, 1277, 1356, 1188, 1116];
const ALLPASS_TUNINGS: [usize; NUM_ALLPASS] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;

struct CombFilter {
    buffer: Vec<f32>,
    pos: usize,
    filterstore: f32,
}

impl CombFilter {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size],
            pos: 0,
            filterstore: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, input: f32, feedback: f32, damp1: f32, damp2: f32) -> f32 {
        let output = self.buffer[self.pos];
        self.filterstore = output * damp2 + self.filterstore * damp1;
        self.buffer[self.pos] = input + self.filterstore * feedback;
        self.pos += 1;
        if self.pos >= self.buffer.len() {
            self.pos = 0;
        }
        output
    }
}

struct AllpassFilter {
    buffer: Vec<f32>,
    pos: usize,
}

impl AllpassFilter {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size],
            pos: 0,
        }
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        let bufout = self.buffer[self.pos];
        let output = -input + bufout;
        self.buffer[self.pos] = input + bufout * 0.5;
        self.pos += 1;
        if self.pos >= self.buffer.len() {
            self.pos = 0;
        }
        output
    }
}

/// Freeverb-style stereo reverb.
pub struct Reverb {
    combs_l: Vec<CombFilter>,
    combs_r: Vec<CombFilter>,
    allpass_l: Vec<AllpassFilter>,
    allpass_r: Vec<AllpassFilter>,
    room_size: f32,
    damp: f32,
    wet: f32,
    dry: f32,
    width: f32,
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44100.0;
        let combs_l: Vec<_> = COMB_TUNINGS
            .iter()
            .map(|&d| CombFilter::new((d as f32 * scale) as usize))
            .collect();
        let combs_r: Vec<_> = COMB_TUNINGS
            .iter()
            .map(|&d| CombFilter::new(((d + STEREO_SPREAD) as f32 * scale) as usize))
            .collect();
        let allpass_l: Vec<_> = ALLPASS_TUNINGS
            .iter()
            .map(|&d| AllpassFilter::new((d as f32 * scale) as usize))
            .collect();
        let allpass_r: Vec<_> = ALLPASS_TUNINGS
            .iter()
            .map(|&d| AllpassFilter::new(((d + STEREO_SPREAD) as f32 * scale) as usize))
            .collect();

        Self {
            combs_l,
            combs_r,
            allpass_l,
            allpass_r,
            room_size: 0.85,
            damp: 0.3,
            wet: 0.25,
            dry: 0.8,
            width: 1.0,
        }
    }

    /// Set reverb parameters.
    /// room_size: 0.0..1.0, damp: 0.0..1.0, wet: 0.0..1.0
    pub fn set_params(&mut self, room_size: f32, damp: f32, wet: f32) {
        self.room_size = room_size;
        self.damp = damp;
        self.wet = wet;
        self.dry = 1.0 - wet * 0.5;
    }

    /// Process mono input to stereo output.
    pub fn process_mono_to_stereo(
        &mut self,
        input: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) {
        let feedback = self.room_size;
        let damp1 = self.damp;
        let damp2 = 1.0 - damp1;
        let wet1 = self.wet * (self.width * 0.5 + 0.5);
        let wet2 = self.wet * ((1.0 - self.width) * 0.5);

        for i in 0..input.len() {
            let inp = input[i];
            let mut sum_l = 0.0f32;
            let mut sum_r = 0.0f32;

            for comb in &mut self.combs_l {
                sum_l += comb.process(inp, feedback, damp1, damp2);
            }
            for comb in &mut self.combs_r {
                sum_r += comb.process(inp, feedback, damp1, damp2);
            }

            for ap in &mut self.allpass_l {
                sum_l = ap.process(sum_l);
            }
            for ap in &mut self.allpass_r {
                sum_r = ap.process(sum_r);
            }

            out_l[i] = inp * self.dry + sum_l * wet1 + sum_r * wet2;
            out_r[i] = inp * self.dry + sum_r * wet1 + sum_l * wet2;
        }
    }
}

/// Soft saturation to simulate DX7 analog output stage warmth.
/// Applies a gentle tanh-like curve that adds even harmonics.
#[inline]
pub fn soft_saturate(x: f32) -> f32 {
    // Fast tanh approximation: x / (1 + |x|*0.3)
    // Adds subtle warmth without aggressive distortion
    let abs_x = x.abs();
    if abs_x < 0.5 {
        x // Linear region for quiet signals
    } else {
        x / (1.0 + (abs_x - 0.5) * 0.4)
    }
}

// --- Stereo chorus ---
// Classic analog chorus for the DX7 E.Piano shimmer.
// Two LFO-modulated delay lines with opposite phases for stereo spread.

/// Stereo chorus effect with two modulated delay lines.
pub struct Chorus {
    delay_line: Vec<f32>,
    write_pos: usize,
    max_delay: usize,
    lfo_phase: f64,
    lfo_rate: f64,    // Hz
    center_delay: f64, // in samples
    depth: f64,        // modulation depth in samples
    mix: f32,          // wet/dry mix (0=dry, 1=wet)
    sample_rate: f64,
}

impl Chorus {
    /// Create a new stereo chorus.
    /// rate_hz: LFO rate (typ. 0.8-2.0 Hz)
    /// delay_ms: center delay (typ. 7-10 ms)
    /// depth_ms: modulation depth (typ. 2-4 ms)
    /// mix: wet/dry (typ. 0.5)
    pub fn new(sample_rate: f64, rate_hz: f64, delay_ms: f64, depth_ms: f64, mix: f32) -> Self {
        let center_delay = delay_ms * sample_rate / 1000.0;
        let depth = depth_ms * sample_rate / 1000.0;
        let max_delay = (center_delay + depth + 2.0) as usize;
        Self {
            delay_line: vec![0.0; max_delay + 1],
            write_pos: 0,
            max_delay,
            lfo_phase: 0.0,
            lfo_rate: rate_hz,
            center_delay,
            depth,
            mix,
            sample_rate,
        }
    }

    /// Process mono input to stereo output with chorus.
    pub fn process(&mut self, input: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        let lfo_inc = self.lfo_rate / self.sample_rate;

        for i in 0..input.len() {
            // Write input to delay line
            self.delay_line[self.write_pos] = input[i];

            // Two LFOs at opposite phases for stereo
            let lfo_l = (self.lfo_phase * 2.0 * std::f64::consts::PI).sin();
            let lfo_r = ((self.lfo_phase + 0.25) * 2.0 * std::f64::consts::PI).sin();

            // Compute delay times
            let delay_l = self.center_delay + self.depth * lfo_l;
            let delay_r = self.center_delay + self.depth * lfo_r;

            // Read from delay line with linear interpolation
            let wet_l = self.read_delay(delay_l);
            let wet_r = self.read_delay(delay_r);

            // Mix dry and wet
            let dry = 1.0 - self.mix * 0.5;
            out_l[i] = input[i] * dry + wet_l * self.mix;
            out_r[i] = input[i] * dry + wet_r * self.mix;

            // Advance
            self.write_pos += 1;
            if self.write_pos > self.max_delay {
                self.write_pos = 0;
            }
            self.lfo_phase += lfo_inc;
            if self.lfo_phase >= 1.0 {
                self.lfo_phase -= 1.0;
            }
        }
    }

    /// Process stereo buffers in-place with chorus widening.
    pub fn process_stereo_inplace(&mut self, left: &mut [f32], right: &mut [f32]) {
        let lfo_inc = self.lfo_rate / self.sample_rate;

        for i in 0..left.len() {
            let mono = (left[i] + right[i]) * 0.5;
            self.delay_line[self.write_pos] = mono;

            let lfo_l = (self.lfo_phase * 2.0 * std::f64::consts::PI).sin();
            let lfo_r = ((self.lfo_phase + 0.25) * 2.0 * std::f64::consts::PI).sin();

            let delay_l = self.center_delay + self.depth * lfo_l;
            let delay_r = self.center_delay + self.depth * lfo_r;

            let wet_l = self.read_delay(delay_l);
            let wet_r = self.read_delay(delay_r);

            let dry = 1.0 - self.mix * 0.5;
            left[i] = left[i] * dry + wet_l * self.mix;
            right[i] = right[i] * dry + wet_r * self.mix;

            self.write_pos += 1;
            if self.write_pos > self.max_delay {
                self.write_pos = 0;
            }
            self.lfo_phase += lfo_inc;
            if self.lfo_phase >= 1.0 {
                self.lfo_phase -= 1.0;
            }
        }
    }

    #[inline]
    fn read_delay(&self, delay: f64) -> f32 {
        let delay = delay.max(0.0).min(self.max_delay as f64);
        let int_part = delay as usize;
        let frac = delay - int_part as f64;

        let pos0 = if self.write_pos >= int_part {
            self.write_pos - int_part
        } else {
            self.max_delay + 1 + self.write_pos - int_part
        };
        let pos1 = if pos0 == 0 { self.max_delay } else { pos0 - 1 };

        let s0 = self.delay_line[pos0];
        let s1 = self.delay_line[pos1];
        s0 + (s1 - s0) * frac as f32
    }
}

// --- DC-blocking high-pass filter ---
// Simulates the coupling capacitors on the real DX7's analog output.
// FM synthesis can produce large DC offsets from asymmetric modulation;
// this removes them while passing all audible frequencies.

/// DC-blocking high-pass filter (two cascaded 1st-order stages).
///
/// FM synthesis produces time-varying DC offset from asymmetric modulation.
/// Default cutoff 5 Hz matches DX7 hardware coupling capacitor behavior:
/// -0.5 dB at 16 Hz, -0.1 dB at 33 Hz, preserving all audible bass.
pub struct DcBlocker {
    // Stage 1
    x1a: f64,
    y1a: f64,
    // Stage 2
    x1b: f64,
    y1b: f64,
    r: f64,
}

impl DcBlocker {
    pub fn new(sample_rate: f64) -> Self {
        Self::with_cutoff(sample_rate, 5.0)
    }

    /// Create a DC blocker with a custom cutoff frequency in Hz.
    /// Lower cutoff preserves more bass but tracks DC slower.
    pub fn with_cutoff(sample_rate: f64, cutoff_hz: f64) -> Self {
        let r = 1.0 - (2.0 * std::f64::consts::PI * cutoff_hz / sample_rate);
        Self { x1a: 0.0, y1a: 0.0, x1b: 0.0, y1b: 0.0, r }
    }

    /// Process a buffer of samples in-place.
    pub fn process(&mut self, buf: &mut [f32]) {
        for sample in buf.iter_mut() {
            // Stage 1
            let x0 = *sample as f64;
            let y0 = x0 - self.x1a + self.r * self.y1a;
            self.x1a = x0;
            self.y1a = y0;
            // Stage 2
            let y1 = y0 - self.x1b + self.r * self.y1b;
            self.x1b = y0;
            self.y1b = y1;
            *sample = y1 as f32;
        }
    }
}

// --- Biquad low-pass filter ---
// Simulates the DX7's reconstruction LPF (~10-12 kHz).

/// 2nd-order Butterworth low-pass filter (biquad).
pub struct LowPassFilter {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl LowPassFilter {
    /// Create a 2nd-order Butterworth LPF at the given cutoff frequency.
    pub fn new(sample_rate: f64, cutoff_hz: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * cutoff_hz / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        // Q = 1/sqrt(2) for Butterworth
        let alpha = sin_w0 / (2.0 * std::f64::consts::FRAC_1_SQRT_2);

        let b0 = (1.0 - cos_w0) / 2.0;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Process a buffer of samples in-place.
    pub fn process(&mut self, buf: &mut [f32]) {
        for sample in buf.iter_mut() {
            let x0 = *sample as f64;
            let y0 = self.b0 * x0 + self.b1 * self.x1 + self.b2 * self.x2
                - self.a1 * self.y1
                - self.a2 * self.y2;
            self.x2 = self.x1;
            self.x1 = x0;
            self.y2 = self.y1;
            self.y1 = y0;
            *sample = y0 as f32;
        }
    }
}

/// 4th-order Butterworth LPF (two cascaded biquads).
/// Better approximation of the DX7's reconstruction filter.
pub struct LowPassFilter4 {
    stage1: LowPassFilter,
    stage2: LowPassFilter,
}

impl LowPassFilter4 {
    pub fn new(sample_rate: f64, cutoff_hz: f64) -> Self {
        // For 4th-order Butterworth, cascade two 2nd-order sections
        // with Q values from Butterworth polynomial
        let w0 = 2.0 * std::f64::consts::PI * cutoff_hz / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();

        // Stage 1: Q = 1/(2*cos(pi/8)) ≈ 0.5412
        let q1 = 0.54119610;
        let alpha1 = sin_w0 / (2.0 * q1);
        let stage1 = Self::make_biquad(cos_w0, alpha1);

        // Stage 2: Q = 1/(2*cos(3*pi/8)) ≈ 1.3066
        let q2 = 1.30656296;
        let alpha2 = sin_w0 / (2.0 * q2);
        let stage2 = Self::make_biquad(cos_w0, alpha2);

        Self { stage1, stage2 }
    }

    fn make_biquad(cos_w0: f64, alpha: f64) -> LowPassFilter {
        let b0 = (1.0 - cos_w0) / 2.0;
        let b1 = 1.0 - cos_w0;
        let b2 = (1.0 - cos_w0) / 2.0;
        let a0 = 1.0 + alpha;
        LowPassFilter {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        self.stage1.process(buf);
        self.stage2.process(buf);
    }
}

/// High-pass filter (2nd-order Butterworth) for the exciter.
pub struct HighPassFilter {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl HighPassFilter {
    pub fn new(sample_rate: f64, cutoff_hz: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * cutoff_hz / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * std::f64::consts::FRAC_1_SQRT_2);

        let b0 = (1.0 + cos_w0) / 2.0;
        let b1 = -(1.0 + cos_w0);
        let b2 = (1.0 + cos_w0) / 2.0;
        let a0 = 1.0 + alpha;

        Self {
            b0: b0 / a0, b1: b1 / a0, b2: b2 / a0,
            a1: (-2.0 * cos_w0) / a0, a2: (1.0 - alpha) / a0,
            x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0,
        }
    }

    pub fn process_sample(&mut self, x0: f64) -> f64 {
        let y0 = self.b0 * x0 + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x0;
        self.y2 = self.y1;
        self.y1 = y0;
        y0
    }
}

/// Exciter effect — generates harmonics via soft saturation, then
/// high-passes to extract only the added brightness and mixes back in.
pub struct Exciter {
    hpf: HighPassFilter,
    drive: f32,
    mix: f32,
}

impl Exciter {
    /// Create an exciter.
    /// - `sample_rate`: audio sample rate
    /// - `freq_hz`: high-pass cutoff (harmonics above this are added). ~3000-5000 Hz typical.
    /// - `drive`: saturation amount (1.0 = mild, 3.0 = aggressive)
    /// - `mix`: how much of the excited signal to add (0.0-1.0)
    pub fn new(sample_rate: f64, freq_hz: f64, drive: f32, mix: f32) -> Self {
        Self {
            hpf: HighPassFilter::new(sample_rate, freq_hz),
            drive,
            mix,
        }
    }

    /// Process a stereo pair of buffers in-place.
    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mono = (*l + *r) * 0.5;
            // Soft-clip saturation to generate harmonics
            let driven = (mono * self.drive).tanh();
            // High-pass to isolate only the generated harmonics
            let harmonics = self.hpf.process_sample(driven as f64) as f32;
            // Mix back
            *l += harmonics * self.mix;
            *r += harmonics * self.mix;
        }
    }
}

/// Stereo tremolo — LFO modulates amplitude with opposite phase per channel.
/// Creates the classic "wow wow" pulsing effect heard on 80s ballad recordings.
pub struct StereoTremolo {
    phase: f64,
    rate: f64,       // Hz
    depth: f32,      // 0.0 = no effect, 1.0 = full mute at trough
    sample_rate: f64,
}

impl StereoTremolo {
    pub fn new(sample_rate: f64, rate_hz: f64, depth: f32) -> Self {
        Self { phase: 0.0, rate: rate_hz, depth, sample_rate }
    }

    pub fn process_stereo(&mut self, left: &mut [f32], right: &mut [f32]) {
        let inc = self.rate / self.sample_rate;
        let two_pi = 2.0 * std::f64::consts::PI;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            // Sine LFO, 90-degree offset between L/R for stereo movement
            let lfo_l = (self.phase * two_pi).sin() as f32;
            let lfo_r = ((self.phase + 0.25) * two_pi).sin() as f32;
            // Map LFO (-1..1) to gain (1-depth .. 1)
            let gain_l = 1.0 - self.depth * 0.5 * (1.0 - lfo_l);
            let gain_r = 1.0 - self.depth * 0.5 * (1.0 - lfo_r);
            *l *= gain_l;
            *r *= gain_r;
            self.phase += inc;
            if self.phase >= 1.0 { self.phase -= 1.0; }
        }
    }
}

/// Stereo widener using mid-side processing.
/// Boosts the side (L-R) component relative to mid (L+R) to widen the image.
pub struct StereoWidener {
    width: f32, // 1.0 = no change, >1.0 = wider, <1.0 = narrower
}

impl StereoWidener {
    /// `width`: 1.0 = unchanged, 1.5 = 50% wider, 2.0 = double width
    pub fn new(width: f32) -> Self {
        Self { width }
    }

    pub fn process_stereo(&self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let mid = (*l + *r) * 0.5;
            let side = (*l - *r) * 0.5;
            *l = mid + side * self.width;
            *r = mid - side * self.width;
        }
    }
}
