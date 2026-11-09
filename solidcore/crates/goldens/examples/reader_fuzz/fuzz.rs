//! Unix implementation of the `reader_fuzz` example (see `../reader_fuzz.rs`).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use solidcore::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::topology::brep::{p10, p7};
use solidcore::topology::brep_data::{AttributeSink, BrepData};
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::topology::Brep;
use solidcore::{EntityRef, Kernel, SolidError};

// ---- deterministic RNG ------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    /// Below `big` one time in three, else below `small`.
    fn below_mixed(&mut self, big: usize, small: usize) -> usize {
        let n = if self.below(3) == 0 { big } else { small };
        self.below(n)
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> Option<&'a T> {
        let i = self.below(xs.len());
        xs.get(i)
    }
}

fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ---- mutations --------------------------------------------------------------------------

const INT_VALUES: &[&str] = &[
    "0",
    "1",
    "2",
    "3",
    "-1",
    "-2",
    "7",
    "64",
    "65",
    "255",
    "256",
    "65535",
    "65536",
    "1000000",
    "2147483647",
    "2147483648",
    "4294967295",
    "4294967296",
    "9223372036854775807",
    "9223372036854775808",
    "18446744073709551615",
    "18446744073709551616",
    "-9223372036854775808",
    "99999999999999999999999999",
];

const FLOAT_VALUES: &[&str] = &[
    "0", "-0", "1", "-1", "0.5", "1e308", "-1e308", "1e-308", "4.9e-324", "1e400", "-1e400", "nan",
    "inf", "-inf", "1e10", "1e-10", "1e-300", "1e300", "2", "3.0e5",
];

/// Type codes for `Object Type` / `Curve Type` / `Surface Type` headers.
const TYPE_CODES: &[i64] = &[
    0, 1, -1, 14000, 14001, 14010, 14020, 14021, 14022, 14023, 14024, 14032, 14033, 14034, 14036,
    14037, 14045, 14048, 14050, 14053, 14999, 15000, 15001, 15002, 15011, 15012, 15013, 15014,
    15015, 15020, 15030, 15045, 15218, 15400, 15410, 16016, 16005, 17000,
];

const FILL_VALUES: &[&str] = &[
    "0",
    "1",
    "2",
    "3",
    "4",
    "-1",
    "0.5",
    "1.0",
    "-0.5",
    "10",
    "1e300",
    "nan",
    "1000000",
    "18446744073709551615",
    "3.14159",
    "0.0001",
];

const NEST_DEPTHS: &[usize] = &[2, 8, 32, 63, 64, 65, 100, 1000, 20000];

#[derive(Clone, Copy)]
struct Tok {
    start: usize,
    end: usize,
    is_int: bool,
    line_start: bool,
}

fn is_num_char(c: u8) -> bool {
    c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.' | b'e' | b'E')
}

