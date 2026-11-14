//! The stream contract of an ASCII B-rep data file (`SmBrep::ReadFromFile`), ported from
//! the reference binding's stream simulator for the `topology.brep_p7` read cases'
//! generator and runner: a stream keeps it when every extraction of upstream's read
//! succeeds within the stream, no count reaches `SM_BIG_ULONG`, no attribute count or
//! index exceeds `2^20`, every curve and surface kind is known (B-spline curves and
//! surfaces, composite curves and offset surfaces up to version 32, the non-geometry type
//! code `1`), B-spline headers keep `1 <= degree <= count` with non-decreasing knots over
//! a non-degenerate domain and positive weights, a closed composite curve has segments,
//! composite curves nest at most 32 deep, and every geometry attribute index is in the
//! attribute table. For an upstream test file the simulation stops, and accepts, at the
//! first geometry kind it does not know.

/// Why a stream is outside the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rejected(String);

fn bad(m: impl Into<String>) -> Rejected {
    Rejected(m.into())
}

/// Whether the ASCII stream `bytes` keeps the contract (`upstream_file`: the exact bytes
/// of a shipped upstream test file).
#[must_use]
pub fn ascii_file_accepted(bytes: &[u8], upstream_file: bool) -> bool {
    let mut sim = Sim {
        data: bytes,
        pos: 0,
        binary: false,
        eof: false,
        table: Vec::new(),
        depth: 0,
        trust: upstream_file,
    };
    sim.file().is_ok()
}

/// `SM_BIG_ULONG`: `SmTArray::SetSize` refuses this size or more.
const BIG_ULONG: u64 = 99_999_999;
/// Largest attribute count / index accepted (as the Rust port).
const MAX_ATTR: u64 = 1 << 20;
/// Deepest composite curve nesting accepted (as the Rust reader's
/// `solidcore::topology::database_io::MAX_READ_NESTING`: each composite is one level).
const MAX_DEPTH: u32 = 32;
/// Largest B-spline control point count accepted (per direction).
const MAX_POINTS: i64 = 100_000;
/// Largest B-spline degree accepted.
const MAX_DEGREE: i64 = 30;

/// Outcome of a simulated geometry read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Read {
    /// Read completely; the caller continues.
    Done,
    /// Upstream returns an error status here (nothing more is read), or a trusted
    /// upstream file holds a kind the simulator does not know: stop simulating.
    Failed,
}

/// Simulates upstream's reads on the bytes (strictly: anything that could make an
/// extraction fail is rejected).
struct Sim<'a> {
    data: &'a [u8],
    pos: usize,
    binary: bool,
    /// The ASCII stream hit its end (eofbit): any further extraction fails.
    eof: bool,
    /// Attribute table slots (`true` = an attribute is present).
    table: Vec<bool>,
    depth: u32,
    /// Accept geometry kinds the simulator does not know (upstream test files).
    trust: bool,
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

