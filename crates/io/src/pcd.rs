//! PCD (Point Cloud Data) import, from the format's public description: the file the Point Cloud
//! Library writes. A text header names the fields of a point (`FIELDS`, with each one's `SIZE` in
//! bytes, its `TYPE` (`I`, `U` or `F`) and its `COUNT` of values), says how many points there are
//! (`POINTS`, or `WIDTH` times `HEIGHT`) and how they are stored (`DATA`): as text (`ascii`), as
//! rows of little-endian values (`binary`), or as one LZF-compressed block holding each field's
//! values for all the points in turn (`binary_compressed`). `x`, `y` and `z` are the positions
//! and a packed `rgb` or `rgba` field the colours. Every other field with one value a point
//! becomes a column named as in the file, whole-numbered when its type is an integer. A field
//! with several values a point and the padding fields named `_` are passed over. `VIEWPOINT`
//! (where the scanner stood) is not applied: the points keep the file's own coordinates. The
//! file is untrusted: counts are checked against its size and capped.

use solidcraft_doc::point_cloud::{Column, Complex, Floats, MAX_COLUMNS, On};

use crate::ply_complex::PlyCloud;
use crate::{IoError, Result};

/// Largest header read (bytes).
const MAX_HEADER: usize = 1 << 20;
const MAX_FIELDS: usize = 256;
/// Most values in one point (a descriptor field has a few hundred).
const MAX_VALUES: usize = 1 << 16;
/// How many times its length LZF data can grow at most: three bytes make up to 264.
const LZF_GROWTH: usize = 88;

fn invalid(m: impl std::fmt::Display) -> IoError {
    IoError::Invalid(format!("PCD: {m}"))
}

fn early() -> IoError {
    invalid("the file ends early")
}

/// The start of a name or value from the file, for a message.
fn short(text: &str) -> String {
    text.chars().take(32).collect()
}

/// The type of a field's values: its `TYPE` and `SIZE`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Num {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
}

impl Num {
    fn of(kind: &str, size: usize) -> Option<Num> {
        Some(match (kind.to_ascii_uppercase().as_str(), size) {
            ("I", 1) => Num::I8,
            ("U", 1) => Num::U8,
            ("I", 2) => Num::I16,
            ("U", 2) => Num::U16,
            ("I", 4) => Num::I32,
            ("U", 4) => Num::U32,
            ("I", 8) => Num::I64,
            ("U", 8) => Num::U64,
            ("F", 4) => Num::F32,
            ("F", 8) => Num::F64,
            _ => return None,
        })
    }
    fn size(self) -> usize {
        match self {
            Num::I8 | Num::U8 => 1,
            Num::I16 | Num::U16 => 2,
            Num::I32 | Num::U32 | Num::F32 => 4,
            Num::I64 | Num::U64 | Num::F64 => 8,
        }
    }
    fn integer(self) -> bool {
        !matches!(self, Num::F32 | Num::F64)
    }
    /// The little-endian value at the start of `b`.
    fn read(self, b: &[u8]) -> Option<f64> {
        fn take<const N: usize>(b: &[u8]) -> Option<[u8; N]> {
            b.get(..N)?.try_into().ok()
        }
        Some(match self {
            Num::I8 => f64::from(i8::from_le_bytes(take(b)?)),
            Num::U8 => f64::from(*b.first()?),
            Num::I16 => f64::from(i16::from_le_bytes(take(b)?)),
            Num::U16 => f64::from(u16::from_le_bytes(take(b)?)),
            Num::I32 => f64::from(i32::from_le_bytes(take(b)?)),
            Num::U32 => f64::from(u32::from_le_bytes(take(b)?)),
            Num::I64 => i64::from_le_bytes(take(b)?) as f64,
            Num::U64 => u64::from_le_bytes(take(b)?) as f64,
            Num::F32 => f64::from(f32::from_le_bytes(take(b)?)),
            Num::F64 => f64::from_le_bytes(take(b)?),
        })
    }
}

