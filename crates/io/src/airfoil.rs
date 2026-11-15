//! Airfoil coordinate files (`.dat`), as the public airfoil databases publish them: a name line,
//! then `x y` pairs with the chord running from 0 to 1. Two layouts are read, told apart by their
//! content, not the file name:
//!
//! - **Selig:** one loop from the trailing edge over the upper side to the leading edge and back
//!   along the lower side.
//! - **Lednicer:** a line with the two point counts (`61. 61.`), then the upper side and the lower
//!   side, each from the leading edge to the trailing edge.
//!
//! Real files vary, and the reader takes what they do: CRLF line endings, `#` comments, blank lines
//! of spaces, counts written as `61.0`, a repeated leading-edge point, an open or closed trailing
//! edge, an `x` a hair above 1 and numbers like `-.00041`. What it does not do is guess: a
//! placeholder (`......`), a third column or a point off the chord is an error naming the line.
//! The points come back in Selig order, so the sketch gets one loop whatever the file's layout.

use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;

use crate::{IoError, Result};

/// Largest airfoil file read: a section is a few kilobytes, the biggest known about 50 KB.
pub const MAX_AIRFOIL_BYTES: usize = 1 << 20;
/// Most points a file may hold; past this the reader stops rather than read on.
pub const MAX_AIRFOIL_POINTS: usize = 20_000;
/// How far an `x` may leave 0..1: files round their coordinates and come out at 1.00001.
const X_SLACK: f64 = 1e-5;
/// Points closer than this (in chord units) are the same point written twice.
const SAME_POINT: f64 = 1e-9;

/// The layout of an airfoil file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirfoilFormat {
    /// One loop: trailing edge, upper side, leading edge, lower side, trailing edge.
    Selig,
    /// Two point counts, then the upper and the lower side, each from the leading edge.
    Lednicer,
}

impl AirfoilFormat {
    /// The format's name as the commands take and return it.
    pub fn name(self) -> &'static str {
        match self {
            AirfoilFormat::Selig => "selig",
            AirfoilFormat::Lednicer => "lednicer",
        }
    }

    /// The format named `name` (any case), if there is one.
    pub fn parse(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("selig") {
            Some(AirfoilFormat::Selig)
        } else if name.eq_ignore_ascii_case("lednicer") {
            Some(AirfoilFormat::Lednicer)
        } else {
            None
        }
    }
}

/// An airfoil read from a file: its points in Selig order (trailing edge, upper side, leading edge,
/// lower side, trailing edge), in chord units (`x` from 0 to 1).
#[derive(Clone, Debug, PartialEq)]
pub struct Airfoil {
    /// The name line of the file (empty when the file starts with coordinates).
    pub name: String,
    /// The layout the file was read as.
    pub format: AirfoilFormat,
    /// The points in Selig order, a point written twice in a row read once.
    pub points: Vec<Vec2>,
    /// The index in `points` of the leading edge: the point nearest `x = 0`.
    pub leading_edge: usize,
    /// How far the first and the last point are apart: 0 for a closed trailing edge.
    pub te_gap: f64,
}

/// A line of the file that holds data: its 1-based number and its tokens.
struct Row<'a> {
    line: usize,
    tokens: Vec<&'a str>,
}

