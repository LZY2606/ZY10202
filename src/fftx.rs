//! Minimal in-place radix-2 decimation-in-time FFT (f64), no external deps.
//! Length must be a power of two. Inputs/outputs are separate real/imag vecs.

pub fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    assert_eq!(n, im.len());
    assert!(n.is_power_of_two(), "fft length must be a power of two, got {n}");
    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let wlen_re = ang.cos();
        let wlen_im = ang.sin();
        let half = len / 2;
        let mut start = 0;
        while start < n {
            let mut wre = 1.0f64;
            let mut wim = 0.0f64;
            for k in 0..half {
                let a = start + k;
                let b = a + half;
                let tre = wre * re[b] - wim * im[b];
                let tim = wre * im[b] + wim * re[b];
                re[b] = re[a] - tre;
                im[b] = im[a] - tim;
                re[a] += tre;
                im[a] += tim;
                let nwr = wre * wlen_re - wim * wlen_im;
                wim = wre * wlen_im + wim * wlen_re;
                wre = nwr;
            }
            start += len;
        }
        len <<= 1;
    }
}

/// One-sided magnitude spectrum of a real signal, normalized so that the DC
/// bin equals the mean magnitude (MTF(0) = 1 after division by the DC bin).
pub fn real_spectrum(samples: &[f64]) -> Vec<f64> {
    let n = samples.len();
    assert!(n >= 2 && n.is_power_of_two());
    let mut re = samples.to_vec();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    re[..=n / 2].iter().zip(&im[..=n / 2]).map(|(a, b)| a.hypot(*b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dc_bin_is_sum() {
        let v = vec![1.0, 2.0, 3.0, 4.0];
        let s = real_spectrum(&v);
        assert!((s[0] - 10.0).abs() < 1e-9);
        // Nyquist bin for [1,2,3,4]: |1-2+3-4| = 2.
        assert!((s[2] - 2.0).abs() < 1e-9);
    }

    #[test]
    fn sinusoid_lands_on_expected_bin() {
        let n = 64usize;
        let k = 6usize;
        let v: Vec<f64> = (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * k as f64 * i as f64 / n as f64).cos())
            .collect();
        let s = real_spectrum(&v);
        // Pure cosine amplitude 1 => FFT bins at +/-k each carry n/2.
        assert!((s[k] - (n as f64 / 2.0)).abs() < 1e-7);
        for (i, mag) in s.iter().enumerate() {
            if i != k {
                assert!(*mag < 1e-7, "bin {i} leaked {mag}");
            }
        }
    }
}
