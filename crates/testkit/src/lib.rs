//! Deterministic mutations of valid files, and the readers run over them (#469). Every reader of
//! bytes the user didn't write gets the same treatment: truncation, flipped and overwritten
//! bytes, dropped and repeated runs, appended garbage and, for text formats, dropped and
//! repeated lines and numbers that lie (`nan`, `inf`, `1e400`, …). Each mutated file must give an
//! error or a result, never a panic, within a time budget and the reader's own caps. A panic a
//! reader catches itself (`solidcraft_kernel::guard`) counts too: the browser build aborts on any
//! panic, so a caught one on the desktop is a crash there.
//!
//! The mutations come from a fixed seed (xorshift), so a failure names a case that reproduces.
//! No fuzzing dependency, nothing nightly, nothing native: it runs in the normal test suite and
//! builds for wasm. The readers' own hand-written cases stay; they cover particular lies (a
//! length in a container header) that random changes rarely hit. STEP, IGES and FreeCAD/Rhino
//! files are read by the kernel, which is moving to solidcore (#1597), and keep their own tests.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

/// A deterministic xorshift64 stream.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A value in `0..n` (0 when `n` is 0).
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

/// Numbers a hostile file might hold in place of a real one.
const LIARS: [&str; 10] = ["nan", "inf", "-inf", "1e400", "-1e400", "-0", "4294967296", "-1", "99999999999999999999", "0x7fffffff"];

/// Most truncations of one file (every length up to this, evenly spaced beyond).
const MAX_TRUNCATIONS: usize = 1024;

/// Mutations of `good`, the same for the same `good` and `seed`. `text`: also line and number
/// mutations (the file is UTF-8 text).
pub fn mutations(good: &[u8], text: bool, seed: u64) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let n = good.len();
    let mut rng = Rng::new(seed);
    // Truncations: every length of a short file; of a long one, its first and last 64 lengths
    // (headers, final records) and evenly spaced ones between.
    if n <= MAX_TRUNCATIONS {
        out.extend((0..n).map(|k| good[..k].to_vec()));
    } else {
        let mut cuts: Vec<usize> = (0..64).chain(n - 64..n).collect();
        cuts.extend((0..MAX_TRUNCATIONS - 128).map(|i| 64 + i * (n - 128) / (MAX_TRUNCATIONS - 128)));
        cuts.sort_unstable();
        cuts.dedup();
        out.extend(cuts.into_iter().map(|k| good[..k].to_vec()));
    }
    if n == 0 {
        return out;
    }
    // Bytes flipped, zeroed and saturated at seeded places.
    for i in 0..384 {
        let at = rng.below(n);
        let mut b = good.to_vec();
        b[at] = match i % 3 {
            0 => b[at] ^ ((rng.next() >> 32) as u8 | 1),
            1 => 0x00,
            _ => 0xff,
        };
        out.push(b);
    }
    // Runs of bytes dropped, repeated and swapped with their neighbour.
    for _ in 0..96 {
        let at = rng.below(n);
        let len = 1 + rng.below(64.min(n - at));
        let end = at + len;
        let mut dropped = good[..at].to_vec();
        dropped.extend_from_slice(&good[end..]);
        out.push(dropped);
        let mut repeated = good[..end].to_vec();
        repeated.extend_from_slice(&good[at..]);
        out.push(repeated);
        if end + len <= n {
            let mut swapped = good[..at].to_vec();
            swapped.extend_from_slice(&good[end..end + len]);
            swapped.extend_from_slice(&good[at..end]);
            swapped.extend_from_slice(&good[end + len..]);
            out.push(swapped);
        }
    }
    // The whole file: with garbage after it, twice over, reversed.
    let mut tail = good.to_vec();
    tail.extend((0..4096).map(|_| (rng.next() >> 40) as u8));
    out.push(tail);
    out.push([good, good].concat());
    out.push(good.iter().rev().copied().collect());
    if text && let Ok(s) = std::str::from_utf8(good) {
        out.extend(text_mutations(s, &mut rng));
    }
    out
}

/// A text file's lines dropped, repeated and swapped, and its numbers replaced by lies.
fn text_mutations(s: &str, rng: &mut Rng) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let lines: Vec<&str> = s.split_inclusive('\n').collect();
    let join = |v: &[&str]| v.concat().into_bytes();
    let picked: Vec<usize> = if lines.len() <= 200 { (0..lines.len()).collect() } else { (0..200).map(|_| rng.below(lines.len())).collect() };
    for &i in &picked {
        let mut v = lines.clone();
        v.remove(i);
        out.push(join(&v));
        let mut v = lines.clone();
        v.insert(i, lines[i]);
        out.push(join(&v));
        if i + 1 < lines.len() {
            let mut v = lines.clone();
            v.swap(i, i + 1);
            out.push(join(&v));
        }
    }
    // A line in place of another (a block that inserts itself, a count where a coordinate was).
    for _ in 0..200.min(lines.len() * lines.len()) {
        let (i, j) = (rng.below(lines.len()), rng.below(lines.len()));
        let mut v = lines.clone();
        v[i] = lines[j];
        out.push(join(&v));
    }
    // A word in place of another of the file's own words: a renamed block, a swapped keyword, a
    // block that now inserts itself. Every pair in a small file, a sample in a large one.
    let words = word_spans(s);
    let mut vocab: Vec<&str> = words.iter().map(|&(a, b)| &s[a..b]).collect();
    vocab.sort_unstable();
    vocab.dedup();
    let pairs = words.len().saturating_mul(vocab.len());
    let swap = |(a, b): (usize, usize), w: &str| format!("{}{w}{}", &s[..a], &s[b..]).into_bytes();
    if pairs <= 3000 {
        for &span in &words {
            for &w in vocab.iter().filter(|w| **w != &s[span.0..span.1]) {
                out.push(swap(span, w));
            }
        }
    } else {
        for _ in 0..400 {
            out.push(swap(words[rng.below(words.len())], vocab[rng.below(vocab.len())]));
        }
    }
    // Numbers: each of up to 200 of them, in turn, becomes one of the liars.
    let numbers = number_spans(s);
    let chosen: Vec<(usize, usize)> = if numbers.len() <= 200 { numbers } else { (0..200).map(|_| numbers[rng.below(numbers.len())]).collect() };
    for (k, (a, b)) in chosen.into_iter().enumerate() {
        let liar = LIARS[k % LIARS.len()];
        out.push(format!("{}{liar}{}", &s[..a], &s[b..]).into_bytes());
    }
    // Every number lying at once (two counts that are only harmless one at a time).
    let all = number_spans(s);
    for liar in LIARS {
        let mut t = String::with_capacity(s.len());
        let mut at = 0;
        for &(a, b) in &all {
            t.push_str(&s[at..a]);
            t.push_str(liar);
            at = b;
        }
        t.push_str(&s[at..]);
        out.push(t.into_bytes());
    }
    out
}