/// Read an airfoil file. `format` forces a layout; `None` tells Selig and Lednicer apart by the
/// content: a first pair of whole numbers above 1 whose sum is the number of points that follow is
/// a Lednicer count line. The errors name the line of the file they are about.
pub fn read_airfoil(text: &str, format: Option<AirfoilFormat>) -> Result<Airfoil> {
    if text.len() > MAX_AIRFOIL_BYTES {
        return Err(IoError::Invalid("the airfoil file is too large".into()));
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rows: Vec<Row<'_>> = text
        .split('\n')
        .enumerate()
        .filter_map(|(i, raw)| {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            Some(Row { line: i + 1, tokens: line.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).collect() })
        })
        .collect();
    let mut rows = rows.into_iter().peekable();
    // The first line is the name, unless the file starts with its coordinates (two numbers).
    let name = match rows.peek().map(|row| is_pair(&row.tokens)) {
        None => return Err(IoError::Invalid("the file is empty".into())),
        Some(true) => String::new(),
        Some(false) => rows.next().map(|row| row.tokens.join(" ")).unwrap_or_default(),
    };
    // A column header (`x y`) some files add under the name.
    if rows.peek().is_some_and(|row| {
        let lower: Vec<String> = row.tokens.iter().map(|t| t.to_ascii_lowercase()).collect();
        lower == ["x", "y"] || lower == ["x/c", "y/c"]
    }) {
        rows.next();
    }
    let mut pairs: Vec<(usize, Vec2)> = Vec::new();
    for row in rows {
        if pairs.len() >= MAX_AIRFOIL_POINTS {
            return Err(IoError::Invalid(format!("more than {MAX_AIRFOIL_POINTS} points")));
        }
        let [x, y] = row.tokens[..] else {
            return Err(IoError::Invalid(format!("line {}: expected two numbers, found {}: `{}`", row.line, row.tokens.len(), row.tokens.join(" "))));
        };
        pairs.push((row.line, Vec2::new(number(x, row.line)?, number(y, row.line)?)));
    }
    let Some((first, body)) = pairs.split_first() else {
        return Err(IoError::Invalid("no coordinates".into()));
    };
    let count_line = (is_count(first.1.x) && is_count(first.1.y)).then_some((first.0, first.1.x as usize, first.1.y as usize));
    let format = match format {
        Some(f) => f,
        None => match count_line {
            Some((_, upper, lower)) if upper + lower == body.len() => AirfoilFormat::Lednicer,
            _ => AirfoilFormat::Selig,
        },
    };
    let points: Vec<Vec2> = match format {
        AirfoilFormat::Lednicer => {
            let Some((line, upper, lower)) = count_line else {
                return Err(IoError::Invalid(format!("line {}: a Lednicer file starts with the two point counts", first.0)));
            };
            if upper + lower != body.len() {
                let found = body.windows(2).position(|w| w[1].1.x < w[0].1.x).map(|i| i + 1).unwrap_or(body.len());
                return Err(IoError::Invalid(format!("line {line}: Lednicer counts say {upper}+{lower}, found {found}+{}", body.len() - found)));
            }
            if upper < 2 || lower < 2 {
                return Err(IoError::Invalid(format!("line {line}: each side needs at least two points")));
            }
            check_range(body)?;
            let (upper, lower) = body.split_at(upper.min(body.len()));
            let mut points: Vec<Vec2> = upper.iter().rev().map(|(_, p)| *p).collect();
            points.extend(lower.iter().map(|(_, p)| *p));
            points
        }
        AirfoilFormat::Selig => {
            check_range(&pairs)?;
            pairs.iter().map(|(_, p)| *p).collect()
        }
    };
    // A point written twice in a row (the leading edge of a Lednicer file) is read once.
    let mut clean: Vec<Vec2> = Vec::with_capacity(points.len());
    for p in points {
        if clean.last().is_none_or(|q| q.dist(p) > SAME_POINT) {
            clean.push(p);
        }
    }
    if clean.len() < 4 {
        return Err(IoError::Invalid(format!("{} points; an airfoil needs at least four", clean.len())));
    }
    let leading_edge = clean
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (a.x, a.y.abs()).partial_cmp(&(b.x, b.y.abs())).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0);
    if leading_edge == 0 || leading_edge + 1 == clean.len() {
        return Err(IoError::Invalid("the leading edge is at an end of the points: not a loop from the trailing edge and back".into()));
    }
    let te_gap = match (clean.first(), clean.last()) {
        (Some(a), Some(b)) => a.dist(*b),
        _ => 0.0,
    };
    Ok(Airfoil { name, format, points: clean, leading_edge, te_gap })
}

/// Whether the tokens are two numbers (a coordinate pair or a count line), not a name.
fn is_pair(tokens: &[&str]) -> bool {
    matches!(tokens, [a, b] if number(a, 0).is_ok() && number(b, 0).is_ok())
}

/// Whether `v` is a point count: a whole number above 1 (a coordinate never is).
fn is_count(v: f64) -> bool {
    v > 1.0 + X_SLACK && v.fract() == 0.0 && v <= MAX_AIRFOIL_POINTS as f64
}

/// Every `x` lies on the chord (0 to 1, with a little slack for rounding) and every `y` within
/// a chord of it; anything else is not an airfoil, or not one in chord units.
fn check_range(pairs: &[(usize, Vec2)]) -> Result<()> {
    for (line, p) in pairs {
        if p.x < -X_SLACK || p.x > 1.0 + X_SLACK {
            return Err(IoError::Invalid(format!("line {line}: x = {} is outside 0..1", p.x)));
        }
        if p.y.abs() > 1.0 {
            return Err(IoError::Invalid(format!("line {line}: y = {} is outside -1..1", p.y)));
        }
    }
    Ok(())
}