/// One field of a point.
#[derive(Debug)]
struct Field {
    name: String,
    num: Num,
    /// How many values a point has of it.
    count: usize,
    /// Where its first value is in a point, counted in values (text).
    first: usize,
    /// The same, counted in bytes (binary).
    offset: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Encoding {
    Ascii,
    Binary,
    Compressed,
}

/// What the header says, and where the points start.
#[derive(Debug)]
struct Header {
    fields: Vec<Field>,
    points: u64,
    encoding: Encoding,
    body: usize,
    /// Values in one point.
    values: usize,
    /// Bytes in one point of a binary file.
    row: usize,
}

/// A header line's entry for each of the `n` fields.
fn per_field<T>(given: Option<Vec<T>>, n: usize, what: &str) -> Result<Vec<T>> {
    let given = given.ok_or_else(|| invalid(format!("no `{what}` line")))?;
    if given.len() != n {
        return Err(invalid(format!("`{what}` does not have one entry for each field")));
    }
    Ok(given)
}

fn header(bytes: &[u8]) -> Result<Header> {
    let head = bytes.get(..bytes.len().min(MAX_HEADER)).unwrap_or(bytes);
    let whole = |v: Option<&str>, what: &str| v.and_then(|v| v.parse::<u64>().ok()).ok_or_else(|| invalid(format!("`{what}` is not a whole number")));
    let wholes = |it: std::str::SplitWhitespace<'_>, what: &str| {
        it.map(|v| v.parse::<usize>().ok()).collect::<Option<Vec<usize>>>().ok_or_else(|| invalid(format!("a `{what}` is not a whole number")))
    };
    let mut names: Vec<String> = Vec::new();
    let (mut sizes, mut types, mut counts) = (None, None, None);
    let (mut width, mut height, mut points) = (None, None, None);
    let mut pos = 0;
    let encoding = loop {
        let rest = head.get(pos..).filter(|r| !r.is_empty()).ok_or_else(|| invalid("no `DATA` line in the first megabyte"))?;
        let end = rest.iter().position(|b| *b == b'\n');
        let line = String::from_utf8_lossy(rest.get(..end.unwrap_or(rest.len())).unwrap_or_default());
        pos += end.map_or(rest.len(), |e| e + 1);
        let mut it = line.split_whitespace();
        match it.next() {
            Some("FIELDS") => names = it.map(str::to_string).collect(),
            Some("SIZE") => sizes = Some(wholes(it, "SIZE")?),
            Some("TYPE") => types = Some(it.map(str::to_string).collect::<Vec<String>>()),
            Some("COUNT") => counts = Some(wholes(it, "COUNT")?),
            Some("WIDTH") => width = Some(whole(it.next(), "WIDTH")?),
            Some("HEIGHT") => height = Some(whole(it.next(), "HEIGHT")?),
            Some("POINTS") => points = Some(whole(it.next(), "POINTS")?),
            Some("DATA") => {
                break match it.next() {
                    Some("ascii") => Encoding::Ascii,
                    Some("binary") => Encoding::Binary,
                    Some("binary_compressed") => Encoding::Compressed,
                    other => return Err(invalid(format!("unknown `DATA` kind `{}`", short(other.unwrap_or(""))))),
                };
            }
            // `VERSION`, `VIEWPOINT`, comments (`#`) and blank lines.
            _ => {}
        }
    };
    let n = names.len();
    if n == 0 {
        return Err(invalid("no `FIELDS` line"));
    }
    if n > MAX_FIELDS {
        return Err(invalid("too many fields"));
    }
    let sizes = per_field(sizes, n, "SIZE")?;
    let types = per_field(types, n, "TYPE")?;
    // A file without `COUNT` has one value of each field.
    let counts = if counts.is_some() { per_field(counts, n, "COUNT")? } else { vec![1; n] };
    let mut fields = Vec::with_capacity(n);
    let (mut values, mut row) = (0usize, 0usize);
    for (((name, size), kind), count) in names.into_iter().zip(sizes).zip(types).zip(counts) {
        let num = Num::of(&kind, size)
            .ok_or_else(|| invalid(format!("SolidCraft does not read the type of field `{}` (`{}` of {size} bytes)", short(&name), short(&kind))))?;
        let (first, offset) = (values, row);
        values = values.checked_add(count).filter(|v| *v <= MAX_VALUES).ok_or_else(|| invalid("a point has more values than SolidCraft reads"))?;
        row += count * num.size();
        fields.push(Field { name, num, count, first, offset });
    }
    let points = match (points, width) {
        (Some(p), _) => p,
        (None, Some(w)) => w.checked_mul(height.unwrap_or(1)).ok_or_else(|| invalid("`WIDTH` times `HEIGHT` is too large"))?,
        (None, None) => return Err(invalid("no `POINTS` line")),
    };
    Ok(Header { fields, points, encoding, body: pos, values, row })
}

