//! Original bounded display-bloom operator. Three separable normalized box
//! convolutions spread positive scene-linear highlights. This is an artistic
//! screen-space filter, not a lens simulation or a transport estimator.
use super::{CancelToken, ErrorCode, GPU_PATH_LIMITS, JobError, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bloom {
    pub strength: f64,
    /// Positive relative scene-linear radiance, before exposure and tone mapping.
    pub threshold: f64,
    /// Box half-width in output pixels, repeated three times per axis.
    pub radius: u32,
}
impl Default for Bloom {
    fn default() -> Self {
        Self { strength: 0.0, threshold: 1.0, radius: 8 }
    }
}
impl Bloom {
    pub fn active(self) -> bool {
        self.strength != 0.0
    }
    pub fn validate(self) -> Result<()> {
        if !self.strength.is_finite()
            || !(0.0..=2.0).contains(&self.strength)
            || !self.threshold.is_finite()
            || !(0.0..=1e6).contains(&self.threshold)
            || !(1..=64).contains(&self.radius)
        {
            return Err(invalid("bloom requires strength 0..2, finite threshold 0..1000000 and radius 1..64 pixels"));
        }
        Ok(())
    }
    pub fn scratch_bytes(self, width: u32, height: u32) -> Result<u64> {
        self.validate()?;
        if !self.active() {
            return Ok(0);
        }
        u64::from(width).checked_mul(u64::from(height)).and_then(|n| n.checked_mul(24)).ok_or_else(|| invalid("bloom scratch overflow"))
    }
}
fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidRequest, message)
}
fn allocate(count: usize) -> Result<Vec<[f32; 3]>> {
    let mut result = Vec::new();
    result.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "bloom scratch allocation failed"))?;
    result.resize(count, [0.0; 3]);
    Ok(result)
}

pub struct BloomTask {
    pixels: Vec<[f32; 4]>,
    horizontal: Vec<[f32; 3]>,
    vertical: Vec<[f32; 3]>,
    width: usize,
    height: usize,
    options: Bloom,
    cancel: CancelToken,
    phase: usize,
    line: usize,
}
impl BloomTask {
    pub fn new(width: u32, height: u32, pixels: Vec<[f32; 4]>, options: Bloom, cancel: CancelToken) -> Result<Self> {
        options.validate()?;
        cancel.check()?;
        let count = u64::from(width).checked_mul(u64::from(height)).ok_or_else(|| invalid("bloom dimensions overflow"))?;
        if width == 0
            || height == 0
            || u64::from(width) > GPU_PATH_LIMITS.max_dimension
            || u64::from(height) > GPU_PATH_LIMITS.max_dimension
            || count > GPU_PATH_LIMITS.max_pixels
            || usize::try_from(count).ok() != Some(pixels.len())
            || !options.active()
        {
            return Err(invalid("bloom requires an admitted nonempty image and positive strength"));
        }
        let horizontal = allocate(pixels.len())?;
        let vertical = allocate(pixels.len())?;
        Ok(Self { pixels, horizontal, vertical, width: width as usize, height: height as usize, options, cancel, phase: 0, line: 0 })
    }
    pub fn total_lines(&self) -> u64 {
        (4 * self.height + 3 * self.width) as u64
    }
    pub fn completed_lines(&self) -> u64 {
        let previous: usize = (0..self.phase).map(|p| if p == 0 || p % 2 == 1 { self.height } else { self.width }).sum();
        (previous + self.line) as u64
    }
    /// Every call has bounded line and pixel loops, independent of kernel radius.
    pub fn step(&mut self, lines: usize) -> Result<bool> {
        self.cancel.check()?;
        for _ in 0..lines.clamp(1, 32) {
            if self.phase == 7 {
                return Ok(true);
            }
            let rowwise = self.phase == 0 || self.phase % 2 == 1;
            if self.phase == 0 {
                let begin = self.line * self.width;
                for (index, (pixel, out)) in
                    self.pixels[begin..begin + self.width].iter().zip(&mut self.vertical[begin..begin + self.width]).enumerate()
                {
                    if index % 1024 == 0 {
                        self.cancel.check()?;
                    }
                    if pixel.iter().any(|v| !v.is_finite()) || pixel[..3].iter().any(|v| *v < 0.) || !(0.0..=1.0).contains(&pixel[3]) {
                        return Err(invalid("bloom refuses nonfinite, negative or invalid-coverage radiance"));
                    }
                    let maximum = f64::from(pixel[0].max(pixel[1]).max(pixel[2]));
                    let fraction = if maximum > self.options.threshold { (maximum - self.options.threshold) / maximum } else { 0.0 };
                    *out = std::array::from_fn(|c| (f64::from(pixel[c]) * fraction) as f32);
                }
            } else if rowwise {
                blur_line(&self.vertical, &mut self.horizontal, self.line * self.width, 1, self.width, self.options.radius as usize, &self.cancel)?;
            } else {
                blur_line(&self.horizontal, &mut self.vertical, self.line, self.width, self.height, self.options.radius as usize, &self.cancel)?;
            }
            self.line += 1;
            if self.line == if rowwise { self.height } else { self.width } {
                self.phase += 1;
                self.line = 0;
            }
        }
        Ok(self.phase == 7)
    }
    pub fn finish(self) -> Result<(Vec<[f32; 4]>, Vec<[f32; 3]>)> {
        self.cancel.check()?;
        if self.phase != 7 {
            return Err(invalid("bloom has not completed"));
        }
        Ok((self.pixels, self.vertical))
    }
}

