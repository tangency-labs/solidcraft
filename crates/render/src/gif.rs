//! Animated GIF (GIF89a) from RGBA frames, for motion animations. Every frame shares one palette,
//! so a colour keeps its value from frame to frame, and each frame after the first stores only
//! the rectangle that changed. The palette splits the frames' colours where that removes the
//! most squared colour error, so a small, strongly coloured part (a red part, a thin trace line)
//! keeps its colour instead of being averaged into the shading around it. No dithering: what
//! stands still stays still.

use std::collections::HashMap;

/// Colours a GIF palette holds.
const PALETTE: usize = 256;

/// How long each of `frames` frames shows, in the hundredths of a second a GIF counts in, so that
/// they play at `fps` on average. A rate that is not a whole number of hundredths a frame, such
/// as 24 or 30, mixes two lengths (4 and 5 for 24: 24 frames last exactly 1 s), where one rounded
/// length would play at 25 or 33. No frame is shorter than 2, which viewers slow down, nor longer
/// than 1 s. Empty for a rate that is not a number above 0.
pub fn delays(frames: usize, fps: f64) -> Vec<u16> {
    if !fps.is_finite() || fps <= 0.0 {
        return Vec::new();
    }
    let at = |k: usize| (k as f64 * 100.0 / fps).round();
    (0..frames).map(|k| (at(k + 1) - at(k)).clamp(2.0, 100.0) as u16).collect()
}

/// Frames (straight RGBA, `width × height`, rows top to bottom; alpha is ignored) as a GIF that
/// loops forever and shows each frame for its `delays` hundredths of a second ([`delays`]).
/// `None` without frames, for a frame of the wrong size, without a delay for each frame, or a
/// side GIF can't store (above 65535).
pub fn gif(width: usize, height: usize, frames: &[Vec<u8>], delays: &[u16]) -> Option<Vec<u8>> {
    let (w, h) = (u16::try_from(width).ok().filter(|w| *w > 0)?, u16::try_from(height).ok().filter(|h| *h > 0)?);
    let bytes = width.checked_mul(height)?.checked_mul(4)?;
    if frames.is_empty() || frames.len() != delays.len() || frames.iter().any(|f| f.len() != bytes) {
        return None;
    }
    let (palette, index) = palette(frames);
    let mut out = Vec::with_capacity(1024);
    out.extend_from_slice(b"GIF89a");
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    // A global table of 256 colours (8 bits each), background colour 0, square pixels.
    out.extend_from_slice(&[0xF7, 0, 0]);
    for k in 0..PALETTE {
        out.extend_from_slice(palette.get(k).unwrap_or(&[0, 0, 0]));
    }
    // Loop forever (the NETSCAPE2.0 extension every viewer reads).
    out.extend_from_slice(&[0x21, 0xFF, 0x0B]);
    out.extend_from_slice(b"NETSCAPE2.0");
    out.extend_from_slice(&[0x03, 0x01, 0x00, 0x00, 0x00]);
    let mut previous: Option<Vec<u8>> = None;
    for (frame, delay) in frames.iter().zip(delays) {
        let now = indices(frame, &index);
        let (x0, y0, x1, y1) = previous.as_deref().map_or(Some((0, 0, width - 1, height - 1)), |p| changed(p, &now, width)).unwrap_or((0, 0, 0, 0));
        let mut rect = Vec::with_capacity((x1 - x0 + 1) * (y1 - y0 + 1));
        for y in y0..=y1 {
            rect.extend_from_slice(now.get(y * width + x0..=y * width + x1)?);
        }
        // Graphic control: kept on screen under the next frame (disposal 1), shown `delay`.
        out.extend_from_slice(&[0x21, 0xF9, 0x04, 0x04]);
        out.extend_from_slice(&delay.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.push(0x2C);
        for v in [x0, y0, x1 - x0 + 1, y1 - y0 + 1] {
            out.extend_from_slice(&u16::try_from(v).ok()?.to_le_bytes());
        }
        out.push(0);
        out.push(8);
        let data = weezl::encode::Encoder::new(weezl::BitOrder::Lsb, 8).encode(&rect).ok()?;
        for block in data.chunks(255) {
            out.push(block.len() as u8);
            out.extend_from_slice(block);
        }
        out.push(0);
        previous = Some(now);
    }
    out.push(0x3B);
    Some(out)
}

/// The pixels' colours, `0xRRGGBB`.
fn colours(frame: &[u8]) -> impl Iterator<Item = u32> + '_ {
    frame.as_chunks::<4>().0.iter().map(|&[r, g, b, _]| (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
}

/// A frame's palette indices.
fn indices(frame: &[u8], index: &HashMap<u32, u8>) -> Vec<u8> {
    let mut last = (u32::MAX, 0);
    colours(frame)
        .map(|k| {
            if k != last.0 {
                last = (k, index.get(&k).copied().unwrap_or(0));
            }
            last.1
        })
        .collect()
}

/// The smallest rectangle (x0, y0, x1, y1, inclusive) holding every pixel that differs, or `None`
/// when none does.
fn changed(before: &[u8], now: &[u8], width: usize) -> Option<(usize, usize, usize, usize)> {
    let mut r: Option<(usize, usize, usize, usize)> = None;
    for (i, _) in before.iter().zip(now).enumerate().filter(|(_, (a, b))| a != b) {
        let (x, y) = (i % width, i / width);
        r = Some(r.map_or((x, y, x, y), |(x0, y0, x1, y1)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y))));
    }
    r
}