/// Numeric tokens of an ASCII file (`[-+]?digits[.digits][e[-+]digits]`, delimited
/// by whitespace or line ends).
fn tokens(d: &[u8]) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = 0;
    let n = d.len();
    while i < n {
        let prev_ws = i == 0 || d.get(i - 1).is_some_and(u8::is_ascii_whitespace);
        let c = d.get(i).copied().unwrap_or(b' ');
        if prev_ws && (c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.') {
            let mut j = i;
            while j < n && d.get(j).copied().is_some_and(is_num_char) {
                j += 1;
            }
            let end_ok = j == n || d.get(j).is_some_and(u8::is_ascii_whitespace);
            let s = d.get(i..j).unwrap_or(&[]);
            if end_ok && s.iter().any(u8::is_ascii_digit) {
                let is_int = !s.iter().any(|&c| matches!(c, b'.' | b'e' | b'E'));
                let line_start = i == 0 || d.get(i - 1) == Some(&b'\n');
                out.push(Tok {
                    start: i,
                    end: j,
                    is_int,
                    line_start,
                });
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

/// Byte offsets of line starts.
fn line_starts(d: &[u8]) -> Vec<usize> {
    let mut v = vec![0];
    for (i, &c) in d.iter().enumerate() {
        if c == b'\n' && i + 1 < d.len() {
            v.push(i + 1);
        }
    }
    v
}

fn line_range(d: &[u8], starts: &[usize], first: usize, count: usize) -> (usize, usize) {
    let a = starts.get(first).copied().unwrap_or(d.len());
    let b = starts.get(first + count).copied().unwrap_or(d.len());
    (a, b.max(a))
}

fn splice(d: &mut Vec<u8>, a: usize, b: usize, with: &[u8]) {
    let a = a.min(d.len());
    let b = b.clamp(a, d.len());
    d.splice(a..b, with.iter().copied());
}

/// Positions of `<word> Type <code>` headers: (byte offset of the code token, end).
fn type_headers(d: &[u8]) -> Vec<(usize, usize, bool)> {
    let mut out = Vec::new();
    for s in line_starts(d) {
        let line = d.get(s..).unwrap_or(&[]);
        let line = line
            .get(..line.iter().position(|&c| c == b'\n').unwrap_or(line.len()))
            .unwrap_or(&[]);
        for (prefix, curve) in [
            (&b"Object Type "[..], None),
            (&b"Curve Type "[..], Some(true)),
            (&b"Surface Type "[..], Some(false)),
        ] {
            if line.starts_with(prefix) {
                let a = s + prefix.len();
                let mut b = a;
                while d.get(b).is_some_and(|c| c.is_ascii_digit() || *c == b'-') {
                    b += 1;
                }
                // `Object Type` is a curve when `Curve Dim` follows
                let is_curve = curve.unwrap_or_else(|| {
                    let rest = d.get(b..).unwrap_or(&[]);
                    let nl = rest
                        .iter()
                        .position(|&c| c == b'\n')
                        .map_or(rest.len(), |p| p + 1);
                    rest.get(nl..).unwrap_or(&[]).starts_with(b"Curve Dim")
                });
                out.push((a, b, is_curve));
            }
        }
    }
    out
}

fn mutate_ascii(d: &mut Vec<u8>, rng: &mut Rng, desc: &mut String) {
    let kind = rng.below(120);
    match kind {
        // bit flips
        0..=7 => {
            let p = rng.below(d.len());
            let bit = rng.below(8);
            if let Some(c) = d.get_mut(p) {
                *c ^= 1 << bit;
            }
            let _ = write!(desc, "bitflip@{p}.{bit}; ");
        }
        // byte overwrite
        8..=14 => {
            let p = rng.below(d.len());
            let set: &[u8] = b"0123456789-+.e \n\t\0\xffV/x";
            let v = if rng.below(4) == 0 {
                rng.next() as u8
            } else {
                rng.pick(set).copied().unwrap_or(b'0')
            };
            if let Some(c) = d.get_mut(p) {
                *c = v;
            }
            let _ = write!(desc, "byte@{p}={v:#04x}; ");
        }
        // truncation
        15..=19 => {
            let p = if rng.below(2) == 0 {
                let ls = line_starts(d);
                rng.pick(&ls).copied().unwrap_or(0)
            } else {
                rng.below(d.len() + 1)
            };
            d.truncate(p);
            let _ = write!(desc, "truncate@{p}; ");
        }
        // integer token (counts, indices, flags)
        20..=44 => {
            let toks = tokens(d);
            let pref: Vec<Tok> = if rng.below(2) == 0 {
                toks.iter()
                    .copied()
                    .filter(|t| t.is_int && t.line_start)
                    .collect()
            } else {
                toks.iter().copied().filter(|t| t.is_int).collect()
            };
            let Some(t) = rng.pick(&pref).copied() else {
                return;
            };
            let old = String::from_utf8_lossy(d.get(t.start..t.end).unwrap_or(&[])).into_owned();
            let new = match rng.below(4) {
                0 => {
                    let v: i128 = old.parse().unwrap_or(0);
                    let dv = [-2i128, -1, 1, 2, 1000];
                    (v + rng.pick(&dv).copied().unwrap_or(1)).to_string()
                }
                1 => {
                    let v: i128 = old.parse().unwrap_or(0);
                    (v.saturating_mul(2) + 1).to_string()
                }
                _ => (*rng.pick(INT_VALUES).unwrap_or(&"0")).to_string(),
            };
            let _ = write!(desc, "int@{}:{old}->{new}; ", t.start);
            splice(d, t.start, t.end, new.as_bytes());
        }
        // float token
        45..=56 => {
            let toks: Vec<Tok> = tokens(d).into_iter().filter(|t| !t.is_int).collect();
            let Some(t) = rng.pick(&toks).copied() else {
                return;
            };
            let old = String::from_utf8_lossy(d.get(t.start..t.end).unwrap_or(&[])).into_owned();
            let new = if rng.below(4) == 0 {
                format!("-{}", old.trim_start_matches('-'))
            } else {
                (*rng.pick(FLOAT_VALUES).unwrap_or(&"0")).to_string()
            };
            let _ = write!(desc, "float@{}:{old}->{new}; ", t.start);
            splice(d, t.start, t.end, new.as_bytes());
        }
        // delete lines
        57..=63 => {
            let ls = line_starts(d);
            let first = rng.below(ls.len());
            let count = 1 + rng.below_mixed(40, 3);
            let (a, b) = line_range(d, &ls, first, count);
            splice(d, a, b, &[]);
            let _ = write!(desc, "dellines@{first}+{count}; ");
        }
        // duplicate lines in place
        64..=70 => {
            let ls = line_starts(d);
            let first = rng.below(ls.len());
            let count = 1 + rng.below_mixed(40, 3);
            let times = 1 + rng.below(3);
            let (a, b) = line_range(d, &ls, first, count);
            let chunk = d.get(a..b).unwrap_or(&[]).to_vec();
            let mut ins = Vec::new();
            for _ in 0..times {
                ins.extend_from_slice(&chunk);
            }
            splice(d, b, b, &ins);
            let _ = write!(desc, "duplines@{first}+{count}x{times}; ");
        }
        // swap two lines / move a block elsewhere
        71..=76 => {
            let ls = line_starts(d);
            let first = rng.below(ls.len());
            let count = 1 + rng.below(20);
            let (a, b) = line_range(d, &ls, first, count);
            let chunk = d.get(a..b).unwrap_or(&[]).to_vec();
            let to = rng.pick(&ls).copied().unwrap_or(0);
            let keep = rng.below(2) == 0;
            if keep {
                splice(d, to, to, &chunk);
            } else if to >= b {
                splice(d, to, to, &chunk);
                splice(d, a, b, &[]);
            } else if to <= a {
                splice(d, a, b, &[]);
                splice(d, to, to, &chunk);
            }
            let _ = write!(
                desc,
                "{}lines@{first}+{count}->{to}; ",
                if keep { "copy" } else { "move" }
            );
        }
        // type code swap
        77..=86 => {
            let hs = type_headers(d);
            let Some(&(a, b, _)) = rng.pick(&hs) else {
                return;
            };
            let code = rng.pick(TYPE_CODES).copied().unwrap_or(0);
            let old = String::from_utf8_lossy(d.get(a..b).unwrap_or(&[])).into_owned();
            let _ = write!(desc, "type@{a}:{old}->{code}; ");
            splice(d, a, b, code.to_string().as_bytes());
        }
        // nesting increase: wrap a curve or surface record in N wrappers
        87..=91 => {
            let hs = type_headers(d);
            let Some(&(a, b, is_curve)) = rng.pick(&hs) else {
                return;
            };
            // back to the start of the header line
            let line_a = d
                .get(..a)
                .unwrap_or(&[])
                .iter()
                .rposition(|&c| c == b'\n')
                .map_or(0, |p| p + 1);
            let header = String::from_utf8_lossy(d.get(line_a..a).unwrap_or(&[])).into_owned();
            let depth = rng.pick(NEST_DEPTHS).copied().unwrap_or(2);
            let old_style = header.starts_with("Curve Type") || header.starts_with("Surface Type");
            let wrapper = match (is_curve, old_style) {
                // v<=32: an old composite curve of one segment
                (true, true) => "Curve Type 14032\n1 3 0\n1 0 0\n".to_string(),
                // v>32: an offset curve (normal, distance, interval, reversed, comment)
                (true, false) => {
                    "Object Type 14036\nCurve Dim 3\n0 0 1\n0.1\n0 1\n0\n//base\n".to_string()
                }
                // v<=32: offset surfaces only wrap B-splines (no recursion); still useful
                (false, true) => "Surface Type 15400\n0.1\n".to_string(),
                // v>32: offset surface (distance, 4 flags, comment)
                (false, false) => "Object Type 15400\n0.1\n0\n0\n0\n0\n//base\n".to_string(),
            };
            let _ = b;
            let ins = wrapper.repeat(depth);
            splice(d, line_a, line_a, ins.as_bytes());
            let _ = write!(
                desc,
                "nest@{line_a}:{}x{depth}; ",
                if is_curve { "curve" } else { "surface" }
            );
        }
        // type swap plus numeric filler: reach the readers of curve and surface kinds
        // the corpus does not contain (each reader takes what it needs from the filler)
        100..=119 => {
            let hs = type_headers(d);
            let Some(&(a, b, is_curve)) = rng.pick(&hs) else {
                return;
            };
            let curve_codes = [
                14010, 14020, 14021, 14022, 14023, 14024, 14032, 14033, 14034, 14036, 14037, 14045,
                14048, 14050, 14053,
            ];
            let surface_codes = [
                15002, 15011, 15012, 15013, 15014, 15015, 15020, 15030, 15045, 15218, 15400, 15410,
            ];
            let code = if is_curve {
                rng.pick(&curve_codes)
            } else {
                rng.pick(&surface_codes)
            }
            .copied()
            .unwrap_or(14010);
            let lines = 1 + rng.below(40);
            let mut fill = String::from("\n");
            for _ in 0..lines {
                // now and then a nested object header (wrapper kinds read one)
                if rng.below(6) == 0 {
                    if rng.below(2) == 0 {
                        let c = rng.pick(&curve_codes).copied().unwrap_or(14010);
                        let _ = write!(fill, "Object Type {c}\nCurve Dim {}\n", 2 + rng.below(2));
                    } else {
                        let c = rng.pick(&surface_codes).copied().unwrap_or(15002);
                        let _ = writeln!(fill, "Object Type {c}");
                    }
                    continue;
                }
                let n = 1 + rng.below(4);
                for j in 0..n {
                    if j > 0 {
                        fill.push(' ');
                    }
                    fill.push_str(rng.pick(FILL_VALUES).copied().unwrap_or("0"));
                }
                fill.push('\n');
            }
            // the filler goes after the header line (and its `Curve Dim` line)
            let rest = d.get(b..).unwrap_or(&[]);
            let mut at = b + rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len());
            if is_curve && d.get(at + 1..).is_some_and(|r| r.starts_with(b"Curve Dim")) {
                let rest = d.get(at + 1..).unwrap_or(&[]);
                at = at + 1 + rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len());
            }
            let fill = fill.trim_end_matches('\n').to_string();
            splice(d, at, at, fill.as_bytes());
            splice(d, a, b, code.to_string().as_bytes());
            let _ = write!(desc, "typefill@{a}:{code}+{lines}lines; ");
        }
        // splice random bytes / garbage line
        92..=95 => {
            let p = rng.below(d.len() + 1);
            let n = 1 + rng.below(64);
            let g: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
            splice(d, p, p, &g);
            let _ = write!(desc, "garbage@{p}+{n}; ");
        }
        // delete or duplicate a whole section (from a `//` title line to the next)
        _ => {
            let ls = line_starts(d);
            let titles: Vec<usize> = ls
                .iter()
                .enumerate()
                .filter(|(_, &s)| {
                    d.get(s..)
                        .is_some_and(|r| r.starts_with(b"//") || r.starts_with(b"\n"))
                })
                .map(|(i, _)| i)
                .collect();
            let Some(&ti) = rng.pick(&titles) else {
                return;
            };
            let next = titles
                .iter()
                .copied()
                .find(|&j| j > ti + 1)
                .unwrap_or(ls.len());
            let (a, b) = line_range(d, &ls, ti, next - ti);
            if rng.below(2) == 0 {
                splice(d, a, b, &[]);
                let _ = write!(desc, "delsection@{ti}..{next}; ");
            } else {
                let chunk = d.get(a..b).unwrap_or(&[]).to_vec();
                splice(d, b, b, &chunk);
                let _ = write!(desc, "dupsection@{ti}..{next}; ");
            }
        }
    }
}

fn mutate_binary(d: &mut Vec<u8>, rng: &mut Rng, desc: &mut String) {
    let kind = rng.below(120);
    match kind {
        0..=14 => {
            let p = rng.below(d.len());
            let bit = rng.below(8);
            if let Some(c) = d.get_mut(p) {
                *c ^= 1 << bit;
            }
            let _ = write!(desc, "bitflip@{p}.{bit}; ");
        }
        15..=24 => {
            let p = rng.below(d.len());
            let v = rng.next() as u8;
            if let Some(c) = d.get_mut(p) {
                *c = v;
            }
            let _ = write!(desc, "byte@{p}={v:#04x}; ");
        }
        25..=31 => {
            let p = rng.below(d.len() + 1);
            d.truncate(p);
            let _ = write!(desc, "truncate@{p}; ");
        }
        // 8-byte integer or double overwrite at any offset
        32..=64 => {
            let p = rng.below(d.len());
            let ints: [u64; 14] = [
                0,
                1,
                2,
                3,
                u64::MAX,
                u64::MAX - 1,
                1 << 31,
                1 << 32,
                (1 << 32) - 1,
                i64::MAX as u64,
                1 << 63,
                1 << 20,
                1000,
                65,
            ];
            let v = if rng.below(4) == 0 {
                let f = [
                    f64::NAN,
                    f64::INFINITY,
                    -f64::INFINITY,
                    1e308,
                    -1e308,
                    0.0,
                    -0.0,
                    5e-324,
                ];
                rng.pick(&f).copied().unwrap_or(0.0).to_bits()
            } else if rng.below(3) == 0 {
                // a small delta on the existing value
                let mut old = [0u8; 8];
                for (i, b) in old.iter_mut().enumerate() {
                    *b = d.get(p + i).copied().unwrap_or(0);
                }
                u64::from_le_bytes(old).wrapping_add(
                    [1u64, u64::MAX, 2, 8]
                        .get(rng.below(4))
                        .copied()
                        .unwrap_or(1),
                )
            } else {
                rng.pick(&ints).copied().unwrap_or(0)
            };
            for (i, b) in v.to_le_bytes().iter().enumerate() {
                if let Some(c) = d.get_mut(p + i) {
                    *c = *b;
                }
            }
            let _ = write!(desc, "u64@{p}={v:#x}; ");
        }
        // type swap plus filler values
        100..=119 => {
            let mut hits = Vec::new();
            let mut i = 0;
            while i + 8 <= d.len() {
                let mut w = [0u8; 8];
                for (j, b) in w.iter_mut().enumerate() {
                    *b = d.get(i + j).copied().unwrap_or(0);
                }
                if (14000..=15500).contains(&i64::from_le_bytes(w)) {
                    hits.push(i);
                }
                i += 1;
            }
            let Some(&p) = rng.pick(&hits) else {
                return;
            };
            let code = rng.pick(TYPE_CODES).copied().unwrap_or(0);
            let mut ins = Vec::new();
            ins.extend_from_slice(&code.to_le_bytes());
            let n = 1 + rng.below(40);
            for _ in 0..n {
                let v: u64 = match rng.below(4) {
                    0 => rng
                        .pick(&[0.0f64, 1.0, 0.5, -1.0, 1e300, f64::NAN])
                        .copied()
                        .unwrap_or(0.0)
                        .to_bits(),
                    1 => u64::from(rng.pick(&[0u32, 1, 2]).copied().unwrap_or(0)) | (1u64 << 32),
                    _ => rng
                        .pick(&[0u64, 1, 2, 3, 4, u64::MAX, 1 << 40])
                        .copied()
                        .unwrap_or(0),
                };
                ins.extend_from_slice(&v.to_le_bytes());
            }
            splice(d, p, p + 8, &ins);
            let _ = write!(desc, "btypefill@{p}={code}+{n}; ");
        }
        // 4-byte overwrite (booleans)
        65..=72 => {
            let p = rng.below(d.len());
            let vals: [u32; 6] = [0, 1, 2, u32::MAX, 1 << 31, 255];
            let v = rng.pick(&vals).copied().unwrap_or(0);
            for (i, b) in v.to_le_bytes().iter().enumerate() {
                if let Some(c) = d.get_mut(p + i) {
                    *c = *b;
                }
            }
            let _ = write!(desc, "u32@{p}={v:#x}; ");
        }
        // delete a range
        73..=80 => {
            let p = rng.below(d.len());
            let n = 1 + rng.below_mixed(4096, 16);
            splice(d, p, p + n, &[]);
            let _ = write!(desc, "delrange@{p}+{n}; ");
        }
        // duplicate a range
        81..=88 => {
            let p = rng.below(d.len());
            let n = 1 + rng.below_mixed(4096, 16);
            let chunk = d.get(p..(p + n).min(d.len())).unwrap_or(&[]).to_vec();
            let times = 1 + rng.below(4);
            let mut ins = Vec::new();
            for _ in 0..times {
                ins.extend_from_slice(&chunk);
            }
            let at = (p + chunk.len()).min(d.len());
            splice(d, at, at, &ins);
            let _ = write!(desc, "duprange@{p}+{n}x{times}; ");
        }
        // copy a range elsewhere
        89..=94 => {
            let p = rng.below(d.len());
            let n = 1 + rng.below(512);
            let chunk = d.get(p..(p + n).min(d.len())).unwrap_or(&[]).to_vec();
            let to = rng.below(d.len() + 1);
            splice(d, to, to, &chunk);
            let _ = write!(desc, "copyrange@{p}+{n}->{to}; ");
        }
        // binary type code swap: overwrite an 8-byte value that is a known type code
        _ => {
            let mut hits = Vec::new();
            let mut i = 0;
            while i + 8 <= d.len() {
                let mut w = [0u8; 8];
                for (j, b) in w.iter_mut().enumerate() {
                    *b = d.get(i + j).copied().unwrap_or(0);
                }
                let v = i64::from_le_bytes(w);
                if (14000..=15500).contains(&v) {
                    hits.push(i);
                }
                i += 1;
            }
            let Some(&p) = rng.pick(&hits) else {
                return;
            };
            let code = rng.pick(TYPE_CODES).copied().unwrap_or(0);
            for (j, b) in code.to_le_bytes().iter().enumerate() {
                if let Some(c) = d.get_mut(p + j) {
                    *c = *b;
                }
            }
            let _ = write!(desc, "btype@{p}={code}; ");
        }
    }
}

/// The mutated input of case `case` of `seed` (relative name `rel`), its file type and
/// a description of the mutations.
fn make_case(rel: &str, seed: &[u8], binary: bool, case: u64) -> (Vec<u8>, FileType, String) {
    let mut rng = Rng(fnv1a64(rel) ^ case.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let mut d = seed.to_vec();
    let mut desc = String::new();
    let n = match rng.below(10) {
        0..=4 => 1,
        5..=7 => 2,
        _ => 3 + rng.below(3),
    };
    for _ in 0..n {
        if binary {
            mutate_binary(&mut d, &mut rng, &mut desc);
        } else {
            mutate_ascii(&mut d, &mut rng, &mut desc);
        }
    }
    // a few cases read the bytes with the other file type
    let mut ft = if binary {
        FileType::Binary
    } else {
        FileType::Ascii
    };
    if rng.below(40) == 0 {
        ft = if binary {
            FileType::Ascii
        } else {
            FileType::Binary
        };
        desc.push_str("as-other-type; ");
    }
    (d, ft, desc)
}

// ---- running the reader -----------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// `BrepData::read_from_file` only.
    Read,
    /// `p7::read_from_file` (`SmBrep::ReadFromFile`: read and `MakeTopologyFromData`
    /// with its default flags).
    Full,
    /// Read, then `MakeTopologyFromData` without analytics, healing or UV trim curves
    /// (gets past the unported analytic-surface checks the default flags hit).
    Topo,
    /// `BrepData::read_from_db` with `persist_attribs` (attribute indices on every
    /// record and geometry object), after a `ReadFromFile`-style header; the attribute
    /// table starts with a few slots, some empty.
    Persist,
    /// Harness self-test: by input length mod 5, `ok`, a panic, a huge allocation, a
    /// hang or unbounded recursion (checks that every bad verdict is detected).
    SelfTest,
}

const KIND_SHIFT: u32 = 40;

fn key_of(e: EntityRef) -> ObjectRef {
    let (t, k, i) = match e {
        EntityRef::Brep(x) => (16011, 0, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code: t,
        key: ObjectKey((1u64 << KIND_SHIFT) * (k + 1) + u64::from(i)),
    }
}

static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn classify(r: &Result<(), SolidError>) -> String {
    if let Err(e) = r {
        let d = format!("{e:?}");
        let op = d
            .split("operation: \"")
            .nth(1)
            .and_then(|x| x.split('"').next())
            .unwrap_or("?");
        let reason = d
            .split("reason: \"")
            .nth(1)
            .and_then(|x| x.split('"').next())
            .or_else(|| d.split("code: ").nth(1).and_then(|x| x.split(',').next()))
            .unwrap_or("");
        if let Ok(mut g) = LAST_ERROR.lock() {
            *g = Some(format!("{op} | {reason}"));
        }
    }
    if std::env::var_os("READER_FUZZ_VERBOSE").is_some() {
        if let Err(e) = r {
            eprintln!("error: {e:?}");
        }
    }
    match r {
        Ok(()) => "ok".into(),
        Err(SolidError::Unported { .. }) => "unported".into(),
        Err(_) => "err".into(),
    }
}

/// Runs one input; returns `ok`, `err`, `unported` (with the stage that ended it).
fn run_input(bytes: Vec<u8>, ft: FileType, stage: Stage) -> String {
    if stage == Stage::SelfTest {
        return self_test(bytes.len());
    }
    if stage == Stage::Persist {
        return format!("persist-{}", classify(&read_persist(bytes, ft)));
    }
    let k = Kernel::default();
    let r = BrepData::read_from_file(&k, bytes.clone(), ft).map(|_| ());
    let v = classify(&r);
    if stage == Stage::Read || r.is_err() {
        return format!("read-{v}");
    }
    let mut k = Kernel::default();
    let Ok(mut b) = Brep::new_with_infinite_region() else {
        return "harness-error".into();
    };
    let mut records = solidcore::topology::topology::NotifyRecords::new();
    let mut attrs = AttributeStore::default();
    let mut env = p10::TopologyEnv {
        all_attributes: &[],
        attributes: &mut attrs,
        key_of: &key_of,
        records: &mut records,
        next_key: 1u64 << 50,
    };
    if stage == Stage::Topo {
        let Ok(mut data) = BrepData::read_from_file(&k, bytes, ft) else {
            return "harness-error".into();
        };
        let options = p10::MakeTopologyOptions {
            make_uv_trim_curves: false,
            add_analytics: false,
            force_same_tol: false,
            check_uv_trim_curves: false,
            always_heal: false,
            healer_enabled: false,
        };
        let r = p10::make_topology_from_data(&mut k, &mut b, &mut data, &mut env, options);
        return format!("topo-{}", classify(&r));
    }
    let r = p7::read_from_file(&mut k, &mut b, bytes, ft, false, &mut env);
    format!("full-{}", classify(&r))
}

/// [`Stage::Persist`]: the header as `BrepData::read_from_file` reads it, then
/// `read_from_db` with persisted attributes.
fn read_persist(bytes: Vec<u8>, ft: FileType) -> Result<(), SolidError> {
    let k = Kernel::default();
    let mut db = DatabaseReader::new();
    db.open_for_read(bytes, ft);
    let mut version = 30;
    if ft == FileType::Ascii {
        if let Some(s) = db.in_stream_mut() {
            let _ = s.extract_word();
            version = s.extract_ulong().unwrap_or(0);
        }
        db.go_to_next_line();
        if version > 41 {
            db.go_to_next_line();
        }
    } else {
        let _ = db.read_char()?;
        version = db.read_ulong()?;
        if version > 41 {
            let _ = db.read_char()?;
            let _ = db.read_ulong()?;
        }
    }
    db.set_version(version);
    let mut store = AttributeStore::new();
    let mut all = Vec::new();
    for i in 0..4u64 {
        if i % 2 == 0 {
            let h = store.insert(Attribute::new(1000 + i, Behavior::StandaloneReference))?;
            all.push(Some(h));
        } else {
            all.push(None);
        }
    }
    let mut attrs = AttributeSink::new(&mut store, &mut all, 0);
    BrepData::read_from_db(&k, version, &mut attrs, &mut db, true).map(|_| ())
}

fn recurse(n: u64) -> u64 {
    let mut p = std::hint::black_box([0u8; 512]);
    p[0] = p[0].wrapping_add(n as u8);
    if n == 0 {
        0
    } else {
        recurse(n - 1) + u64::from(std::hint::black_box(p)[0])
    }
}

fn self_test(len: usize) -> String {
    match len % 5 {
        // an arithmetic overflow (the sweep builds keep overflow checks)
        1 => format!("self-ok {}", std::hint::black_box(u64::MAX) + 1),
        2 => {
            let v: Vec<u8> = vec![1; std::hint::black_box(1usize << 40)];
            format!("self-ok {}", v.len())
        }
        3 => loop {
            std::thread::sleep(Duration::from_millis(100));
        },
        4 => format!("self-ok {}", recurse(std::hint::black_box(u64::MAX))),
        _ => "self-ok".into(),
    }
}

static PANIC_INFO: Mutex<Option<String>> = Mutex::new(None);

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let loc = info
            .location()
            .map_or_else(|| "?".to_string(), |l| format!("{}:{}", l.file(), l.line()));
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let s = format!("{loc} {}", msg.replace(['\n', '\t'], " "));
        if let Ok(mut g) = PANIC_INFO.lock() {
            *g = Some(s);
        }
    }));
}