// Full-kernel normalization and zero outside the frame: no artificial border
// renormalization/reflection. Accumulate in f64; intermediates are nonnegative.
fn blur_line(
    source: &[[f32; 3]],
    target: &mut [[f32; 3]],
    begin: usize,
    stride: usize,
    length: usize,
    radius: usize,
    cancel: &CancelToken,
) -> Result<()> {
    let mut sum = [0.0f64; 3];
    for x in 0..length.min(radius + 1) {
        for (s, v) in sum.iter_mut().zip(source[begin + x * stride]) {
            *s += f64::from(v);
        }
    }
    let divisor = (2 * radius + 1) as f64;
    for x in 0..length {
        if x % 1024 == 0 {
            cancel.check()?;
        }
        target[begin + x * stride] = sum.map(|v| (v.max(0.0) / divisor) as f32);
        if x >= radius {
            for (s, v) in sum.iter_mut().zip(source[begin + (x - radius) * stride]) {
                *s -= f64::from(v);
            }
        }
        if let Some(next) = x.checked_add(radius + 1).filter(|next| *next < length) {
            for (s, v) in sum.iter_mut().zip(source[begin + next * stride]) {
                *s += f64::from(v);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(pixels: Vec<[f32; 4]>, width: u32, height: u32, radius: u32, quantum: usize) -> Vec<[f32; 3]> {
        let original = pixels.clone();
        let mut work = BloomTask::new(width, height, pixels, Bloom { strength: 0.5, threshold: 0.0, radius }, CancelToken::default()).unwrap();
        while !work.step(quantum).unwrap() {}
        assert_eq!(work.total_lines(), work.completed_lines());
        let (unchanged, glow) = work.finish().unwrap();
        assert_eq!(original, unchanged);
        glow
    }
    #[test]
    fn box_convolution_matches_independent_direct_kernel_and_work_quanta() {
        let (w, h) = (9, 7);
        let pixels: Vec<_> = (0..w * h).map(|i| [i as f32 % 5., (i * 3) as f32 % 7., 0.25, 1.]).collect();
        let actual = run(pixels.clone(), w, h, 2, 1);
        assert_eq!(actual, run(pixels.clone(), w, h, 2, 32));
        let mut reference: Vec<_> = pixels.iter().map(|v| [v[0] as f64, v[1] as f64, v[2] as f64]).collect();
        for _ in 0..3 {
            let mut next = vec![[0.; 3]; reference.len()];
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    for dy in -2..=2 {
                        for dx in -2..=2 {
                            if (0..w as i32).contains(&(x + dx)) && (0..h as i32).contains(&(y + dy)) {
                                for c in 0..3 {
                                    next[(y * w as i32 + x) as usize][c] += reference[((y + dy) * w as i32 + x + dx) as usize][c] / 25.;
                                }
                            }
                        }
                    }
                }
            }
            reference = next;
        }
        for (a, b) in actual.iter().zip(reference) {
            for (a, b) in a.iter().zip(b) {
                assert!((f64::from(*a) - b).abs() < 2e-6);
            }
        }
    }
    #[test]
    fn interior_impulse_preserves_energy_hue_symmetry_and_scratch_is_explicit() {
        let mut image = vec![[0., 0., 0., 1.]; 31 * 31];
        image[15 * 31 + 15] = [8., 4., 2., 1.];
        let result = run(image, 31, 31, 2, 3);
        let sum: f64 = result.iter().map(|v| f64::from(v[0])).sum();
        assert!((sum - 8.).abs() < 1e-5);
        for y in 0..31 {
            for x in 0..31 {
                let a = result[y * 31 + x];
                assert_eq!(a, result[y * 31 + 30 - x]);
                assert_eq!(a[0] * 0.5, a[1]);
            }
        }
        assert_eq!(Bloom::default().scratch_bytes(3840, 2160).unwrap(), 0);
        assert_eq!(Bloom { strength: 0.5, ..Default::default() }.scratch_bytes(3840, 2160).unwrap(), 3840 * 2160 * 24);
    }
    #[test]
    fn threshold_invalid_inputs_and_cancellation_do_not_publish_a_result() {
        let mut task =
            BloomTask::new(2, 1, vec![[1., 0.5, 0.2, 1.]; 2], Bloom { strength: 1., threshold: 2., radius: 1 }, CancelToken::default()).unwrap();
        while !task.step(1).unwrap() {}
        assert_eq!(task.finish().unwrap().1, vec![[0.; 3]; 2]);
        for strength in [f64::NAN, f64::INFINITY, -1., 2.1] {
            assert!(Bloom { strength, ..Default::default() }.validate().is_err());
        }
        assert!(Bloom { radius: 0, ..Default::default() }.validate().is_err());
        assert!(Bloom { threshold: -1., ..Default::default() }.validate().is_err());
        assert!(BloomTask::new(2, 2, vec![[0.; 4]; 3], Bloom { strength: 1., ..Default::default() }, CancelToken::default()).is_err());
        let cancel = CancelToken::default();
        let mut task = BloomTask::new(2, 2, vec![[1.; 4]; 4], Bloom { strength: 1., ..Default::default() }, cancel.clone()).unwrap();
        cancel.cancel();
        assert!(task.step(1).is_err());
        assert!(task.finish().is_err());
    }
}