/// How many pixels have a colour, and the sums its mean and squared error follow from.
#[derive(Clone, Copy, Default)]
struct Sums {
    n: f64,
    s: [f64; 3],
    q: f64,
}

impl Sums {
    fn of(rgb: u32, n: u64) -> Sums {
        let n = n as f64;
        let c = [f64::from((rgb >> 16) as u8), f64::from((rgb >> 8) as u8), f64::from(rgb as u8)];
        Sums { n, s: c.map(|v| n * v), q: n * c.iter().map(|v| v * v).sum::<f64>() }
    }
    fn add(&mut self, o: &Sums) {
        self.n += o.n;
        self.q += o.q;
        self.s.iter_mut().zip(o.s).for_each(|(a, b)| *a += b);
    }
    fn minus(&self, o: &Sums) -> Sums {
        let mut s = *self;
        s.n -= o.n;
        s.q -= o.q;
        s.s.iter_mut().zip(o.s).for_each(|(a, b)| *a -= b);
        s
    }
    /// The squared distance of the pixels from their mean colour.
    fn error(&self) -> f64 {
        if self.n <= 0.0 { 0.0 } else { self.q - self.s.iter().map(|s| s * s).sum::<f64>() / self.n }
    }
    fn mean(&self) -> [u8; 3] {
        self.s.map(|s| if self.n > 0.0 { (s / self.n).round().clamp(0.0, 255.0) as u8 } else { 0 })
    }
}

/// A set of the frames' colours that becomes one palette entry, and its best split: (error
/// removed, channel, the largest value of that channel on the low side).
struct Group {
    colours: Vec<(u32, Sums)>,
    sums: Sums,
    split: Option<(f64, u32, u8)>,
}

impl Group {
    fn new(colours: Vec<(u32, Sums)>) -> Group {
        let mut sums = Sums::default();
        colours.iter().for_each(|(_, s)| sums.add(s));
        let mut best: Option<(f64, u32, u8)> = None;
        for channel in [16u32, 8, 0] {
            let mut bins = [Sums::default(); 256];
            for (k, s) in &colours {
                if let Some(b) = bins.get_mut(usize::from((k >> channel) as u8)) {
                    b.add(s);
                }
            }
            let mut low = Sums::default();
            for (v, b) in bins.iter().enumerate() {
                low.add(b);
                if low.n <= 0.0 || low.n >= sums.n {
                    continue;
                }
                let gain = sums.error() - low.error() - sums.minus(&low).error();
                if best.is_none_or(|(g, _, _)| gain > g) {
                    best = Some((gain, channel, v as u8));
                }
            }
        }
        Group { colours, sums, split: best.filter(|(g, _, _)| *g > 0.0) }
    }
}