/// Expand LZF-compressed `data` into exactly `size` bytes, from the format's public description.
/// A control byte under 32 copies that many bytes and one more from the data. Any other control
/// byte copies bytes already written: its top three bits are their count less two (seven means
/// the next byte adds to the count), and its other five bits, with the byte after as the low
/// part, say how far back they start, less one.
fn lzf(data: &[u8], size: usize) -> Result<Vec<u8>> {
    let damaged = || invalid("the compressed points are damaged");
    if size > data.len().saturating_mul(LZF_GROWTH) {
        return Err(damaged());
    }
    // Reserved once and never outgrown: a copy that would pass `size` is refused before it is made.
    let mut out: Vec<u8> = Vec::new();
    out.try_reserve_exact(size).map_err(|_| invalid("not enough memory to expand the compressed points"))?;
    let mut pos = 0;
    while let Some(control) = data.get(pos).map(|c| usize::from(*c)) {
        pos += 1;
        if control < 32 {
            let run = data.get(pos..pos + control + 1).ok_or_else(damaged)?;
            if run.len() > size - out.len() {
                return Err(damaged());
            }
            out.extend_from_slice(run);
            pos += run.len();
        } else {
            let mut len = (control >> 5) + 2;
            if len == 9 {
                len += usize::from(*data.get(pos).ok_or_else(damaged)?);
                pos += 1;
            }
            let back = ((control & 0x1f) << 8) + usize::from(*data.get(pos).ok_or_else(damaged)?) + 1;
            pos += 1;
            let from = out.len().checked_sub(back).ok_or_else(damaged)?;
            if len > size - out.len() {
                return Err(damaged());
            }
            if back >= len {
                out.extend_from_within(from..from + len);
            } else {
                // The copy runs into the bytes it writes: a short pattern repeated.
                for k in from..from + len {
                    let b = *out.get(k).ok_or_else(damaged)?;
                    out.push(b);
                }
            }
        }
    }
    if out.len() != size {
        return Err(damaged());
    }
    Ok(out)
}

/// A value written as text. "Not a number" is read however a C library prints it (`nan`,
/// `-nan(ind)`, `1.#QNAN`).
fn number(token: &[u8]) -> Result<f64> {
    let text = std::str::from_utf8(token).unwrap_or_default();
    if let Ok(v) = text.parse::<f64>() {
        return Ok(v);
    }
    let bare = text.trim_start_matches(['+', '-']).to_ascii_lowercase();
    if bare.starts_with("nan") || bare.starts_with("1.#") {
        return Ok(f64::NAN);
    }
    Err(invalid(format!("`{}` is not a number", short(&String::from_utf8_lossy(token)))))
}