fn reset_peak_rss() {
    let _ = std::fs::write("/proc/self/clear_refs", "5");
}

fn peak_rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|x| x.parse().ok()))
        })
        .unwrap_or(0)
}

/// Runs a case on a thread with `stack` bytes of stack; returns the verdict line.
fn run_guarded(bytes: Vec<u8>, ft: FileType, stage: Stage, stack: usize) -> String {
    if let Ok(mut g) = PANIC_INFO.lock() {
        *g = None;
    }
    reset_peak_rss();
    let t0 = Instant::now();
    let h = std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || run_input(bytes, ft, stage));
    let verdict = match h {
        Err(e) => format!("harness-error spawn {e}"),
        Ok(h) => h.join().unwrap_or_else(|_| {
            let info = PANIC_INFO
                .lock()
                .ok()
                .and_then(|g| g.clone())
                .unwrap_or_default();
            format!("panic {info}")
        }),
    };
    format!("{} {} {verdict}", t0.elapsed().as_millis(), peak_rss_kb())
}

// ---- seeds ------------------------------------------------------------------------------

struct Seed {
    rel: String,
    path: PathBuf,
    binary: bool,
}

fn list_seeds(dirs: &[PathBuf]) -> Vec<Seed> {
    let mut out = Vec::new();
    for root in dirs {
        let binary = root.file_name().is_some_and(|n| n == "corpus-bin");
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "smb") {
                    let rel = p
                        .strip_prefix(root)
                        .unwrap_or(&p)
                        .to_string_lossy()
                        .into_owned();
                    let rel = if binary { format!("bin:{rel}") } else { rel };
                    out.push(Seed {
                        rel,
                        path: p,
                        binary,
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

fn seed_binary(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == "corpus-bin")
}

// ---- child ------------------------------------------------------------------------------

/// `child --seed <path> --rel <rel> --from a --to b --stage s --stack n`: prints
/// `B <i>` before and `E <i> <ms> <peak_kb> <verdict>` after each case.
fn child(args: &Args) -> i32 {
    install_panic_hook();
    let Some(path) = args.get("seed") else {
        return 2;
    };
    let Ok(seed) = std::fs::read(path) else {
        return 2;
    };
    let rel = args.get("rel").unwrap_or_default();
    let binary = args.flag("binary");
    let from: u64 = args.num("from", 0);
    let to: u64 = args.num("to", 0);
    let stage = args.stage();
    let stack = args.num("stack-kb", 8192) as usize * 1024;
    let out = std::io::stdout();
    for i in from..to {
        let (bytes, ft, _) = make_case(&rel, &seed, binary, i);
        {
            let mut o = out.lock();
            let _ = writeln!(o, "B {i}");
            let _ = o.flush();
        }
        let v = run_guarded(bytes, ft, stage, stack);
        let mut o = out.lock();
        let _ = writeln!(o, "E {i} {v}");
        let _ = o.flush();
    }
    0
}

// ---- parent -----------------------------------------------------------------------------

struct Args(Vec<String>);

impl Args {
    fn get(&self, k: &str) -> Option<String> {
        let key = format!("--{k}");
        self.0
            .iter()
            .position(|a| *a == key)
            .and_then(|i| self.0.get(i + 1).cloned())
    }
    fn all(&self, k: &str) -> Vec<String> {
        let key = format!("--{k}");
        let mut v = Vec::new();
        let mut it = self.0.iter();
        while let Some(a) = it.next() {
            if *a == key {
                if let Some(x) = it.next() {
                    v.push(x.clone());
                }
            }
        }
        v
    }
    fn flag(&self, k: &str) -> bool {
        let key = format!("--{k}");
        self.0.contains(&key)
    }
    fn num(&self, k: &str, d: u64) -> u64 {
        self.get(k).and_then(|s| s.parse().ok()).unwrap_or(d)
    }
    fn stage(&self) -> Stage {
        match self.get("stage").as_deref() {
            Some("full") => Stage::Full,
            Some("topo") => Stage::Topo,
            Some("persist") => Stage::Persist,
            Some("selftest") => Stage::SelfTest,
            _ => Stage::Read,
        }
    }
}

struct Limits {
    mem_kb: u64,
    timeout: Duration,
    stack_kb: u64,
    stage: Stage,
}

/// One finished case.
#[derive(Clone)]
struct Outcome {
    rel: String,
    case: u64,
    ms: u64,
    peak_kb: u64,
    verdict: String,
}

fn self_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("reader_fuzz"))
}

/// Spawns a child under `ulimit -v` running `args`.
fn spawn_child(
    lim: &Limits,
    args: &[String],
    stderr_path: &Path,
) -> std::io::Result<std::process::Child> {
    let err = std::fs::File::create(stderr_path)?;
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(format!("ulimit -v {} && exec \"$0\" \"$@\"", lim.mem_kb))
        .arg(self_exe())
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::from(err))
        .stdin(Stdio::null());
    cmd.spawn()
}

