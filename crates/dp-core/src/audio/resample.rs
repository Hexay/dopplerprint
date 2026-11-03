//! Integer-factor anti-aliased decimation. Streamable, so hour-long files never sit in RAM at full rate.

use std::borrow::Cow;

use super::Audio;

/// Largest divisor `q` of `sr` with `sr / q >= 2.5 * f_max`. A divisor keeps the output rate an exact integer.
pub fn decimation_factor(sr: u32, f_max: f64) -> u32 {
    let min_out = 2.5 * f_max;
    (1..=sr)
        .rev()
        .find(|&q| sr.is_multiple_of(q) && sr as f64 / q as f64 >= min_out)
        .unwrap_or(1)
}

/// Blackman-windowed sinc low-pass + keep-every-q. Output sample n is centred on input sample n·q.
pub struct Decimator {
    q: usize,
    taps: Vec<f32>,
    half: usize,
    buf: Vec<f32>,
    next: usize,
    n_in: usize,
    n_out: usize,
}

impl Decimator {
    /// `f_pass` is the highest frequency that must stay alias-free; the stopband starts at `out_sr - f_pass`.
    pub fn new(sr: u32, q: u32, f_pass: f64) -> Self {
        let q = q.max(1) as usize;
        let out_sr = sr as f64 / q as f64;
        let trans = (out_sr - 2.0 * f_pass).max(0.1 * out_sr);
        let half = if q == 1 {
            0
        } else {
            (2.75 * sr as f64 / trans).ceil() as usize
        };
        let fc = 0.5 / q as f64;
        let n = 2 * half + 1;
        let mut taps: Vec<f64> = (0..n)
            .map(|i| {
                let k = i as f64 - half as f64;
                let sinc = if k == 0.0 {
                    2.0 * fc
                } else {
                    (2.0 * std::f64::consts::PI * fc * k).sin() / (std::f64::consts::PI * k)
                };
                let x = 2.0 * std::f64::consts::PI * i as f64 / (n.max(2) - 1) as f64;
                sinc * (0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos())
            })
            .collect();
        let sum: f64 = taps.iter().sum();
        taps.iter_mut().for_each(|t| *t /= sum);
        Self {
            q,
            taps: taps.into_iter().map(|t| t as f32).collect(),
            half,
            buf: vec![0.0; half],
            next: half,
            n_in: 0,
            n_out: 0,
        }
    }

    pub fn push(&mut self, x: &[f32], out: &mut Vec<f32>) {
        self.n_in += x.len();
        self.buf.extend_from_slice(x);
        self.drain(out);
    }

    pub fn finish(mut self, out: &mut Vec<f32>) {
        let expected = self.n_in.div_ceil(self.q);
        self.buf.extend(std::iter::repeat_n(0.0, self.half + self.q));
        while self.n_out < expected && self.next + self.half < self.buf.len() {
            self.emit(out);
        }
    }

    fn drain(&mut self, out: &mut Vec<f32>) {
        while self.next + self.half < self.buf.len() {
            self.emit(out);
        }
        let consumed = self.next - self.half;
        if consumed > 1 << 16 {
            self.buf.drain(..consumed);
            self.next -= consumed;
        }
    }

    fn emit(&mut self, out: &mut Vec<f32>) {
        let w = &self.buf[self.next - self.half..=self.next + self.half];
        out.push(w.iter().zip(&self.taps).map(|(a, b)| a * b).sum());
        self.next += self.q;
        self.n_out += 1;
    }
}

/// Decimate in memory so the alias-free band covers `f_max`. Borrowed when no decimation is needed.
pub fn decimate_for(audio: &Audio, f_max: f64) -> Cow<'_, Audio> {
    let q = decimation_factor(audio.sample_rate, f_max);
    if q == 1 {
        return Cow::Borrowed(audio);
    }
    let mut d = Decimator::new(audio.sample_rate, q, f_max);
    let mut out = Vec::with_capacity(audio.samples.len() / q as usize + 1);
    for chunk in audio.samples.chunks(1 << 16) {
        d.push(chunk, &mut out);
    }
    d.finish(&mut out);
    Cow::Owned(Audio {
        sample_rate: audio.sample_rate / q,
        samples: out,
    })
}