impl Sim<'_> {
    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    // ---- binary --------------------------------------------------------------------

    fn take(&mut self, n: usize) -> Result<&[u8], Rejected> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| bad("short binary data"))?;
        let s = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| bad("short binary data"))?;
        self.pos = end;
        Ok(s)
    }

    fn b_u64(&mut self) -> Result<u64, Rejected> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(u64::from_ne_bytes(a))
    }

    fn b_i16(&mut self) -> Result<i16, Rejected> {
        let s = self.take(2)?;
        Ok(i16::from_ne_bytes([
            s.first().copied().unwrap_or(0),
            s.get(1).copied().unwrap_or(0),
        ]))
    }

    fn b_f64(&mut self) -> Result<f64, Rejected> {
        Ok(f64::from_bits(self.b_u64()?))
    }

    // ---- ASCII ---------------------------------------------------------------------

    /// The sentry: skips white space; fails at the end.
    fn sentry(&mut self) -> Result<(), Rejected> {
        if self.eof {
            return Err(bad("extraction after the end of the stream"));
        }
        while let Some(c) = self.peek() {
            if !is_space(c) {
                return Ok(());
            }
            self.pos += 1;
        }
        Err(bad("extraction at the end of the stream"))
    }

    /// A whitespace-delimited token (its end sets eof when it is the stream's end).
    fn token(&mut self) -> Result<&[u8], Rejected> {
        self.sentry()?;
        let start = self.pos;
        while let Some(c) = self.peek() {
            if is_space(c) {
                break;
            }
            self.pos += 1;
        }
        if self.pos >= self.data.len() {
            self.eof = true;
        }
        Ok(self.data.get(start..self.pos).unwrap_or(&[]))
    }

    /// `>> word` with a buffer of `cap` bytes (words longer than `cap - 1` are split).
    fn word(&mut self, cap: usize) -> Result<(), Rejected> {
        self.sentry()?;
        let start = self.pos;
        while let Some(c) = self.peek() {
            if is_space(c) || self.pos - start >= cap - 1 {
                break;
            }
            self.pos += 1;
        }
        if self.pos >= self.data.len() {
            self.eof = true;
        }
        Ok(())
    }

    fn int_token(&mut self) -> Result<(bool, u64), Rejected> {
        let t = self.token()?;
        let (neg, digits) = match t.first() {
            Some(b'-') => (true, t.get(1..).unwrap_or(&[])),
            Some(b'+') => (false, t.get(1..).unwrap_or(&[])),
            _ => (false, t),
        };
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return Err(bad("not an integer token"));
        }
        let text = core::str::from_utf8(digits).map_err(|_| bad("not an integer token"))?;
        let v: u64 = text.parse().map_err(|_| bad("integer out of range"))?;
        Ok((neg, v))
    }

    /// `>> unsigned long` (a leading `-` negates modulo 2^64).
    fn a_ulong(&mut self) -> Result<u64, Rejected> {
        let (neg, v) = self.int_token()?;
        Ok(if neg { v.wrapping_neg() } else { v })
    }

    /// `>> long` (or a narrower signed type of `bits` bits).
    fn a_signed(&mut self, bits: u32) -> Result<i64, Rejected> {
        let (neg, v) = self.int_token()?;
        let max = (1u64 << (bits - 1)) - 1;
        if neg {
            if v > max + 1 {
                return Err(bad("integer out of range"));
            }
            Ok((v as i64).wrapping_neg())
        } else {
            if v > max {
                return Err(bad("integer out of range"));
            }
            Ok(v as i64)
        }
    }

    /// `>> double`.
    fn a_double(&mut self) -> Result<f64, Rejected> {
        let t = self.token()?;
        let ok = {
            let mut i = 0;
            if matches!(t.first(), Some(b'+' | b'-')) {
                i += 1;
            }
            let mut mant = 0;
            while t.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
                mant += 1;
            }
            if t.get(i) == Some(&b'.') {
                i += 1;
                while t.get(i).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                    mant += 1;
                }
            }
            if mant > 0 && matches!(t.get(i), Some(b'e' | b'E')) {
                i += 1;
                if matches!(t.get(i), Some(b'+' | b'-')) {
                    i += 1;
                }
                let mut ex = 0;
                while t.get(i).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                    ex += 1;
                }
                mant = if ex > 0 { mant } else { 0 };
            }
            mant > 0 && i == t.len()
        };
        if !ok {
            return Err(bad("not a plain number"));
        }
        let text = core::str::from_utf8(t).map_err(|_| bad("not a plain number"))?;
        match text.parse::<f64>() {
            Ok(v) if v.is_finite() => Ok(v),
            _ => Err(bad("number out of range")),
        }
    }

    /// `ignore(256, '\n')`.
    fn next_line(&mut self) {
        if self.eof {
            return;
        }
        let mut n = 0;
        while n < 256 {
            match self.peek() {
                None => {
                    self.eof = true;
                    return;
                }
                Some(c) => {
                    self.pos += 1;
                    n += 1;
                    if c == b'\n' {
                        return;
                    }
                }
            }
        }
    }

    /// `GOTO_NEXT_ATTLINE`: `true` when upstream would return `SM_ERR`.
    fn attline(&mut self) -> Result<bool, Rejected> {
        if self.eof {
            return Err(bad("attribute line after the end of the stream"));
        }
        let mut count = 0u32;
        loop {
            let ch = match self.peek() {
                None => return Err(bad("attribute line runs to the end of the stream")),
                Some(c) => {
                    self.pos += 1;
                    c
                }
            };
            if ch == b'\n' {
                return Ok(false);
            }
            count += 1;
            if count > 1000 || ch == 0xFF {
                return Ok(true);
            }
        }
    }

    // ---- common reads ----------------------------------------------------------------

    fn ulong(&mut self) -> Result<u64, Rejected> {
        if self.binary {
            self.b_u64()
        } else {
            self.a_ulong()
        }
    }

    fn long(&mut self) -> Result<i64, Rejected> {
        if self.binary {
            Ok(self.b_u64()? as i64)
        } else {
            self.a_signed(64)
        }
    }

    fn boolean(&mut self) -> Result<(), Rejected> {
        if self.binary {
            self.take(4).map(|_| ())
        } else {
            self.a_signed(32).map(|_| ())
        }
    }

    fn double(&mut self) -> Result<f64, Rejected> {
        if self.binary {
            self.b_f64()
        } else {
            self.a_double()
        }
    }

    fn count(&mut self) -> Result<u64, Rejected> {
        let c = self.ulong()?;
        if c >= BIG_ULONG {
            return Err(bad("count of SM_BIG_ULONG or more"));
        }
        Ok(c)
    }

    fn lines(&mut self, n: usize) {
        if !self.binary {
            for _ in 0..n {
                self.next_line();
            }
        }
    }

    /// `ReadIndexedAttributesFromDB`; returns the indices and whether the placeholders
    /// were created.
    fn attributes(&mut self) -> Result<(Vec<u64>, bool), Rejected> {
        let n = self.ulong()?;
        if n > MAX_ATTR {
            return Err(bad("attribute count too large"));
        }
        if n == 0 {
            if !self.binary {
                let failed = self.attline()?;
                return Ok((Vec::new(), !failed));
            }
            return Ok((Vec::new(), true));
        }
        let mut idx = Vec::new();
        for _ in 0..n {
            let i = self.ulong()?;
            if i > MAX_ATTR {
                return Err(bad("attribute index too large"));
            }
            idx.push(i);
        }
        if !self.binary && self.attline()? {
            return Ok((idx, false));
        }
        for &i in &idx {
            let i = i as usize;
            if self.table.len() < i + 1 {
                self.table.resize(i + 1, false);
            }
            if let Some(s) = self.table.get_mut(i) {
                *s = true;
            }
        }
        Ok((idx, true))
    }

    fn record_attributes(
        &mut self,
        version: u64,
        persist: bool,
        lists: usize,
    ) -> Result<(), Rejected> {
        if version > 30 && persist {
            for _ in 0..lists {
                self.attributes()?;
            }
        }
        Ok(())
    }

    /// Geometry attributes: every index must address the table (upstream indexes it).
    fn geometry_attributes(&mut self, version: u64, persist: bool) -> Result<(), Rejected> {
        if version > 30 && persist {
            let (idx, _) = self.attributes()?;
            if idx.iter().any(|&i| i as usize >= self.table.len()) {
                return Err(bad("geometry attribute index past the attribute table"));
            }
        }
        Ok(())
    }

    fn topology(&mut self, version: u64) -> Result<(), Rejected> {
        if version > 31 {
            self.ulong()?;
            self.ulong()?;
            self.ulong()?;
        }
        Ok(())
    }

    // ---- the B-rep -------------------------------------------------------------------

    fn file(&mut self) -> Result<(), Rejected> {
        // (char)get(), a second get() after a newline, one unget()
        let mut c = self.peek();
        if c.is_none() {
            return Err(bad("empty file"));
        }
        if c == Some(10) {
            c = self.data.get(1).copied();
            if c.is_some() {
                self.pos = 1;
            }
        }
        if c.is_none() {
            return Err(bad("file ends after the first newline"));
        }
        let version = if c != Some(b'V') {
            30
        } else if self.binary {
            self.take(1)?;
            self.b_u64()?
        } else {
            self.word(8)?;
            let v = self.a_ulong()?;
            self.next_line();
            v
        };
        if version > 41 {
            if self.binary {
                self.take(1)?;
                self.b_u64()?;
            } else {
                self.word(8)?;
                self.word(8)?;
                self.a_ulong()?;
                self.next_line();
            }
        }
        self.brep(version, false)
    }

    fn brep(&mut self, version: u64, persist: bool) -> Result<(), Rejected> {
        let attrs = version > 30 && persist;
        let ascii = !self.binary;
        // properties
        self.lines(1);
        if attrs {
            self.attributes()?;
        } else {
            self.double()?;
            self.double()?;
            self.double()?;
            self.lines(1);
        }
        if version <= 43 {
            self.double()?;
        }
        self.lines(1);
        self.boolean()?;
        self.lines(1);
        self.boolean()?;
        self.lines(3);
        let regions = self.count()?;
        self.lines(1);
        self.ulong()?;
        self.lines(2);
        for _ in 0..regions {
            if ascii {
                self.ulong()?;
            }
            self.topology(version)?;
            self.ulong()?;
            self.ulong()?;
            self.boolean()?;
            self.lines(1);
            self.record_attributes(version, persist, 1)?;
        }
        // shells
        self.lines(2);
        let n = self.count()?;
        self.lines(4);
        for _ in 0..n {
            if ascii {
                self.ulong()?;
            }
            self.topology(version)?;
            let ty = self.ulong()?;
            match ty {
                0 => {
                    if ascii {
                        self.boolean()?;
                        self.ulong()?;
                        self.ulong()?;
                    } else {
                        self.ulong()?;
                        self.ulong()?;
                        self.boolean()?;
                    }
                }
                1 | 2 => {
                    self.ulong()?;
                }
                _ => {}
            }
            self.lines(1);
            self.record_attributes(version, persist, 1)?;
        }
        self.composite_list()?;
        // faceuses
        self.lines(2);
        let n = self.count()?;
        self.lines(2);
        for _ in 0..n {
            if ascii {
                self.ulong()?;
            }
            self.topology(version)?;
            self.ulong()?;
            self.boolean()?;
            self.lines(1);
            self.record_attributes(version, persist, 1)?;
        }
        // faces
        self.lines(2);
        let n = self.count()?;
        self.lines(3);
        for _ in 0..n {
            if ascii {
                self.ulong()?;
                self.topology(version)?;
                self.double()?;
                self.ulong()?;
                self.ulong()?;
                self.ulong()?;
                self.boolean()?;
                self.lines(1);
                for _ in 0..4 {
                    self.double()?;
                }
                self.lines(1);
            } else {
                self.topology(version)?;
                for _ in 0..5 {
                    self.double()?;
                }
                self.ulong()?;
                self.ulong()?;
                self.ulong()?;
                self.boolean()?;
            }
            self.record_attributes(version, persist, 1)?;
        }
        // loops
        self.lines(2);
        let n = self.count()?;
        self.lines(3);
        for _ in 0..n {
            if ascii {
                self.ulong()?;
            }
            self.topology(version)?;
            match self.ulong()? {
                0 => {
                    self.ulong()?;
                    self.ulong()?;
                }
                1 => {
                    self.ulong()?;
                }
                _ => {}
            }
            self.lines(1);
            self.record_attributes(version, persist, 3)?;
        }
        self.composite_list()?;
        // edge uses
        self.lines(2);
        let n = self.count()?;
        self.lines(5);
        for _ in 0..n {
            if ascii {
                self.ulong()?;
            }
            self.topology(version)?;
            if version > 31 {
                self.ulong()?;
                self.ulong()?;
                self.ulong()?;
            }
            let ty = self.ulong()?;
            if ascii {
                match ty {
                    0 => {
                        self.ulong()?;
                        self.ulong()?;
                        self.boolean()?;
                        self.ulong()?;
                        self.long()?;
                        self.long()?;
                    }
                    1..=3 => {
                        self.ulong()?;
                        self.ulong()?;
                        self.boolean()?;
                        self.ulong()?;
                    }
                    _ => {}
                }
                self.next_line();
            } else {
                if ty <= 3 {
                    self.ulong()?;
                    self.ulong()?;
                }
                self.boolean()?;
                self.ulong()?;
                if ty == 0 {
                    self.long()?;
                    self.long()?;
                }
            }
            self.record_attributes(version, persist, 2)?;
        }
        // edges
        self.lines(2);
        let n = self.count()?;
        self.lines(2);
        for _ in 0..n {
            if ascii {
                self.ulong()?;
            }
            self.topology(version)?;
            self.double()?;
            self.ulong()?;
            self.ulong()?;
            self.ulong()?;
            self.long()?;
            self.double()?;
            self.double()?;
            self.lines(1);
            self.record_attributes(version, persist, 1)?;
        }
        // vertices
        self.lines(2);
        let n = self.count()?;
        self.lines(2);
        for _ in 0..n {
            if ascii {
                self.ulong()?;
            }
            self.topology(version)?;
            for _ in 0..4 {
                self.double()?;
            }
            self.lines(1);
            self.record_attributes(version, persist, 1)?;
        }
        // UV curves, 3D curves
        for _dim in [2, 3] {
            self.lines(2);
            let n = self.count()?;
            self.lines(1);
            for _ in 0..n {
                if ascii {
                    self.ulong()?;
                    self.next_line();
                }
                if self.curve(version, persist)? == Read::Failed {
                    return Ok(());
                }
            }
        }
        // surfaces
        self.lines(2);
        let n = self.count()?;
        self.lines(1);
        for _ in 0..n {
            if ascii {
                self.ulong()?;
                self.next_line();
            }
            if self.surface(version, persist)? == Read::Failed {
                return Ok(());
            }
        }
        Ok(())
    }

    fn composite_list(&mut self) -> Result<(), Rejected> {
        self.lines(2);
        let n = self.count()?;
        self.lines(1);
        if n != 0 {
            self.lines(1);
            for _ in 0..n {
                if self.binary {
                    let m = self.count()?;
                    for _ in 0..m {
                        self.ulong()?;
                    }
                } else {
                    self.ulong()?;
                    let m = self.count()?;
                    for _ in 0..m {
                        self.ulong()?;
                    }
                    self.next_line();
                }
            }
        }
        Ok(())
    }

    // ---- geometry --------------------------------------------------------------------

    /// `ReadType`: the type code (and dimension).
    fn read_type(&mut self, with_dim: bool) -> Result<(i64, u64), Rejected> {
        if self.binary {
            let t = self.b_u64()? as i64;
            let d = if with_dim { self.b_u64()? } else { 0 };
            Ok((t, d))
        } else {
            self.word(256)?;
            self.word(256)?;
            let t = self.a_signed(64)?;
            self.next_line();
            let mut d = 0;
            if with_dim {
                self.word(256)?;
                self.word(256)?;
                d = self.a_ulong()?;
                self.next_line();
            }
            Ok((t, d))
        }
    }

    fn curve(&mut self, version: u64, persist: bool) -> Result<Read, Rejected> {
        if version > 32 {
            let (ty, _read_dim) = self.read_type(true)?;
            match ty {
                14033 => self.bspline_curve(version)?,
                1 => return Ok(Read::Failed),
                _ if self.trust => return Ok(Read::Failed),
                _ => return Err(bad("curve type the guard does not know")),
            }
        } else {
            let (ty, _) = self.read_type(false)?;
            match ty {
                14033 => self.bspline_curve(version)?,
                14032 => {
                    if self.depth >= MAX_DEPTH {
                        return Err(bad("composite curves nested too deeply"));
                    }
                    let n = self.ulong()?;
                    self.ulong()?;
                    let closed = self.ulong()?;
                    self.lines(1);
                    if n >= BIG_ULONG {
                        return Err(bad("segment count too large"));
                    }
                    if n == 0 && closed != 0 {
                        return Err(bad("closed composite curve without segments"));
                    }
                    self.depth += 1;
                    for _ in 0..n {
                        self.ulong()?;
                        self.double()?;
                        self.double()?;
                        self.lines(1);
                        if self.curve(version, persist)? == Read::Failed {
                            self.depth -= 1;
                            return Ok(Read::Failed);
                        }
                    }
                    self.depth -= 1;
                }
                _ => return Ok(Read::Failed),
            }
        }
        self.geometry_attributes(version, persist)?;
        Ok(Read::Done)
    }

    /// `SmBSplineCurve::ReadFromDB`.
    fn bspline_curve(&mut self, version: u64) -> Result<(), Rejected> {
        let (n, p, rat, read_dim) = if self.binary {
            let n = self.b_u64()? as i64;
            let p = i64::from(self.b_i16()?);
            let rat = i64::from(self.b_i16()?);
            let d = i64::from(self.b_i16()?);
            (n, p, rat, d)
        } else {
            let n = self.a_signed(64)?;
            self.next_line();
            let p = self.a_signed(16)?;
            self.next_line();
            let rat = self.a_signed(16)?;
            self.next_line();
            let d = self.a_signed(16)?;
            self.next_line();
            (n, p, rat, d)
        };
        if version > 32 {
            let kt = self.ulong()?;
            self.lines(1);
            let form = self.ulong()?;
            self.lines(1);
            if kt > 4 || form > 6 {
                return Err(bad("knot type or curve form out of range"));
            }
        }
        if !(1..=MAX_POINTS).contains(&n) || !(1..=MAX_DEGREE.min(n)).contains(&p) {
            return Err(bad("B-spline curve header out of range"));
        }
        if !(rat == 0 || rat == 1) || !(read_dim == 2 || read_dim == 3) {
            return Err(bad(
                "B-spline curve rational flag or dimension out of range",
            ));
        }
        let per = read_dim as usize + rat as usize;
        for _ in 0..=n {
            for k in 0..per {
                let v = self.double()?;
                if rat == 1 && k + 1 == per && v <= 0.0 {
                    return Err(bad("non-positive weight"));
                }
            }
            self.lines(1);
        }
        let mut knots = Vec::new();
        for _ in 0..(n + p + 2) {
            knots.push(self.double()?);
            self.lines(1);
        }
        check_knots(&knots, p as usize, n as usize)
    }

    fn surface(&mut self, version: u64, persist: bool) -> Result<Read, Rejected> {
        let (ty, _) = self.read_type(false)?;
        if version > 32 {
            match ty {
                15002 => self.bspline_surface(version)?,
                1 => return Ok(Read::Failed),
                _ if self.trust => return Ok(Read::Failed),
                _ => return Err(bad("surface type the guard does not know")),
            }
        } else {
            match ty {
                15002 => self.bspline_surface(version)?,
                15400 => {
                    self.double()?;
                    self.lines(1);
                    self.bspline_surface(version)?;
                }
                _ => return Ok(Read::Failed),
            }
        }
        if version > 30 && persist {
            // ReadSurfaceFromDB skips empty slots but indexes the table
            self.geometry_attributes(version, persist)?;
        }
        Ok(Read::Done)
    }

    fn bspline_surface(&mut self, version: u64) -> Result<(), Rejected> {
        let (n, m, p, q, rat) = if self.binary {
            let n = self.b_u64()? as i64;
            let m = self.b_u64()? as i64;
            let p = i64::from(self.b_i16()?);
            let q = i64::from(self.b_i16()?);
            let rat = i64::from(self.b_i16()?);
            (n, m, p, q, rat)
        } else {
            let n = self.a_signed(64)?;
            let m = self.a_signed(64)?;
            self.next_line();
            let p = self.a_signed(16)?;
            let q = self.a_signed(16)?;
            self.next_line();
            let rat = self.a_signed(16)?;
            self.next_line();
            (n, m, p, q, rat)
        };
        if version > 32 {
            let kt = self.ulong()?;
            self.lines(1);
            let form = self.ulong()?;
            self.lines(1);
            if kt > 4 || form > 11 {
                return Err(bad("knot type or surface form out of range"));
            }
        }
        if !(1..=1000).contains(&n)
            || !(1..=1000).contains(&m)
            || !(1..=MAX_DEGREE.min(n)).contains(&p)
            || !(1..=MAX_DEGREE.min(m)).contains(&q)
        {
            return Err(bad("B-spline surface header out of range"));
        }
        if !(rat == 0 || rat == 1) {
            return Err(bad("B-spline surface rational flag out of range"));
        }
        let per = 3 + rat as usize;
        for _ in 0..(n + 1) * (m + 1) {
            for k in 0..per {
                let v = self.double()?;
                if rat == 1 && k + 1 == per && v <= 0.0 {
                    return Err(bad("non-positive weight"));
                }
            }
            self.lines(1);
        }
        let mut u = Vec::new();
        for _ in 0..(n + p + 2) {
            u.push(self.double()?);
            self.lines(1);
        }
        let mut v = Vec::new();
        for _ in 0..(m + q + 2) {
            v.push(self.double()?);
            self.lines(1);
        }
        check_knots(&u, p as usize, n as usize)?;
        check_knots(&v, q as usize, m as usize)
    }
}

/// Non-decreasing knots with a non-degenerate domain `[U[p], U[n+1]]`.
fn check_knots(knots: &[f64], p: usize, n: usize) -> Result<(), Rejected> {
    if knots.windows(2).any(|w| w.first() > w.get(1)) {
        return Err(bad("decreasing knots"));
    }
    match (knots.get(p), knots.get(n + 1)) {
        (Some(a), Some(b)) if a < b => Ok(()),
        _ => Err(bad("degenerate knot domain")),
    }
}