fn classify_death(status: Option<std::process::ExitStatus>, stderr: &str) -> String {
    use std::os::unix::process::ExitStatusExt;
    let sig = status.and_then(|s| s.signal());
    let code = status.and_then(|s| s.code());
    let tail: String = stderr
        .lines()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .join(" | ")
        .replace('\t', " ");
    if stderr.contains("has overflowed its stack") {
        format!("stack-overflow sig={sig:?} {tail}")
    } else if stderr.contains("memory allocation of") || stderr.contains("capacity overflow") {
        format!("oom sig={sig:?} {tail}")
    } else {
        format!("crash sig={sig:?} code={code:?} {tail}")
    }
}

/// Runs cases `[from, to)` of one seed, restarting after a dead or hung case.
fn run_range(seed: &Seed, from: u64, to: u64, lim: &Limits, tmp: &Path) -> Vec<Outcome> {
    let mut out = Vec::new();
    let mut next = from;
    let stderr_path = tmp.join(format!(
        "stderr-{}-{from}.txt",
        seed.rel.replace(['/', ':'], "_")
    ));
    while next < to {
        let mut args = vec![
            "child".to_string(),
            "--seed".into(),
            seed.path.to_string_lossy().into_owned(),
            "--rel".into(),
            seed.rel.clone(),
            "--from".into(),
            next.to_string(),
            "--to".into(),
            to.to_string(),
            "--stack-kb".into(),
            lim.stack_kb.to_string(),
            "--stage".into(),
            stage_name(lim.stage).into(),
        ];
        if seed.binary {
            args.push("--binary".into());
        }
        let Ok(mut ch) = spawn_child(lim, &args, &stderr_path) else {
            eprintln!("cannot spawn child for {}", seed.rel);
            return out;
        };
        let Some(stdout) = ch.stdout.take() else {
            return out;
        };
        let (tx, rx) = mpsc::channel::<String>();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut current: Option<u64> = None;
        let mut hung = false;
        loop {
            match rx.recv_timeout(lim.timeout) {
                Ok(line) => {
                    let mut it = line.splitn(5, ' ');
                    match (it.next(), it.next()) {
                        (Some("B"), Some(i)) => current = i.parse().ok(),
                        (Some("E"), Some(i)) => {
                            let i: u64 = i.parse().unwrap_or(0);
                            let ms = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                            let peak_kb = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                            let verdict = it.next().unwrap_or("").to_string();
                            out.push(Outcome {
                                rel: seed.rel.clone(),
                                case: i,
                                ms,
                                peak_kb,
                                verdict,
                            });
                            current = None;
                            next = i + 1;
                        }
                        _ => {}
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if current.is_some() {
                        hung = true;
                        let _ = ch.kill();
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let status = ch.wait().ok();
        let _ = reader.join();
        if let Some(i) = current {
            let verdict = if hung {
                "timeout".to_string()
            } else {
                let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
                classify_death(status, &stderr)
            };
            out.push(Outcome {
                rel: seed.rel.clone(),
                case: i,
                ms: 0,
                peak_kb: 0,
                verdict,
            });
            next = i + 1;
        } else if next < to {
            // child exited without starting a case: give up on this range
            eprintln!("child for {} exited early at {next}", seed.rel);
            break;
        }
    }
    let _ = std::fs::remove_file(&stderr_path);
    out
}

fn stage_name(s: Stage) -> &'static str {
    match s {
        Stage::Read => "read",
        Stage::Full => "full",
        Stage::Topo => "topo",
        Stage::Persist => "persist",
        Stage::SelfTest => "selftest",
    }
}

fn is_fine(v: &str) -> bool {
    matches!(
        v,
        "read-ok"
            | "read-err"
            | "read-unported"
            | "full-ok"
            | "full-err"
            | "full-unported"
            | "topo-ok"
            | "topo-err"
            | "topo-unported"
            | "persist-ok"
            | "persist-err"
            | "persist-unported"
            | "self-ok"
    )
}

fn limits(args: &Args) -> Limits {
    Limits {
        mem_kb: args.num("mem-mb", 3072) * 1024,
        timeout: Duration::from_secs(args.num("timeout", 20)),
        stack_kb: args.num("stack-kb", 8192),
        stage: args.stage(),
    }
}

fn save_case(out_dir: &Path, o: &Outcome, seeds: &[Seed]) {
    let Some(seed) = seeds.iter().find(|s| s.rel == o.rel) else {
        return;
    };
    let Ok(bytes) = std::fs::read(&seed.path) else {
        return;
    };
    let (d, ft, desc) = make_case(&seed.rel, &bytes, seed.binary, o.case);
    let class = o.verdict.split_whitespace().next().unwrap_or("x");
    let dir = out_dir.join("crashes");
    let _ = std::fs::create_dir_all(&dir);
    let stem = format!("{class}-{}-{}", o.rel.replace(['/', ':'], "_"), o.case);
    let ext = if ft == FileType::Ascii {
        "smb"
    } else {
        "bin.smb"
    };
    let _ = std::fs::write(dir.join(format!("{stem}.{ext}")), d);
    let _ = std::fs::write(
        dir.join(format!("{stem}.txt")),
        format!(
            "seed: {}\ncase: {}\nfile type: {ft:?}\nmutations: {desc}\nverdict: {}\nms: {} peak_kb: {}\n",
            seed.path.display(),
            o.case,
            o.verdict,
            o.ms,
            o.peak_kb
        ),
    );
}

fn sweep(args: &Args) -> i32 {
    let dirs: Vec<PathBuf> = args
        .all("seeds-dir")
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let filter = args.get("filter");
    let mut seeds = list_seeds(&dirs);
    if let Some(f) = &filter {
        seeds.retain(|s| s.rel.contains(f.as_str()));
    }
    let cases = args.num("cases", 2000);
    let jobs = args.num("jobs", 4).max(1) as usize;
    let batch = args.num("batch", 250).max(1);
    let out_dir = PathBuf::from(args.get("out").unwrap_or_else(|| "reader-fuzz-out".into()));
    let tag = args.get("tag").unwrap_or_else(|| "sweep".into());
    let tmp = out_dir.join("tmp");
    let _ = std::fs::create_dir_all(&tmp);
    let lim = Arc::new(limits(args));
    let seeds = Arc::new(seeds);
    let mut queue: Vec<(usize, u64, u64)> = Vec::new();
    for si in 0..seeds.len() {
        let mut a = 0;
        while a < cases {
            queue.push((si, a, (a + batch).min(cases)));
            a += batch;
        }
    }
    queue.reverse();
    let total_jobs = queue.len();
    eprintln!(
        "{} seeds x {cases} cases, {total_jobs} batches, {jobs} jobs",
        seeds.len()
    );
    let queue = Arc::new(Mutex::new(queue));
    let results: Arc<Mutex<Vec<Outcome>>> = Arc::new(Mutex::new(Vec::new()));
    let t0 = Instant::now();
    let mut handles = Vec::new();
    for _ in 0..jobs {
        let (queue, results, seeds, lim, tmp) = (
            queue.clone(),
            results.clone(),
            seeds.clone(),
            lim.clone(),
            tmp.clone(),
        );
        handles.push(std::thread::spawn(move || loop {
            let job = queue.lock().ok().and_then(|mut q| q.pop());
            let Some((si, a, b)) = job else { break };
            let Some(seed) = seeds.get(si) else { break };
            let r = run_range(seed, a, b, &lim, &tmp);
            for o in r.iter().filter(|o| !is_fine(&o.verdict)) {
                eprintln!("FINDING {} #{} {}", o.rel, o.case, o.verdict);
            }
            if let Ok(mut g) = results.lock() {
                g.extend(r);
                let left = queue.lock().map_or(0, |q| q.len());
                if left % 50 == 0 {
                    eprintln!(
                        "progress: {} cases, {left} batches left, {:.0}s",
                        g.len(),
                        t0.elapsed().as_secs_f64()
                    );
                }
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    let results = results.lock().map(|g| g.clone()).unwrap_or_default();
    // summary
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut slow: Vec<&Outcome> = Vec::new();
    let mut tsv = String::from("seed\tcase\tms\tpeak_kb\tverdict\n");
    for o in &results {
        let class = if is_fine(&o.verdict) {
            o.verdict.clone()
        } else {
            o.verdict
                .split_whitespace()
                .next()
                .unwrap_or("?")
                .to_string()
        };
        *counts.entry(class).or_default() += 1;
        if !is_fine(&o.verdict) {
            let _ = writeln!(
                tsv,
                "{}\t{}\t{}\t{}\t{}",
                o.rel, o.case, o.ms, o.peak_kb, o.verdict
            );
            save_case(&out_dir, o, &seeds);
        } else if o.ms > 2000 || o.peak_kb > 1_000_000 {
            slow.push(o);
        }
    }
    let mut summary = format!(
        "tag {tag}\nseeds {}\ncases per seed {cases}\ntotal cases {}\nwall {:.0}s\n",
        seeds.len(),
        results.len(),
        t0.elapsed().as_secs_f64()
    );
    for (k, v) in &counts {
        let _ = writeln!(summary, "{k}\t{v}");
    }
    let max_ms = results.iter().map(|o| o.ms).max().unwrap_or(0);
    let max_kb = results.iter().map(|o| o.peak_kb).max().unwrap_or(0);
    let _ = writeln!(
        summary,
        "max case ms {max_ms}\nmax case peak rss kb {max_kb}"
    );
    let _ = writeln!(
        summary,
        "slow (>2 s) or big (>1 GB) fine cases: {}",
        slow.len()
    );
    for o in slow.iter().take(50) {
        let _ = writeln!(
            summary,
            "  {} #{} {} ms {} kb {}",
            o.rel, o.case, o.ms, o.peak_kb, o.verdict
        );
    }
    print!("{summary}");
    let _ = std::fs::write(out_dir.join(format!("{tag}-summary.txt")), &summary);
    let _ = std::fs::write(out_dir.join(format!("{tag}-findings.tsv")), &tsv);
    // per-seed verdict counts, compact
    let mut per: BTreeMap<&str, BTreeMap<&str, u64>> = BTreeMap::new();
    for o in &results {
        let c = o.verdict.split_whitespace().next().unwrap_or("?");
        *per.entry(o.rel.as_str()).or_default().entry(c).or_default() += 1;
    }
    let mut ps = String::new();
    for (s, m) in per {
        let _ = write!(ps, "{s}");
        for (c, n) in m {
            let _ = write!(ps, "\t{c}={n}");
        }
        ps.push('\n');
    }
    let _ = std::fs::write(out_dir.join(format!("{tag}-per-seed.tsv")), ps);
    i32::from(counts.keys().any(|k| !is_fine(k)))
}

/// `repro --seed <path> --case i [--write out]`: rebuilds and runs one case in-process.
fn repro(args: &Args) -> i32 {
    install_panic_hook();
    let Some(path) = args.get("seed") else {
        return 2;
    };
    let p = PathBuf::from(&path);
    let binary = seed_binary(&p);
    let rel = args.get("rel").unwrap_or_else(|| {
        // the relative name the sweep used
        let s = p.to_string_lossy().into_owned();
        if binary {
            format!(
                "bin:{}",
                p.file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default()
            )
        } else {
            s.split("pt_TestFiles/").nth(1).unwrap_or(&s).to_string()
        }
    });
    let Ok(seed) = std::fs::read(&p) else {
        return 2;
    };
    let (bytes, ft, desc) = make_case(&rel, &seed, binary, args.num("case", 0));
    println!("rel {rel}\nfile type {ft:?}\nmutations {desc}");
    if let Some(w) = args.get("write") {
        let _ = std::fs::write(w, &bytes);
    }
    let stack = args.num("stack-kb", 8192) as usize * 1024;
    println!("{}", run_guarded(bytes, ft, args.stage(), stack));
    0
}

/// `run --input <file> [--binary] [--stage full]`: runs a file in-process.
fn run_file(args: &Args) -> i32 {
    install_panic_hook();
    let Some(path) = args.get("input") else {
        return 2;
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return 2;
    };
    let ft = if args.flag("binary") {
        FileType::Binary
    } else {
        FileType::Ascii
    };
    let stack = args.num("stack-kb", 8192) as usize * 1024;
    println!("{}", run_guarded(bytes, ft, args.stage(), stack));
    0
}

/// The class of a single-file run in a child (for minimisation).
fn child_class(input: &Path, binary: bool, lim: &Limits, tmp: &Path) -> String {
    let mut args = vec![
        "run".to_string(),
        "--input".into(),
        input.to_string_lossy().into_owned(),
        "--stack-kb".into(),
        lim.stack_kb.to_string(),
        "--stage".into(),
        stage_name(lim.stage).into(),
    ];
    if binary {
        args.push("--binary".into());
    }
    let errp = tmp.join("min-stderr.txt");
    let Ok(mut ch) = spawn_child(lim, &args, &errp) else {
        return "spawn-error".into();
    };
    let Some(stdout) = ch.stdout.take() else {
        return "spawn-error".into();
    };
    let (tx, rx) = mpsc::channel::<String>();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut BufReader::new(stdout), &mut s);
        let _ = tx.send(s);
    });
    let res = rx.recv_timeout(lim.timeout);
    let out = if let Ok(s) = res {
        let st = ch.wait().ok();
        let line = s.lines().last().unwrap_or("").to_string();
        if line.is_empty() {
            let stderr = std::fs::read_to_string(&errp).unwrap_or_default();
            classify_death(st, &stderr)
        } else {
            line.splitn(3, ' ').nth(2).unwrap_or("").to_string()
        }
    } else {
        let _ = ch.kill();
        let _ = ch.wait();
        "timeout".into()
    };
    let _ = reader.join();
    out
}

/// `minimize --input f [--binary] --expect <prefix>`: greedy removal of line blocks,
/// then byte blocks, keeping the verdict's prefix.
fn minimize(args: &Args) -> i32 {
    let (Some(input), Some(expect)) = (args.get("input"), args.get("expect")) else {
        return 2;
    };
    let binary = args.flag("binary");
    let lim = limits(args);
    let out_path = args.get("output").unwrap_or_else(|| format!("{input}.min"));
    let tmp = PathBuf::from(format!("{out_path}.tmpdir"));
    let _ = std::fs::create_dir_all(&tmp);
    let Ok(mut data) = std::fs::read(&input) else {
        return 2;
    };
    let probe = tmp.join("probe.smb");
    let test = |d: &[u8]| -> bool {
        let _ = std::fs::write(&probe, d);
        child_class(&probe, binary, &lim, &tmp).starts_with(expect.as_str())
    };
    if !test(&data) {
        eprintln!("input does not reproduce `{expect}`");
        return 1;
    }
    // unit = lines for ASCII, then bytes
    for unit_lines in [true, false] {
        let mut chunk = if unit_lines { 256 } else { 64 };
        while chunk >= 1 {
            let mut i = 0;
            loop {
                let pieces: Vec<(usize, usize)> = if unit_lines {
                    let ls = line_starts(&data);
                    let mut v = Vec::new();
                    let mut k = 0;
                    while k < ls.len() {
                        v.push(line_range(&data, &ls, k, chunk));
                        k += chunk;
                    }
                    v
                } else {
                    (0..data.len())
                        .step_by(chunk)
                        .map(|a| (a, (a + chunk).min(data.len())))
                        .collect()
                };
                let Some(&(a, b)) = pieces.get(i) else { break };
                let mut cand = data.clone();
                splice(&mut cand, a, b, &[]);
                if test(&cand) {
                    data = cand;
                } else {
                    i += 1;
                }
            }
            chunk /= 2;
        }
        let _ = std::fs::write(&out_path, &data);
        eprintln!(
            "{} pass: {} bytes",
            if unit_lines { "line" } else { "byte" },
            data.len()
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
    println!("{out_path} {} bytes", data.len());
    0
}

/// `stats --seeds-dir d.. --cases n [--stage s]`: runs cases in-process (only safe
/// once a sweep found no crash) and counts the error sites reached, as a coverage proxy.
fn stats(args: &Args) -> i32 {
    install_panic_hook();
    let dirs: Vec<PathBuf> = args
        .all("seeds-dir")
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let seeds = list_seeds(&dirs);
    let cases = args.num("cases", 200);
    let stage = args.stage();
    let mut sites: BTreeMap<String, u64> = BTreeMap::new();
    let mut verdicts: BTreeMap<String, u64> = BTreeMap::new();
    for seed in &seeds {
        let Ok(bytes) = std::fs::read(&seed.path) else {
            continue;
        };
        for i in 0..cases {
            let (d, ft, _) = make_case(&seed.rel, &bytes, seed.binary, i);
            if let Ok(mut g) = LAST_ERROR.lock() {
                *g = None;
            }
            let v = run_input(d, ft, stage);
            *verdicts.entry(v).or_default() += 1;
            if let Some(e) = LAST_ERROR.lock().ok().and_then(|g| g.clone()) {
                *sites.entry(e).or_default() += 1;
            }
        }
    }
    for (k, v) in &verdicts {
        println!("verdict\t{v}\t{k}");
    }
    let mut by: Vec<(&String, &u64)> = sites.iter().collect();
    by.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (k, v) in by {
        println!("site\t{v}\t{k}");
    }
    0
}

pub(crate) fn main() {
    let args = Args(std::env::args().skip(1).collect());
    let code = match args.0.first().map(String::as_str) {
        Some("child") => child(&args),
        Some("sweep") => sweep(&args),
        Some("repro") => repro(&args),
        Some("run") => run_file(&args),
        Some("minimize") => minimize(&args),
        Some("stats") => stats(&args),
        _ => {
            eprintln!(
                "usage: reader_fuzz sweep|repro|run|minimize|child ... (see the file header)"
            );
            2
        }
    };
    std::process::exit(code);
}