/// One palette for all frames (up to 256 colours) and each colour's index in it. Every colour
/// gets its own entry while there are 256 or fewer.
fn palette(frames: &[Vec<u8>]) -> (Vec<[u8; 3]>, HashMap<u32, u8>) {
    let mut counts: HashMap<u32, u64> = HashMap::new();
    for frame in frames {
        // Runs of one colour (the background) are counted once per run.
        let mut run = (u32::MAX, 0u64);
        for k in colours(frame) {
            if k != run.0 {
                if run.1 > 0 {
                    *counts.entry(run.0).or_default() += run.1;
                }
                run = (k, 0);
            }
            run.1 += 1;
        }
        if run.1 > 0 {
            *counts.entry(run.0).or_default() += run.1;
        }
    }
    let mut groups = vec![Group::new(counts.into_iter().map(|(k, n)| (k, Sums::of(k, n))).collect())];
    while groups.len() < PALETTE {
        let Some(i) = groups.iter().enumerate().filter_map(|(i, g)| g.split.map(|s| (i, s.0))).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
        else {
            break;
        };
        let g = groups.swap_remove(i);
        let Some((_, channel, at)) = g.split else { break };
        let (low, high) = g.colours.into_iter().partition(|(k, _)| (k >> channel) as u8 <= at);
        groups.push(Group::new(low));
        groups.push(Group::new(high));
    }
    let mut index = HashMap::new();
    for (i, g) in groups.iter().enumerate() {
        for (k, _) in &g.colours {
            index.insert(*k, i as u8);
        }
    }
    (groups.iter().map(|g| g.sums.mean()).collect(), index)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frames of a GIF this module wrote, as RGBA (each frame drawn over the one before).
    fn decode(gif: &[u8]) -> (usize, usize, Vec<Vec<u8>>, Vec<u16>) {
        let u16_at = |i: usize| usize::from(u16::from_le_bytes([gif[i], gif[i + 1]]));
        assert_eq!(&gif[..6], b"GIF89a");
        let (w, h) = (u16_at(6), u16_at(8));
        let table = &gif[13..13 + 768];
        let mut at = 13 + 768;
        let (mut frames, mut delays) = (Vec::new(), Vec::new());
        let mut canvas = vec![0u8; w * h * 4];
        loop {
            match gif[at] {
                0x3B => break,
                0x21 => {
                    if gif[at + 1] == 0xF9 {
                        delays.push(u16_at(at + 4) as u16);
                    }
                    at += 2;
                    while gif[at] != 0 {
                        at += usize::from(gif[at]) + 1;
                    }
                    at += 1;
                }
                0x2C => {
                    let (x0, y0, fw, fh) = (u16_at(at + 1), u16_at(at + 3), u16_at(at + 5), u16_at(at + 7));
                    at += 11;
                    let mut data = Vec::new();
                    while gif[at] != 0 {
                        data.extend_from_slice(&gif[at + 1..at + 1 + usize::from(gif[at])]);
                        at += usize::from(gif[at]) + 1;
                    }
                    at += 1;
                    let pixels = weezl::decode::Decoder::new(weezl::BitOrder::Lsb, 8).decode(&data).unwrap();
                    assert_eq!(pixels.len(), fw * fh);
                    for (i, p) in pixels.iter().enumerate() {
                        let o = ((y0 + i / fw) * w + x0 + i % fw) * 4;
                        let c = usize::from(*p) * 3;
                        canvas[o..o + 4].copy_from_slice(&[table[c], table[c + 1], table[c + 2], 255]);
                    }
                    frames.push(canvas.clone());
                }
                b => panic!("unexpected block {b:#x} at {at}"),
            }
        }
        (w, h, frames, delays)
    }

    fn frame(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
        (0..w * h).flat_map(|i| f(i % w, i / w).into_iter().chain([255])).collect()
    }

    #[test]
    fn few_colours_come_back_exactly_and_frames_keep_their_size() {
        let (w, h) = (37, 23);
        let a = frame(w, h, |x, y| {
            if x < 10 {
                [250, 30, 30]
            } else if y > 15 {
                [20, 200, 60]
            } else {
                [220, 224, 230]
            }
        });
        // The second frame moves the red band: only that part changes.
        let b = frame(w, h, |x, y| {
            if (5..15).contains(&x) {
                [250, 30, 30]
            } else if y > 15 {
                [20, 200, 60]
            } else {
                [220, 224, 230]
            }
        });
        let bytes = gif(w, h, &[a.clone(), b.clone(), b.clone()], &[5, 5, 5]).unwrap();
        let (dw, dh, frames, delays) = decode(&bytes);
        assert_eq!((dw, dh, delays), (w, h, vec![5, 5, 5]));
        assert_eq!(frames, vec![a, b.clone(), b]);
    }

    #[test]
    fn a_thin_saturated_line_keeps_its_colour_among_many_shades() {
        let (w, h) = (300, 200);
        // A grey shading with far more than 256 shades, a red part and a 1-pixel green line.
        let f = frame(w, h, |x, y| {
            if y == 100 {
                [30, 220, 40]
            } else if (40..80).contains(&x) && (40..80).contains(&y) {
                [200 + (y as u8 % 40), 30, 30]
            } else {
                let g = 60 + ((x * 180) / w) as u8;
                [g, g.saturating_add((y / 4) as u8), g.saturating_add(10)]
            }
        });
        let (_, _, frames, _) = decode(&gif(w, h, std::slice::from_ref(&f), &[4]).unwrap());
        let px = |img: &[u8], x: usize, y: usize| img[(y * w + x) * 4..(y * w + x) * 4 + 3].to_vec();
        assert_eq!(px(&frames[0], 150, 100), vec![30, 220, 40], "the line keeps its colour");
        let red = px(&frames[0], 60, 60);
        assert!(red[0] > 180 && red[1] < 60 && red[2] < 60, "the part stays red: {red:?}");
        // Elsewhere each channel is close to the original.
        let worst = f.chunks(4).zip(frames[0].chunks(4)).map(|(a, b)| (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap()).max().unwrap();
        assert!(worst <= 12, "worst channel error {worst}");
    }

    #[test]
    fn bad_input_is_none_not_a_crash() {
        assert!(gif(4, 4, &[], &[]).is_none());
        assert!(gif(4, 4, &[vec![0; 63]], &[5]).is_none());
        assert!(gif(0, 4, &[vec![]], &[5]).is_none());
        assert!(gif(70_000, 1, &[vec![0; 280_000]], &[5]).is_none());
        assert!(gif(usize::MAX, 2, &[vec![0; 8]], &[5]).is_none());
        // A delay missing, or one too many.
        assert!(gif(4, 4, &[vec![0; 64], vec![0; 64]], &[5]).is_none());
        assert!(gif(4, 4, &[vec![0; 64]], &[5, 5]).is_none());
        for fps in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(delays(3, fps).is_empty(), "{fps}");
        }
    }

    /// A GIF counts in hundredths of a second: 30 or 24 frames a second can't be one length.
    /// Rounded to one, they played at 33 and 25; mixed, a second of frames lasts a second.
    #[test]
    fn frames_play_at_the_rate_asked_for_on_average() {
        assert_eq!(delays(6, 30.0), vec![3, 4, 3, 3, 4, 3]);
        assert_eq!(delays(6, 24.0), vec![4, 4, 5, 4, 4, 4]);
        for fps in [1.0_f64, 7.0, 12.0, 20.0, 24.0, 25.0, 29.97, 30.0, 33.0, 45.0, 50.0] {
            let d = delays(fps.round() as usize * 3, fps);
            let total: u32 = d.iter().map(|&x| u32::from(x)).sum();
            assert!((f64::from(total) - (d.len() as f64 * 100.0 / fps)).abs() <= 0.5, "{fps}: {d:?}");
            assert!(d.iter().all(|x| (2..=100).contains(x)), "{fps}: {d:?}");
        }
        // Each frame is written with its own delay.
        let f = vec![0u8; 4 * 4 * 4];
        let (_, _, _, written) = decode(&gif(4, 4, &[f.clone(), f.clone(), f], &[3, 4, 3]).unwrap());
        assert_eq!(written, vec![3, 4, 3]);
    }
}