/// A strictly written number: digits with an optional sign, point and exponent, nothing else, so
/// a placeholder such as `......` or `(0.5)` is never read as a value.
fn number(token: &str, line: usize) -> Result<f64> {
    let digits = token.chars().filter(char::is_ascii_digit).count();
    let shape = token.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | 'e' | 'E'));
    let value = if digits > 0 && shape { token.parse::<f64>().ok().filter(|v| v.is_finite()) } else { None };
    value.ok_or_else(|| IoError::Invalid(format!("line {line}: not a number: `{token}`")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// NACA 00xx thickness (the public four-digit formula, trailing edge open), `n` points per
    /// side on a cosine spacing, written in Selig order.
    fn naca_00(thickness: f64, n: usize) -> Vec<Vec2> {
        let yt = |x: f64| 5.0 * thickness * (0.2969 * x.sqrt() - 0.1260 * x - 0.3516 * x * x + 0.2843 * x * x * x - 0.1015 * x * x * x * x);
        let xs: Vec<f64> = (0..=n).map(|i| 0.5 * (1.0 - (std::f64::consts::PI * i as f64 / n as f64).cos())).collect();
        let mut pts: Vec<Vec2> = xs.iter().rev().map(|&x| Vec2::new(x, yt(x))).collect();
        pts.extend(xs.iter().skip(1).map(|&x| Vec2::new(x, -yt(x))));
        pts
    }

    fn selig_text(name: &str, pts: &[Vec2]) -> String {
        let mut s = format!("{name}\n");
        for p in pts {
            s.push_str(&format!("{:.6} {:.6}\n", p.x, p.y));
        }
        s
    }

    #[test]
    fn a_selig_file_reads_in_order_with_its_name() {
        let pts = naca_00(0.12, 20);
        let a = read_airfoil(&selig_text("NACA 0012", &pts), None).unwrap();
        assert_eq!(a.name, "NACA 0012");
        assert_eq!(a.format, AirfoilFormat::Selig);
        assert_eq!(a.points.len(), 41);
        assert_eq!(a.leading_edge, 20);
        assert!((a.te_gap - 2.0 * 0.00126).abs() < 1e-5, "{}", a.te_gap);
        assert!((a.points[0].x - 1.0).abs() < 1e-9 && a.points[0].y > 0.0);
    }

    /// Lednicer: the counts line, both sides from the nose, the nose written twice; CRLF, a count
    /// written as `61.0`, blank lines of spaces and `#` comments, as the public databases write
    /// them.
    #[test]
    fn a_lednicer_file_becomes_one_loop() {
        let pts = naca_00(0.12, 20);
        let upper: Vec<Vec2> = pts[..=20].iter().rev().copied().collect();
        let lower: Vec<Vec2> = pts[20..].to_vec();
        let mut s = String::from("# from a database\r\nNACA 0012 (Lednicer)\r\n   \r\n      21.0      21.\r\n\r\n");
        for p in &upper {
            s.push_str(&format!("  {:.6}  {:.6}\r\n", p.x, p.y));
        }
        s.push_str("  \r\n# lower\r\n");
        for p in &lower {
            s.push_str(&format!("  {:.6}  {:.6}\r\n", p.x, p.y));
        }
        let a = read_airfoil(&s, None).unwrap();
        assert_eq!(a.format, AirfoilFormat::Lednicer);
        assert_eq!(a.name, "NACA 0012 (Lednicer)");
        assert_eq!(a.points.len(), 41, "the nose is read once");
        assert_eq!(a.leading_edge, 20);
        let expect = read_airfoil(&selig_text("x", &pts), None).unwrap();
        for (p, q) in a.points.iter().zip(&expect.points) {
            assert!(p.dist(*q) < 1e-9, "{p:?} vs {q:?}");
        }
        // Forced as Selig, the count line is a point off the chord.
        let e = read_airfoil(&s, Some(AirfoilFormat::Selig)).unwrap_err().to_string();
        assert_eq!(e, "line 4: x = 21 is outside 0..1");
        // Forced as Lednicer, a Selig file has no counts.
        let e = read_airfoil(&selig_text("x", &pts), Some(AirfoilFormat::Lednicer)).unwrap_err().to_string();
        assert_eq!(e, "line 2: a Lednicer file starts with the two point counts");
    }

    #[test]
    fn lednicer_counts_that_do_not_match_name_what_was_found() {
        let pts = naca_00(0.12, 20);
        let upper: Vec<Vec2> = pts[..=20].iter().rev().copied().collect();
        let lower: Vec<Vec2> = pts[20..].to_vec();
        let mut s = String::from("NACA 0012\n21. 21.\n");
        for p in upper.iter().skip(1).chain(&lower) {
            s.push_str(&format!("{:.6} {:.6}\n", p.x, p.y));
        }
        let e = read_airfoil(&s, Some(AirfoilFormat::Lednicer)).unwrap_err().to_string();
        assert_eq!(e, "line 2: Lednicer counts say 21+21, found 20+21");
        // Without the override, a count line that doesn't add up reads as Selig, and fails as one.
        let e = read_airfoil(&s, None).unwrap_err().to_string();
        assert_eq!(e, "line 2: x = 21 is outside 0..1");
    }

    /// The quirks real files have, one each, and the ones that are refused with their line.
    #[test]
    fn quirks_are_read_and_junk_is_refused_by_line() {
        let pts = naca_00(0.10, 12);
        let good = selig_text("Quirky", &pts);
        // A header line, a number like `-.00041`, commas, an x of 1.00001, a trailing-edge point
        // written twice.
        let quirky = good.replace("Quirky\n", "Quirky\nx y\n").replace("1.000000 0.001050", "1.00001, +.00105\n1.00001, .00105");
        let a = read_airfoil(&quirky, None).unwrap();
        assert_eq!(a.points.len(), pts.len());
        assert!((a.points[0].y - 0.00105).abs() < 1e-9);
        // No name line: the coordinates start at once.
        let bare = good.replacen("Quirky\n", "", 1);
        let a = read_airfoil(&bare, None).unwrap();
        assert_eq!((a.name.as_str(), a.points.len()), ("", pts.len()));
        for (text, message) in [
            ("", "the file is empty"),
            ("Only a name\n", "no coordinates"),
            ("Name\n......\n0 0\n", "line 2: expected two numbers, found 1: `......`"),
            ("Name\n0.5 ...... \n", "line 2: not a number: `......`"),
            ("Name\n0.5 (0.1)\n", "line 2: not a number: `(0.1)`"),
            ("Name\n0.5 0.1 0.2\n", "line 2: expected two numbers, found 3: `0.5 0.1 0.2`"),
            ("Name\n1 0\n0.5 0.1\n100 0\n0.5 -0.1\n1 0\n", "line 4: x = 100 is outside 0..1"),
            ("Name\n1 0\n0.5 0.1\n0 0\n0.5 -2\n1 0\n", "line 5: y = -2 is outside -1..1"),
            ("Name\n0.5 nan\n", "line 2: not a number: `nan`"),
            ("Name\n0.5 1e999\n", "line 2: not a number: `1e999`"),
            ("Name\n1 0\n0 0\n1 0\n", "3 points; an airfoil needs at least four"),
            ("Name\n0 0\n0.5 0.1\n1 0\n0.5 -0.1\n", "the leading edge is at an end of the points: not a loop from the trailing edge and back"),
        ] {
            let e = read_airfoil(text, None).unwrap_err().to_string();
            assert_eq!(e, message, "{text:?}");
        }
        let too_big = "x".repeat(MAX_AIRFOIL_BYTES + 1);
        assert_eq!(read_airfoil(&too_big, None).unwrap_err().to_string(), "the airfoil file is too large");
        let many = format!("Name\n{}", "0.5 0\n".repeat(MAX_AIRFOIL_POINTS + 1));
        assert_eq!(read_airfoil(&many, None).unwrap_err().to_string(), format!("more than {MAX_AIRFOIL_POINTS} points"));
    }

    /// The leading edge is the smallest `x`, not where `y` changes sign: a reflexed section has
    /// negative `y` on its upper side near the trailing edge.
    #[test]
    fn the_leading_edge_is_the_smallest_x() {
        let s = "Reflexed\n1 0.0\n0.8 -0.01\n0.5 0.05\n0.2 0.06\n0.00044 0.002\n0.2 -0.03\n0.5 -0.02\n0.8 -0.012\n1 -0.001\n";
        let a = read_airfoil(s, None).unwrap();
        assert_eq!(a.leading_edge, 4);
        assert!((a.te_gap - 0.001).abs() < 1e-12);
        assert_eq!(a.format.name(), "selig");
        assert_eq!(AirfoilFormat::parse("Lednicer"), Some(AirfoilFormat::Lednicer));
        assert_eq!(AirfoilFormat::parse("csv"), None);
    }

    /// Hostile text never panics (`catch_unwind` turns one into a failed test).
    #[test]
    fn hostile_airfoil_text_never_panics() {
        let cases = [
            "",
            "\n",
            "#",
            "\u{feff}",
            "x y",
            "1 1",
            "2 2\n",
            "3 3\n0 0\n0 0\n0 0\n0 0\n0 0\n0 0",
            "a\n1e308 1e308",
            "a\n-0 -0\n-0 -0\n-0 -0\n-0 -0",
            "a\n1 0\n0 0\n1 0\n0 0\n1 0",
            "a\n2. 2.\n0 0\n1 0\n0 0\n1 0",
            "a\n+ -\n",
            "a\n. .\n",
            "a\ne e\n",
            "a\n1e 1e\n",
        ];
        for text in cases {
            let r = std::panic::catch_unwind(|| read_airfoil(text, None).map(|a| a.points.len()));
            assert!(r.is_ok(), "{text:?} panicked");
            for f in [AirfoilFormat::Selig, AirfoilFormat::Lednicer] {
                assert!(std::panic::catch_unwind(|| read_airfoil(text, Some(f)).map(|a| a.points.len())).is_ok(), "{text:?} {f:?} panicked");
            }
        }
    }
}
