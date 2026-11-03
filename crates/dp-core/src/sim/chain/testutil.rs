//! Spectral probes shared by the chain generator tests.

use realfft::RealFftPlanner;

/// Welch one-sided PSD (Hann, 50 % overlap): (bin spacing Hz, PSD per Hz).
pub fn welch(x: &[f64], fs: f64, nfft: usize) -> (f64, Vec<f64>) {
    let fft = RealFftPlanner::<f64>::new().plan_fft_forward(nfft);
    let w: Vec<f64> = (0..nfft).map(|i| (std::f64::consts::PI * i as f64 / nfft as f64).sin().powi(2)).collect();
    let u: f64 = w.iter().map(|v| v * v).sum();
    let mut acc = vec![0.0; nfft / 2 + 1];
    let mut frames = 0;
    let mut start = 0;
    while start + nfft <= x.len() {
        let mut buf: Vec<f64> = (0..nfft).map(|i| x[start + i] * w[i]).collect();
        let mut spec = fft.make_output_vec();
        fft.process(&mut buf, &mut spec).unwrap();
        for (a, c) in acc.iter_mut().zip(&spec) {
            *a += 2.0 * c.norm_sqr() / (fs * u);
        }
        frames += 1;
        start += nfft / 2;
    }
    (fs / nfft as f64, acc.into_iter().map(|a| a / frames as f64).collect())
}

/// Total power (variance) in [lo, hi] Hz.
pub fn band_power(x: &[f64], fs: f64, lo: f64, hi: f64) -> f64 {
    let (df, p) = welch(x, fs, 4096.min(x.len().next_power_of_two() / 2));
    p.iter().enumerate().filter(|(i, _)| (lo..=hi).contains(&(*i as f64 * df))).map(|(_, v)| v * df).sum()
}

/// Power of a (possibly slightly wandering) line: band power within ±`half_hz`.
pub fn line_power(x: &[f64], fs: f64, f: f64, half_hz: f64) -> f64 {
    let (df, p) = welch(x, fs, ((fs / 0.25) as usize).min(x.len()));
    p.iter().enumerate().filter(|(i, _)| ((*i as f64 * df) - f).abs() <= half_hz).map(|(_, v)| v * df).sum()
}

pub fn db(x: f64) -> f64 {
    10.0 * x.max(1e-300).log10()
}