/// Where the points' values are.
enum Source<'a> {
    /// The values of the point being read, as text.
    Text(Vec<&'a [u8]>),
    /// A point's values after another's.
    Rows { b: &'a [u8], row: usize },
    /// A field's values for all the points after another's.
    Columns { b: Vec<u8>, points: usize },
}

impl<'a> Source<'a> {
    /// Take the next point's `count` values from `tokens` (text; a binary source holds them all).
    fn load(&mut self, tokens: &mut impl Iterator<Item = &'a [u8]>, count: usize) -> Result<()> {
        if let Source::Text(values) = self {
            values.clear();
            for _ in 0..count {
                values.push(tokens.next().ok_or_else(early)?);
            }
        }
        Ok(())
    }
    /// The bytes that start with point `r`'s value of `f`.
    fn at(&self, f: &Field, r: usize) -> Result<&[u8]> {
        match self {
            Source::Text(values) => values.get(f.first).copied(),
            Source::Rows { b, row } => r.checked_mul(*row).and_then(|p| p.checked_add(f.offset)).and_then(|p| b.get(p..)),
            Source::Columns { b, points } => {
                points.checked_mul(f.offset).and_then(|p| p.checked_add(r.checked_mul(f.num.size())?)).and_then(|p| b.get(p..))
            }
        }
        .ok_or_else(early)
    }
    fn value(&self, f: &Field, r: usize) -> Result<f64> {
        let b = self.at(f, r)?;
        match self {
            Source::Text(_) => number(b),
            _ => f.num.read(b).ok_or_else(early),
        }
    }
    /// The 32 bits of a packed colour: alpha, red, green and blue, blue lowest. A binary file
    /// holds the bits themselves whatever the field's type. Text holds the bits as a whole number.
    /// In a float field it holds either that whole number (as the Point Cloud Library writes it)
    /// or a float whose own bits are the colour; a whole number from 1 up is taken as the bits,
    /// since no usual colour is such a float. A colour that is not a number has lost its bits and
    /// is white.
    fn color(&self, f: &Field, r: usize) -> Result<[u8; 3]> {
        let b = self.at(f, r)?;
        let bits = match self {
            Source::Text(_) => {
                let v = number(b)?;
                let whole = v >= 1.0 && v <= f64::from(u32::MAX) && v.fract() == 0.0;
                if v.is_nan() {
                    return Ok([255; 3]);
                } else if f.num == Num::F32 && !whole {
                    // Read as a float at once, not through a double: its bits are the colour.
                    std::str::from_utf8(b).ok().and_then(|s| s.parse::<f32>().ok()).map_or(0, f32::to_bits)
                } else {
                    v as i64 as u32
                }
            }
            _ => b.get(..4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes).ok_or_else(early)?,
        };
        Ok([(bits >> 16) as u8, (bits >> 8) as u8, bits as u8])
    }
}

/// Read a PCD file: every point when they fit in `max_points`, else every `stride`th point.
pub(crate) fn read_pcd_cloud(bytes: &[u8], max_points: usize) -> Result<PlyCloud> {
    let h = header(bytes)?;
    // The first `x`, `y` and `z` are the position and the first packed `rgb` or `rgba` the
    // colour; every other field with one value a point is a column.
    let single = || h.fields.iter().filter(|f| f.count == 1);
    let named = |name: &str| single().find(|f| f.name.eq_ignore_ascii_case(name));
    let (Some(x), Some(y), Some(z)) = (named("x"), named("y"), named("z")) else {
        return Err(invalid("the points have no `x`, `y` and `z`"));
    };
    let rgb = single().find(|f| f.num.size() == 4 && (f.name.eq_ignore_ascii_case("rgb") || f.name.eq_ignore_ascii_case("rgba")));
    let used = [Some(x), Some(y), Some(z), rgb];
    let others: Vec<&Field> = single().filter(|f| f.name != "_" && !used.iter().flatten().any(|u| std::ptr::eq(*u, *f))).collect();
    if others.len() > MAX_COLUMNS {
        return Err(invalid(format!("more than {MAX_COLUMNS} properties")));
    }
    let body = bytes.get(h.body..).unwrap_or_default();
    let too_short = || invalid(format!("the file is too short for its {} points", h.points));
    let mut source = match h.encoding {
        Encoding::Ascii => {
            // A value takes at least a byte.
            if h.points > body.len() as u64 {
                return Err(too_short());
            }
            Source::Text(Vec::with_capacity(h.values))
        }
        Encoding::Binary => {
            if h.points.checked_mul(h.row as u64).is_none_or(|n| n > body.len() as u64) {
                return Err(too_short());
            }
            Source::Rows { b: body, row: h.row }
        }
        Encoding::Compressed => {
            // The block's length as it is stored, then the length it expands to.
            let length = |at: usize| body.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes).ok_or_else(early);
            let (stored, expanded) = (length(0)? as usize, length(4)? as usize);
            let data = body.get(8..).and_then(|b| b.get(..stored)).ok_or_else(early)?;
            if h.points.checked_mul(h.row as u64) != Some(expanded as u64) {
                return Err(invalid(format!("the compressed points do not expand to the file's {} points", h.points)));
            }
            // The expanded block is held in memory as a file's bytes are: under the same limit.
            let limit = crate::limits::Kind::PointCloud.limit();
            if expanded as u64 > limit {
                return Err(invalid(format!(
                    "the compressed points expand to {}, over the {} limit for point cloud files. {}",
                    crate::limits::describe(expanded as u64),
                    crate::limits::describe(limit),
                    crate::limits::how_to_raise()
                )));
            }
            Source::Columns { b: lzf(data, expanded)?, points: h.points as usize }
        }
    };
    let max = max_points.max(1) as u64;
    let tokens = || body.split(u8::is_ascii_whitespace).filter(|t| !t.is_empty());
    let position = |s: &Source, r: usize| -> Result<Option<[f64; 3]>> {
        let p = [s.value(x, r)?, s.value(y, r)?, s.value(z, r)?];
        Ok(p.iter().all(|v| v.is_finite()).then_some(p))
    };
    // Points without a position (`nan`, as a depth camera writes where it saw nothing) are left
    // out before thinning, so they never take the place of points that have one.
    let mut eligible = h.points;
    if h.points > max {
        let mut t = tokens();
        eligible = 0;
        // The checks above leave no more points than a slice can hold.
        for r in 0..h.points as usize {
            source.load(&mut t, h.values)?;
            eligible += u64::from(position(&source, r)?.is_some());
        }
    }
    let stride = eligible.div_ceil(max).max(1);
    let mut out = PlyCloud {
        positions: Vec::new(),
        colors: rgb.map(|_| Vec::new()),
        complex: Complex::default(),
        vertices: h.points,
        bad_faces: 0,
        cells_left_out: 0,
        skipped: 0,
    };
    let mut columns: Vec<Vec<f32>> = vec![Vec::new(); others.len()];
    let (mut t, mut seen) = (tokens(), 0u64);
    for r in 0..h.points as usize {
        source.load(&mut t, h.values)?;
        let Some(p) = position(&source, r)? else {
            out.skipped += 1;
            continue;
        };
        seen += 1;
        if !(seen - 1).is_multiple_of(stride) {
            continue;
        }
        out.positions.push(p);
        if let (Some(colors), Some(f)) = (out.colors.as_mut(), rgb) {
            colors.push(source.color(f, r)?);
        }
        for (values, f) in columns.iter_mut().zip(&others) {
            values.push(source.value(f, r)? as f32);
        }
    }
    let columns = others
        .iter()
        .zip(columns)
        .map(|(f, values)| Column { name: f.name.clone(), on: On::Point, integer: f.num.integer(), values: Floats(values.into()) })
        .collect();
    out.complex = Complex { columns, ..Complex::default() };
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header for `points` points with these `FIELDS`, `SIZE`, `TYPE` and `COUNT` lines.
    fn head(fields: &str, size: &str, kind: &str, count: &str, points: usize, data: &str) -> Vec<u8> {
        format!(
            "# .PCD v0.7 - Point Cloud Data file format\nVERSION 0.7\nFIELDS {fields}\nSIZE {size}\nTYPE {kind}\nCOUNT {count}\n\
             WIDTH {points}\nHEIGHT 1\nVIEWPOINT 0 0 0 1 0 0 0\nPOINTS {points}\nDATA {data}\n"
        )
        .into_bytes()
    }

    /// `bytes` as an LZF block of plain runs alone, after the two lengths a file gives first.
    fn stored(bytes: &[u8]) -> Vec<u8> {
        let mut block = Vec::new();
        for run in bytes.chunks(32) {
            block.push(run.len() as u8 - 1);
            block.extend(run);
        }
        let mut out = (block.len() as u32).to_le_bytes().to_vec();
        out.extend((bytes.len() as u32).to_le_bytes());
        out.extend(block);
        out
    }

    /// Four points with a colour, an intensity and a label each.
    const POINTS: [([f32; 3], u32, f32, u8); 4] = [
        ([0.0, 0.0, 0.0], 0x00FF_0000, 0.5, 2),
        ([1.5, -2.0, 3.0], 0x0000_FF00, 1.5, 7),
        ([4.0, 5.0, 6.25], 0xFF00_00FF, 2.5, 2),
        ([-7.0, 8.0, 9.0], 0x0010_2030, 3.5, 0),
    ];

    fn check(r: &PlyCloud) {
        assert_eq!(r.vertices, 4);
        assert_eq!(r.positions, [[0.0, 0.0, 0.0], [1.5, -2.0, 3.0], [4.0, 5.0, 6.25], [-7.0, 8.0, 9.0]]);
        assert_eq!(r.colors.as_deref(), Some(&[[255, 0, 0], [0, 255, 0], [0, 0, 255], [16, 32, 48]][..]), "alpha is passed over");
        let column = |n: &str| r.complex.column(n, On::Point).unwrap();
        assert_eq!(&*column("intensity").values.0, &[0.5, 1.5, 2.5, 3.5]);
        assert_eq!(&*column("label").values.0, &[2.0, 7.0, 2.0, 0.0]);
        assert!(column("label").integer && !column("intensity").integer);
        assert_eq!(r.complex.columns.len(), 2);
        r.complex.check(4).unwrap();
    }

    #[test]
    fn text_binary_and_compressed_files_hold_the_same_points() {
        let fields = ("x y z rgb intensity label", "4 4 4 4 4 1", "F F F U F U", "1 1 1 1 1 1");
        let mut text = head(fields.0, fields.1, fields.2, fields.3, 4, "ascii");
        let mut rows = head(fields.0, fields.1, fields.2, fields.3, 4, "binary");
        for (p, rgb, intensity, label) in POINTS {
            text.extend(format!("{} {} {} {rgb} {intensity} {label}\n", p[0], p[1], p[2]).into_bytes());
            rows.extend(p.iter().flat_map(|v| v.to_le_bytes()));
            rows.extend(rgb.to_le_bytes());
            rows.extend(intensity.to_le_bytes());
            rows.push(label);
        }
        check(&read_pcd_cloud(&text, 10).unwrap());
        check(&read_pcd_cloud(&rows, 10).unwrap());
        // Compressed, a field's values for all the points come before the next field's.
        let mut columns = Vec::new();
        for axis in 0..3 {
            columns.extend(POINTS.iter().flat_map(|(p, ..)| p[axis].to_le_bytes()));
        }
        columns.extend(POINTS.iter().flat_map(|(_, rgb, ..)| rgb.to_le_bytes()));
        columns.extend(POINTS.iter().flat_map(|(_, _, intensity, _)| intensity.to_le_bytes()));
        columns.extend(POINTS.iter().map(|(.., label)| *label));
        assert_eq!(columns.len(), 4 * 21);
        let mut compressed = head(fields.0, fields.1, fields.2, fields.3, 4, "binary_compressed");
        compressed.extend(stored(&columns));
        check(&read_pcd_cloud(&compressed, 10).unwrap());
        // A header with Windows line ends and no `COUNT` or `POINTS` reads the same.
        let windows = String::from_utf8(text).unwrap().replace("COUNT 1 1 1 1 1 1\n", "").replace("POINTS 4\n", "").replace('\n', "\r\n");
        check(&read_pcd_cloud(windows.as_bytes(), 10).unwrap());
    }

    #[test]
    fn lzf_copies_plain_runs_and_bytes_already_written() {
        // `abc` as it is; three bytes from three back; ten from one back, the last byte repeated.
        let data = [2, b'a', b'b', b'c', 0x20, 2, 0xE0, 1, 0];
        assert_eq!(lzf(&data, 16).unwrap(), b"abcabccccccccccc");
        assert!(lzf(&data, 15).is_err() && lzf(&data, 17).is_err(), "not the length the file says");
        assert!(lzf(&[0x20, 0], 3).is_err(), "a copy from before the start");
        assert!(lzf(&[5, b'a'], 6).is_err(), "a run longer than the data");
        assert!(lzf(&[0xE0], 9).is_err() && lzf(&[0xE0, 1], 10).is_err(), "a copy cut short");
        assert!(lzf(&[], 1).is_err() && lzf(&[], 0).unwrap().is_empty());
        // Two points at (1, 2, 3): each field's four bytes, then the same four from four back.
        let mut file = head("x y z", "4 4 4", "F F F", "1 1 1", 2, "binary_compressed");
        let block = [3, 0, 0, 0x80, 0x3F, 0x40, 3, 3, 0, 0, 0, 0x40, 0x40, 3, 3, 0, 0, 0x40, 0x40, 0x40, 3];
        file.extend((block.len() as u32).to_le_bytes());
        file.extend(24u32.to_le_bytes());
        file.extend(block);
        assert_eq!(read_pcd_cloud(&file, 10).unwrap().positions, [[1.0, 2.0, 3.0], [1.0, 2.0, 3.0]]);
    }

    /// The compressed points of a file Open3D 0.20.0 wrote, its two lengths first: sixteen points
    /// at (i / 2, 1, -2), red where i is even and cyan where it is odd.
    const COMPRESSED_BY_OPEN3D: [u8; 96] = [
        0x58, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x60, 0x00, 0x03, 0x3F, 0x00, 0x00, 0x80, 0x20, 0x03, 0x00, 0xC0, 0x20,
        0x03, 0x04, 0x00, 0x40, 0x00, 0x00, 0x20, 0x20, 0x03, 0x00, 0x40, 0x20, 0x03, 0x00, 0x60, 0x20, 0x03, 0x00, 0x80, 0x20, 0x03, 0x00, 0x90,
        0x20, 0x03, 0x00, 0xA0, 0x20, 0x03, 0x00, 0xB0, 0x20, 0x03, 0x00, 0xC0, 0x20, 0x03, 0x00, 0xD0, 0x20, 0x03, 0x00, 0xE0, 0x20, 0x03, 0x00,
        0xF0, 0x20, 0x03, 0x40, 0x37, 0xE0, 0x33, 0x03, 0x01, 0x00, 0xC0, 0xE0, 0x35, 0x03, 0x03, 0xFF, 0x00, 0xFF, 0xFF, 0x20, 0x41, 0xE0, 0x2C,
        0x07, 0x01, 0x00, 0x00,
    ];

    #[test]
    fn points_another_program_compressed_read_back() {
        let mut file = head("x y z rgb", "4 4 4 4", "F F F U", "1 1 1 1", 16, "binary_compressed");
        file.extend(COMPRESSED_BY_OPEN3D);
        let r = read_pcd_cloud(&file, 100).unwrap();
        let positions: Vec<[f64; 3]> = (0..16).map(|i| [f64::from(i) * 0.5, 1.0, -2.0]).collect();
        let colors: Vec<[u8; 3]> = (0..16).map(|i| if i % 2 == 0 { [255, 0, 0] } else { [0, 255, 255] }).collect();
        assert_eq!((r.vertices, r.positions, r.colors), (16, positions, Some(colors)));
    }

    #[test]
    fn a_grid_of_points_leaves_out_the_points_without_a_number_before_thinning() {
        // A depth camera's picture: `WIDTH` by `HEIGHT` with no `POINTS` line, and `nan` where it
        // saw nothing.
        let file = "VERSION .7\nFIELDS x y z t\nSIZE 4 4 4 8\nTYPE F F F F\nWIDTH 3\nHEIGHT 2\nDATA ascii\n\
            0 0 1 0.5\nnan nan nan 1.5\n2 0 1 2.5\n0 1 1 3.5\n1 1 1 4.5\n2 1 1 5.5\n";
        let all = read_pcd_cloud(file.as_bytes(), 6).unwrap();
        assert_eq!((all.vertices, all.positions.len(), all.skipped), (6, 5, 1));
        assert!(all.positions.iter().flatten().all(|v| v.is_finite()) && all.colors.is_none());
        assert_eq!(&*all.complex.column("t", On::Point).unwrap().values.0, &[0.5, 2.5, 3.5, 4.5, 5.5], "the point left out takes its number");
        // Thinned, every second of the five points with a position.
        let thin = read_pcd_cloud(file.as_bytes(), 3).unwrap();
        assert_eq!((thin.vertices, thin.skipped), (6, 1));
        assert_eq!(thin.positions, [[0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [2.0, 1.0, 1.0]]);
        assert_eq!(&*thin.complex.column("t", On::Point).unwrap().values.0, &[0.5, 3.5, 5.5]);
        thin.complex.check(3).unwrap();
        // A point without a position never takes the place of one that has it.
        let one = "FIELDS x y z\nSIZE 4 4 4\nTYPE F F F\nPOINTS 2\nDATA ascii\nnan nan nan\n1 2 3\n";
        assert_eq!(read_pcd_cloud(one.as_bytes(), 1).unwrap().positions, [[1.0, 2.0, 3.0]]);
    }

    #[test]
    fn padding_and_fields_of_several_values_are_passed_over() {
        // As the Point Cloud Library pads a point: `_` fields, the colour a float holding its bits.
        let fields = ("x y z _ rgb histogram Z", "4 4 4 1 4 4 2", "F F F U F F I", "1 1 1 4 1 3 1");
        // Red 128 with alpha 255 is no number as a float: the bits must come through as they are.
        let rgb = 0xFF80_40C0u32;
        let mut rows = head(fields.0, fields.1, fields.2, fields.3, 2, "binary");
        for k in 0..2u8 {
            rows.extend([1.0f32, 2.0, 3.0].iter().flat_map(|v| (v + f32::from(k)).to_le_bytes()));
            rows.extend([0xAA; 4]);
            rows.extend(rgb.to_le_bytes());
            rows.extend([9.0f32; 3].iter().flat_map(|v| v.to_le_bytes()));
            rows.extend((-5i16 * i16::from(k)).to_le_bytes());
        }
        let r = read_pcd_cloud(&rows, 10).unwrap();
        assert_eq!(r.positions, [[1.0, 2.0, 3.0], [2.0, 3.0, 4.0]]);
        assert_eq!(r.colors.as_deref(), Some(&[[128, 64, 192]; 2][..]));
        // A second `z` is a column of its own, as any other field is.
        assert_eq!(r.complex.columns.len(), 1);
        assert_eq!(&*r.complex.column("Z", On::Point).unwrap().values.0, &[0.0, -5.0]);
    }

    #[test]
    fn colours_and_missing_numbers_written_as_text_are_read() {
        let colour = |kind: &str, text: &str| {
            let file = format!("FIELDS x y z rgb\nSIZE 4 4 4 4\nTYPE F F F {kind}\nPOINTS 1\nDATA ascii\n1 2 3 {text}\n");
            read_pcd_cloud(file.as_bytes(), 10).unwrap().colors.unwrap()[0]
        };
        // A float field holds a float whose bits are the colour, or the bits as a whole number.
        let red = f32::from_bits(0x00FF_0000);
        assert_eq!(colour("F", &format!("{red:e}")), [255, 0, 0]);
        assert_eq!(colour("F", "4294901760"), [255, 0, 0]);
        assert_eq!(colour("F", "65280"), [0, 255, 0]);
        assert_eq!(colour("F", "0"), [0, 0, 0]);
        // An opaque colour can be "not a number" as a float: its bits are lost.
        assert_eq!(colour("F", "nan"), [255, 255, 255]);
        // A whole-numbered field holds the bits, signed or not.
        assert_eq!(colour("U", "255"), [0, 0, 255]);
        assert_eq!(colour("I", "-16711936"), [0, 255, 0]);
        // A missing position as a C library on Windows prints it, and type letters in lower case.
        // Both are read as numbers, so the file is read, and both points are left out.
        let file = "FIELDS x y z\nSIZE 4 4 4\nTYPE f f f\nPOINTS 3\nDATA ascii\n1 2 -nan(ind)\n4 5 1.#QNAN\n7 8 9\n";
        let r = read_pcd_cloud(file.as_bytes(), 10).unwrap();
        assert_eq!((r.positions, r.skipped), (vec![[7.0, 8.0, 9.0]], 2));
    }

    #[test]
    fn hostile_pcd_never_panics() {
        let ok = "FIELDS x y z\nSIZE 4 4 4\nTYPE F F F\nPOINTS 1\nDATA ascii\n1 2 3\n";
        assert_eq!(read_pcd_cloud(ok.as_bytes(), 10).unwrap().positions, [[1.0, 2.0, 3.0]]);
        let binary = |points: &str, data: &[u8]| {
            let mut b = format!("FIELDS x y z\nSIZE 4 4 4\nTYPE F F F\nPOINTS {points}\nDATA binary\n").into_bytes();
            b.extend(data);
            b
        };
        let compressed = |points: u32, stored: u32, expanded: u32, block: &[u8]| {
            let mut b = format!("FIELDS x y z\nSIZE 4 4 4\nTYPE F F F\nPOINTS {points}\nDATA binary_compressed\n").into_bytes();
            b.extend(stored.to_le_bytes());
            b.extend(expanded.to_le_bytes());
            b.extend(block);
            b
        };
        let many = format!("FIELDS {}\nSIZE 4\nTYPE F\nPOINTS 1\nDATA ascii\n", "x ".repeat(MAX_FIELDS + 1));
        let mut not_text = ok.replace("1 2 3\n", "1 2 ").into_bytes();
        not_text.push(0xFF);
        let cases = [
            Vec::new(),
            b"VERSION .7\n".to_vec(),
            b"\xff\xfe\x00DATA".to_vec(),
            ok.replace("DATA ascii", "").into_bytes(),
            ok.replace("DATA ascii", "DATA zip").into_bytes(),
            ok.replace("FIELDS x y z\n", "").into_bytes(),
            ok.replace("SIZE 4 4 4\n", "").into_bytes(),
            ok.replace("TYPE F F F\n", "").into_bytes(),
            ok.replace("SIZE 4 4 4", "SIZE 4 4").into_bytes(),
            ok.replace("SIZE 4 4 4", "SIZE 4 4 3").into_bytes(),
            ok.replace("SIZE 4 4 4", "SIZE 4 4 -4").into_bytes(),
            ok.replace("TYPE F F F", "TYPE F F Q").into_bytes(),
            ok.replace("TYPE F F F", "TYPE F F F\nCOUNT 1 1").into_bytes(),
            ok.replace("TYPE F F F", "TYPE F F F\nCOUNT 1 1 99999999999999999999").into_bytes(),
            ok.replace("TYPE F F F", "TYPE F F F\nCOUNT 1 1 18446744073709551615").into_bytes(),
            ok.replace("TYPE F F F", "TYPE F F F\nCOUNT 1 1 2").into_bytes(),
            ok.replace("FIELDS x y z", "FIELDS x y w").into_bytes(),
            ok.replace("POINTS 1", "POINTS many").into_bytes(),
            ok.replace("POINTS 1", "POINTS -1").into_bytes(),
            ok.replace("POINTS 1", "WIDTH 4294967296\nHEIGHT 4294967296").into_bytes(),
            ok.replace("POINTS 1\n", "").into_bytes(),
            ok.replace("POINTS 1", "POINTS 2").into_bytes(),
            ok.replace("POINTS 1", "POINTS 99999999999").into_bytes(),
            ok.replace("1 2 3", "1 2").into_bytes(),
            ok.replace("1 2 3", "1 2 three").into_bytes(),
            not_text,
            many.into_bytes(),
            binary("1", &[0; 11]),
            binary("2", &[0; 12]),
            binary("18446744073709551615", &[0; 12]),
            compressed(1, 0, 12, &[]),
            compressed(1, 4, 12, &[0, 0]),
            compressed(1, 2, 13, &[0, 0]),
            compressed(1, 2, 12, &[0, 0]),
            compressed(1, 3, 12, &[0x20, 0, 0]),
            compressed(1, 4, 12, &[0xE0, 0xFF, 0, 0]),
            // Two bytes said to expand to two gigabytes: over the limit, more than LZF can make.
            compressed(178_956_970, 2, 2_147_483_640, &[0, 0]),
            b"FIELDS x y z\nSIZE 4 4 4\nTYPE F F F\nPOINTS 1\nDATA binary_compressed\n\x01\x00\x00".to_vec(),
        ];
        for b in &cases {
            let r = std::panic::catch_unwind(|| read_pcd_cloud(b, 10).is_ok());
            assert_eq!(r.ok(), Some(false), "{:?}", String::from_utf8_lossy(b));
        }
    }
}