/// Byte spans of the words in `s` (runs of ASCII letters, digits and `_` that start with a
/// letter).
fn word_spans(s: &str) -> Vec<(usize, usize)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_alphabetic() || (i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')) {
            i += 1;
            continue;
        }
        let a = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
            i += 1;
        }
        out.push((a, i));
    }
    out
}

/// Byte spans of the decimal numbers in `s` (a sign, digits, a point, an exponent).
fn number_spans(s: &str) -> Vec<(usize, usize)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let starts = b[i].is_ascii_digit() || ((b[i] == b'-' || b[i] == b'.') && b.get(i + 1).is_some_and(u8::is_ascii_digit));
        let after_word = i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        if !starts || after_word {
            i += 1;
            continue;
        }
        let a = i;
        i += 1;
        while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.' || ((b[i] == b'e' || b[i] == b'E') && i + 1 < b.len())) {
            if (b[i] == b'e' || b[i] == b'E') && (b[i + 1] == b'-' || b[i + 1] == b'+') {
                i += 1;
            }
            i += 1;
        }
        out.push((a, i));
    }
    out
}

thread_local! {
    /// Panics on this thread so far, caught or not ([`count_panics`]).
    static PANICS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Count every panic per thread (tests run on several), then run the hook that was there.
fn count_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            PANICS.with(|p| p.set(p.get() + 1));
            previous(info);
        }));
    });
}

/// The time any one reader call may take on a mutated file (a debug build on a loaded machine:
/// generous, but far below what unbounded work on a lying count would take).
pub const BUDGET: Duration = Duration::from_secs(2);

/// Run `read` over `inputs`: no panic, within [`BUDGET`], and `read`'s count (triangles,
/// entities, …) at most `cap`. `read` returns `None` for an error. Panics (the test fails) naming
/// the first case that breaks a rule.
// Failing the calling test is this function's job; it only ever runs inside tests.
#[allow(clippy::panic)]
pub fn survives(what: &str, inputs: &[Vec<u8>], cap: usize, mut read: impl FnMut(&[u8]) -> Option<usize>) {
    count_panics();
    let mut slowest = (Duration::ZERO, 0);
    let mut read_ok = 0;
    for (i, input) in inputs.iter().enumerate() {
        let before = PANICS.with(std::cell::Cell::get);
        let t = Instant::now();
        let r = catch_unwind(AssertUnwindSafe(|| read(input)));
        let took = t.elapsed();
        let head = String::from_utf8_lossy(&input[..input.len().min(120)]).into_owned();
        let Ok(r) = r else { panic!("{what}: case {i} ({} bytes) panicked; it starts {head:?}", input.len()) };
        let caught = PANICS.with(std::cell::Cell::get) - before;
        assert!(
            caught == 0,
            "{what}: case {i} ({} bytes) panicked inside the reader (caught there, but the browser aborts); it starts {head:?}",
            input.len()
        );
        assert!(took <= BUDGET, "{what}: case {i} ({} bytes) took {took:?} (budget {BUDGET:?}); it starts {head:?}", input.len());
        if let Some(count) = r {
            assert!(count <= cap, "{what}: case {i} gave {count} (cap {cap})");
            read_ok += 1;
        }
        slowest = slowest.max((took, i));
    }
    eprintln!("{what}: {} mutations, {read_ok} read, slowest case {} in {:?}", inputs.len(), slowest.1, slowest.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutations_are_deterministic_and_cover_the_file() {
        let good = b"solid x\n facet normal 0 0 1\n  vertex 1.5 -2 3e2\n endfacet\nendsolid x\n";
        let a = mutations(good, true, 7);
        assert_eq!(a, mutations(good, true, 7), "the same seed gives the same cases");
        assert_ne!(a, mutations(good, true, 8));
        assert!(a.iter().any(Vec::is_empty) && a.iter().any(|m| m.len() == good.len() - 1), "truncations down to nothing");
        assert!(a.iter().any(|m| m.windows(3).any(|w| w == b"nan")), "a number replaced by a liar");
        assert_eq!(number_spans("v 1.5 -2 3e2 x1 .5e-3"), vec![(2, 5), (6, 8), (9, 12), (16, 21)]);
        // A long file: bounded truncations, kept to the first and last bytes and spaced between.
        let long = vec![b'a'; 100_000];
        let m = mutations(&long, false, 1);
        assert!(m.len() < 3000, "{} cases", m.len());
    }
}
