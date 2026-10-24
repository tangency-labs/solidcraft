//! Runner for unit `topology.brep_data`: reading serialized B-reps, curves and
//! surfaces (`SmBrepData`).
//!
//! C++: `solidcore_ffi::units::topology_brep_data` (upstream
//! `source/SMLib/src/SmBrepData.cpp`). Rust: `solidcore::topology::brep_data`.
//!
//! A case is one read (`mode`: `db` = `ReadFromDB`, `file` = `ReadFromFile`, `curve` =
//! `ReadCurveFromDB`, `surface` = `ReadSurfaceFromDB`) of an ASCII (`text`) or binary
//! (`data`) stream, or an upstream test file (`path`, optionally without its first
//! `skip_lines` lines), or a construction (`construct`). Both sides dump the result
//! in the layout documented in the FFI module: integers go to `ints`, doubles to
//! `dbls` (subnormals as 0) and bit for bit to `dbl_bits`, and `kinds` records the
//! interleaving. Statuses are upstream `SmStatus` values (`0` = success); a Rust
//! `InvalidInput` maps to `-1` (the C++ guard keeps such inputs away from upstream,
//! so it never matches).
//!
//! Generated cases build random B-rep, curve and surface streams in both formats for
//! database versions 29 to 50 (every branch: attributes or not, the version 31 / 32 /
//! 43 / 41 switches, unknown record types, composite faces and edges, old composite
//! curves, offset surfaces, non-geometry types, attribute tables with empty slots,
//! extreme and inverted values, long ASCII lines). Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::attribute::{Attribute, AttributeHandle, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::vector2d::Point2;
use solidcore::topology::brep_data::{
    read_curve_from_db, read_surface_from_db, AttributeSink, BrepData, DataSourceType, Keyed,
    TopologyData,
};
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::{curve::curve::Curve, surface::surface::Surface, Kernel, SolidError};
use solidcore_ffi::units::topology_brep_data::{self as ffi, Item, Mode, Request};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `topology.brep_data` runner.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    mode: String,
    #[serde(default)]
    binary: bool,
    #[serde(default)]
    version: u64,
    #[serde(default)]
    dim: u64,
    #[serde(default)]
    persist: bool,
    #[serde(default)]
    source: i32,
    #[serde(default)]
    pre: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default)]
    skip_lines: usize,
}

fn harness(m: impl Into<String>) -> HarnessError {
    HarnessError::new(m.into())
}

fn mode_of(i: &Inputs) -> Result<Mode, HarnessError> {
    Ok(match i.mode.as_str() {
        "db" => Mode::ReadDb,
        "file" => Mode::ReadFile,
        "curve" => Mode::ReadCurve,
        "surface" => Mode::ReadSurface,
        "construct" => Mode::Construct,
        other => return Err(harness(format!("unknown mode {other}"))),
    })
}

fn bytes_of(i: &Inputs) -> Result<Vec<u8>, HarnessError> {
    if let Some(p) = &i.path {
        let all = std::fs::read(p).map_err(|e| harness(format!("{p}: {e}")))?;
        let mut start = 0;
        for _ in 0..i.skip_lines {
            match all.get(start..).and_then(|s| s.iter().position(|&c| c == b'\n')) {
                Some(k) => start += k + 1,
                None => return Err(harness("skip_lines past the end of the file")),
            }
        }
        return Ok(all.get(start..).unwrap_or(&[]).to_vec());
    }
    if let Some(t) = &i.text {
        return Ok(t.as_bytes().to_vec());
    }
    Ok(i.data.clone().unwrap_or_default())
}

fn to_output(items: &[Item]) -> Output {
    let ints: Vec<i64> = items
        .iter()
        .filter_map(|x| match x {
            Item::Int(v) => Some(*v),
            Item::Dbl(_) => None,
        })
        .collect();
    let dbls: Vec<f64> = items
        .iter()
        .filter_map(|x| match x {
            Item::Dbl(v) => Some(*v),
            Item::Int(_) => None,
        })
        .collect();
    let kinds: Vec<i64> = items
        .iter()
        .map(|x| i64::from(matches!(x, Item::Dbl(_))))
        .collect();
    // Subnormals are compared through their bits only: a relative perturbation
    // cannot change them, so `dbls` holds 0 in their place.
    let bits: Vec<i64> = dbls.iter().map(|v| v.to_bits() as i64).collect();
    let dbls: Vec<f64> = dbls
        .into_iter()
        .map(|v| if v.is_subnormal() { 0.0 } else { v })
        .collect();
    Output::new()
        .ints("ints", ints)
        .values("dbls", dbls)
        .ints("dbl_bits", bits)
        .ints("kinds", kinds)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let req = Request {
        mode: mode_of(i)?,
        binary: i.binary,
        version: i.version,
        dim: i.dim,
        persist: i.persist,
        source: i.source,
        pre: i.pre,
        data: bytes_of(i)?,
        upstream_file: i.path.is_some(),
    };
    match ffi::run(&req) {
        Ok(items) => Ok(to_output(&items)),
        Err(e @ FfiError::InvalidArgument(_)) => Err(harness(format!("ffi rejected: {e}"))),
        Err(e) => Ok(Output::err(e.to_string())),
    }
}

// ---- the Rust side --------------------------------------------------------------------

/// Upstream status of a Rust error.
fn status(e: &SolidError) -> i64 {
    match e {
        SolidError::Upstream { code, .. } => i64::from(*code),
        _ => -1,
    }
}

struct Dump<'a> {
    o: Vec<Item>,
    store: &'a AttributeStore,
    all: &'a [Option<AttributeHandle>],
}

impl Dump<'_> {
    fn i(&mut self, v: i64) {
        self.o.push(Item::Int(v));
    }
    fn u(&mut self, v: u64) {
        self.o.push(Item::Int(v as i64));
    }
    fn d(&mut self, v: f64) {
        self.o.push(Item::Dbl(v));
    }
    fn list(&mut self, l: &[u64]) {
        self.u(l.len() as u64);
        for &x in l {
            self.u(x);
        }
    }

    fn obj_attributes(&mut self, key: ObjectKey) {
        let attrs = self.store.attributes(key);
        self.u(attrs.len() as u64);
        for h in attrs {
            let slot = self
                .all
                .iter()
                .position(|s| *s == Some(h))
                .map_or(-1, |p| p as i64);
            self.i(slot);
        }
    }

    fn curve(&mut self, c: &Keyed<Curve>) {
        let k = &Kernel::default();
        self.i(1);
        let cv = &c.object;
        self.i(cv.type_code());
        self.u(cv.dim());
        let ivl = cv.natural_interval();
        self.d(ivl.min());
        self.d(ivl.max());
        let ts = [ivl.min(), 0.5 * (ivl.min() + ivl.max()), ivl.max()];
        for t in ts {
            match cv.evaluate_point(k, t) {
                Ok(p) => {
                    self.i(0);
                    self.d(p.x);
                    self.d(p.y);
                    self.d(p.z);
                }
                Err(e) => self.i(status(&e)),
            }
        }
        self.obj_attributes(c.key);
    }

    fn surface(&mut self, s: &Keyed<Surface>) {
        let k = &Kernel::default();
        self.i(1);
        let sf = &s.object;
        self.i(sf.type_code());
        let dom = sf.natural_uv_domain();
        let (a, b) = (dom.min(), dom.max());
        self.d(a.x);
        self.d(a.y);
        self.d(b.x);
        self.d(b.y);
        for f in [0.0, 0.5, 1.0] {
            let uv = Point2::new(a.x + f * (b.x - a.x), a.y + f * (b.y - a.y));
            match sf.evaluate_point(k, uv) {
                Ok(p) => {
                    self.i(0);
                    self.d(p.x);
                    self.d(p.y);
                    self.d(p.z);
                }
                Err(e) => self.i(status(&e)),
            }
        }
        self.obj_attributes(s.key);
    }

    fn topo(&mut self, t: &TopologyData) {
        self.list(&t.attributes);
        self.u(t.flags);
        self.u(t.user_index1);
        self.u(t.user_index2);
        self.i(i64::from(t.is_small_topology));
    }

    fn header(&mut self, b: &BrepData) {
        self.i(i64::from(b.data_source_type.code()));
        self.i(i64::from(b.manifold));
        self.d(b.zone_tol_3d);
        self.d(b.model_size_estimate);
        self.d(b.large_small_size_ratio);
        self.d(b.approx_tol_3d);
        self.u(b.start_region);
        self.u(b.num_regions);
        self.d(b.color.x);
        self.d(b.color.y);
        self.d(b.color.z);
        self.i(i64::from(b.is_geom_borrowed));
        self.u(b.healer_version());
    }

    fn brep(&mut self, b: &BrepData) {
        self.header(b);
        self.list(&b.attributes);
        self.u(b.regions.len() as u64);
        for r in &b.regions {
            self.topo(&r.topology);
            self.u(r.start_shell);
            self.u(r.num_shells);
            self.i(i64::from(r.is_void));
        }
        self.u(b.shells.len() as u64);
        for r in &b.shells {
            self.topo(&r.topology);
            self.u(r.shell_type);
            self.u(r.vertex);
            self.u(r.edge);
            self.u(r.faceuse_start);
            self.u(r.num_faceuses);
        }
        self.list(&b.cfaces);
        self.u(b.faceuses.len() as u64);
        for r in &b.faceuses {
            self.topo(&r.topology);
            self.u(r.face);
            self.i(i64::from(r.orientation));
        }
        self.u(b.faces.len() as u64);
        for r in &b.faces {
            self.topo(&r.topology);
            self.d(r.zone_tol_3d);
            self.u(r.surface);
            self.u(r.start_loop);
            self.u(r.num_loops);
            self.i(i64::from(r.rectangular_trim));
            self.d(r.uv_domain.min().x);
            self.d(r.uv_domain.min().y);
            self.d(r.uv_domain.max().x);
            self.d(r.uv_domain.max().y);
        }
        self.u(b.loops.len() as u64);
        for r in &b.loops {
            self.topo(&r.topology);
            self.u(r.loop_type);
            self.u(r.vertex);
            self.u(r.start_eu);
            self.u(r.num_eu);
            self.list(&r.lu_upper_attributes);
            self.list(&r.lu_lower_attributes);
        }
        self.list(&b.cedges);
        self.u(b.edgeuses.len() as u64);
        for r in &b.edgeuses {
            self.topo(&r.topology);
            self.u(r.eu_type);
            self.u(r.edge);
            self.u(r.vertex);
            self.u(r.shell);
            self.u(r.loop_);
            self.i(i64::from(r.orientation));
            self.u(r.uv_curve);
            self.i(r.next_eu);
            self.i(r.mate_next_eu);
            self.i(i64::from(r.downward_lu));
            self.u(r.mate_flags);
            self.u(r.mate_user_index1);
            self.u(r.mate_user_index2);
            self.list(&r.mate_attributes);
        }
        self.u(b.edges.len() as u64);
        for r in &b.edges {
            self.topo(&r.topology);
            self.d(r.zone_tol_3d);
            self.d(r.interval.min());
            self.d(r.interval.max());
            self.u(r.curve);
            self.u(r.start_vertex);
            self.u(r.end_vertex);
            self.i(r.prim_eu);
        }
        self.u(b.vertices.len() as u64);
        for r in &b.vertices {
            self.topo(&r.topology);
            self.d(r.zone_tol_3d);
            self.d(r.point.x);
            self.d(r.point.y);
            self.d(r.point.z);
        }
        self.u(b.uv_curves.len() as u64);
        for c in &b.uv_curves {
            self.curve(c);
        }
        self.u(b.curves_3d.len() as u64);
        for c in &b.curves_3d {
            self.curve(c);
        }
        self.u(b.surfaces.len() as u64);
        for s in &b.surfaces {
            self.surface(s);
        }
    }

    fn table(&mut self) -> Result<(), HarnessError> {
        let all = self.all;
        self.u(all.len() as u64);
        for slot in all {
            match slot {
                None => self.i(0),
                Some(h) => {
                    let a = self
                        .store
                        .get(*h)
                        .ok_or_else(|| harness("attribute table holds a dead handle"))?;
                    let users = self
                        .store
                        .users(*h)
                        .map_err(|e| harness(e.to_string()))?
                        .len();
                    let (id, behavior) = (a.attribute_id(), a.behavior().code());
                    self.i(1);
                    self.u(id);
                    self.i(i64::from(behavior));
                    self.u(users as u64);
                }
            }
        }
        Ok(())
    }
}

/// Ok(dump items) or the Rust error (an `Unported` one defers the case).
fn rust(i: &Inputs) -> Result<Result<Vec<Item>, SolidError>, HarnessError> {
    enum Got {
        Brep(BrepData),
        Curve(Keyed<Curve>),
        Surface(Keyed<Surface>),
    }

    let k = Kernel::default();
    let mode = mode_of(i)?;
    let data = bytes_of(i)?;
    let file_type = if i.binary {
        FileType::Binary
    } else {
        FileType::Ascii
    };
    let empty_store = AttributeStore::new();
    if mode == Mode::Construct {
        let source = DataSourceType::from_code(i.source)
            .ok_or_else(|| harness("data source type out of range"))?;
        // the shim's `(SmBoolean)dim`: an `int64_t` to `int` conversion, which keeps the
        // low 32 bits (GCC), as `as i32` does
        let manifold = i.dim as i64 as i32;
        let mut bd = BrepData::new(manifold, source);
        let mut d = Dump {
            o: Vec::new(),
            store: &empty_store,
            all: &[],
        };
        d.i(0);
        d.header(&bd);
        bd.set_healer_version(i.version);
        d.u(bd.healer_version());
        d.u(bd.regions.len() as u64);
        d.u(bd.uv_curves.len() as u64);
        return Ok(Ok(d.o));
    }
    if mode == Mode::ReadFile {
        let mut d = Dump {
            o: Vec::new(),
            store: &empty_store,
            all: &[],
        };
        match BrepData::read_from_file(&k, data, file_type) {
            Ok(bd) => {
                d.i(0);
                d.brep(&bd);
            }
            Err(e @ SolidError::Unported { .. }) => return Ok(Err(e)),
            Err(e) => d.i(status(&e)),
        }
        return Ok(Ok(d.o));
    }

    // the FFI refuses `pre > 64` (its validate); mirror it so a fuzzed count cannot make
    // the runner build billions of attributes
    if i.pre > 64 {
        return Err(harness("pre > 64"));
    }
    let mut store = AttributeStore::new();
    let mut all: Vec<Option<AttributeHandle>> = Vec::new();
    for k in 0..i.pre {
        if k % 2 == 0 {
            let h = store
                .insert(Attribute::new(1000 + k, Behavior::StandaloneReference))
                .map_err(|e| harness(e.to_string()))?;
            all.push(Some(h));
        } else {
            all.push(None);
        }
    }
    let mut db = DatabaseReader::new();
    db.open_for_read(data, file_type);
    db.set_version(i.version);

    let got = {
        let mut sink = AttributeSink::new(&mut store, &mut all, 1);
        match mode {
            Mode::ReadDb => {
                BrepData::read_from_db(&k, i.version, &mut sink, &mut db, i.persist).map(Got::Brep)
            }
            Mode::ReadCurve => {
                read_curve_from_db(&k, i.dim, i.version, &mut sink, &mut db, i.persist)
                    .map(Got::Curve)
            }
            _ => read_surface_from_db(&k, i.version, &mut sink, &mut db, i.persist)
                .map(Got::Surface),
        }
    };
    let mut d = Dump {
        o: Vec::new(),
        store: &store,
        all: &all,
    };
    match &got {
        Ok(g) => {
            d.i(0);
            match g {
                Got::Brep(b) => d.brep(b),
                Got::Curve(c) => d.curve(c),
                Got::Surface(s) => d.surface(s),
            }
        }
        Err(e @ SolidError::Unported { .. }) => return Ok(Err(e.clone())),
        Err(e) => d.i(status(e)),
    }
    d.table()?;
    Ok(Ok(d.o))
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_data"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        match rust(&case.parse()?)? {
            Ok(items) => Ok(to_output(&items)),
            Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string()))),
        }
    }

    fn skipped_platform(&self, case: &Case) -> Option<String> {
        let i: Inputs = case.parse().ok()?;
        if i.mode == "db" {
            Some("libc++ istream extraction differs from libstdc++".into())
        } else {
            None
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(generate(rng)).ok()
    }
}

// ---- generated streams ------------------------------------------------------------------

const UNDEF_ULONG: u64 = 123_456_789;
const NO_OBJECT: u64 = 9_999_999;

/// Writes a database stream in either format.
struct W {
    binary: bool,
    buf: Vec<u8>,
    /// ASCII: something was written on the current line.
    mid: bool,
}

impl W {
    fn sep(&mut self) {
        if self.mid {
            self.buf.push(b' ');
        }
        self.mid = true;
    }
    fn text(&mut self, s: &str) {
        if !self.binary {
            self.sep();
            self.buf.extend(s.as_bytes());
        }
    }
    /// ASCII line end (with optional trailing text).
    fn eol(&mut self) {
        if !self.binary {
            self.buf.push(b'\n');
            self.mid = false;
        }
    }
    fn line(&mut self, s: &str) {
        if !self.binary {
            if self.mid {
                self.eol();
            }
            self.buf.extend(s.as_bytes());
            self.eol();
        }
    }
    fn ul(&mut self, v: u64) {
        if self.binary {
            self.buf.extend(v.to_ne_bytes());
        } else {
            self.sep();
            self.buf.extend(v.to_string().as_bytes());
        }
    }
    /// An unsigned value written as `-k` in ASCII (`k.wrapping_neg()` in binary).
    fn ul_neg(&mut self, k: u64) {
        if self.binary {
            self.buf.extend(k.wrapping_neg().to_ne_bytes());
        } else {
            self.sep();
            self.buf.extend(format!("-{k}").as_bytes());
        }
    }
    fn lg(&mut self, v: i64) {
        if self.binary {
            self.buf.extend(v.to_ne_bytes());
        } else {
            self.sep();
            self.buf.extend(v.to_string().as_bytes());
        }
    }
    fn bo(&mut self, v: i32) {
        if self.binary {
            self.buf.extend(v.to_ne_bytes());
        } else {
            self.sep();
            self.buf.extend(v.to_string().as_bytes());
        }
    }
    fn sh(&mut self, v: i16) {
        if self.binary {
            self.buf.extend(v.to_ne_bytes());
        } else {
            self.sep();
            self.buf.extend(v.to_string().as_bytes());
        }
    }
    fn db(&mut self, v: f64) {
        if self.binary {
            self.buf.extend(v.to_ne_bytes());
        } else {
            self.sep();
            self.buf.extend(format!("{v:e}").as_bytes());
        }
    }
    fn ch(&mut self, c: u8) {
        if self.binary {
            self.buf.push(c);
        }
    }
}

struct G<'r> {
    rng: &'r mut Rng,
    w: W,
    version: u64,
    persist: bool,
}

impl G<'_> {
    fn chance(&mut self, p: f64) -> bool {
        self.rng.chance(p)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.rng.below(n)
    }

    /// An index-like unsigned value.
    fn index(&mut self) {
        match self.below(20) {
            0 => self.w.ul(UNDEF_ULONG),
            1 => self.w.ul(NO_OBJECT),
            2 => self.w.ul_neg(1),
            3 => self.w.ul(u64::MAX - self.rng.below(3)),
            4 => self.w.ul(1 << 63),
            _ => {
                let v = self.below(12);
                self.w.ul(v);
            }
        }
    }

    fn signed(&mut self) {
        let v = match self.below(8) {
            0 => i64::MIN,
            1 => i64::MAX,
            2 => -(self.rng.below(10) as i64) - 1,
            3 => UNDEF_ULONG as i64,
            _ => self.rng.below(12) as i64,
        };
        self.w.lg(v);
    }

    fn boolean(&mut self) {
        let v = match self.below(10) {
            0 => i32::MIN,
            1 => i32::MAX,
            2 => -1,
            3 => 2,
            _ => self.rng.below(2) as i32,
        };
        self.w.bo(v);
    }

    /// A double of varied scale (subnormals and non-finite values only in binary).
    fn value(&mut self) -> f64 {
        match self.below(16) {
            0 => 0.0,
            1 => -0.0,
            2 => self.rng.uniform(-1.0, 1.0) * 1e300,
            3 => self.rng.uniform(-1.0, 1.0) * 1e-300,
            4 => 5e-324 * (1 + self.rng.below(1000)) as f64,
            5 => self.rng.range_i64(-100, 100) as f64,
            6 => self.rng.uniform(-1.0, 1.0) * 1e6,
            7 => self.rng.uniform(-1.0, 1.0) * 1e-6,
            8 if self.w.binary => f64::NAN,
            9 if self.w.binary => f64::INFINITY,
            _ => self.rng.uniform(-10.0, 10.0),
        }
    }

    fn double(&mut self) {
        let v = self.value();
        self.w.db(v);
    }

    /// An attribute index list (ASCII: one line, optionally with long trailing blanks
    /// when `junk_ok`; 1001 blanks make upstream's line skip fail).
    fn attrs(&mut self, junk_ok: bool) {
        let n = match self.below(6) {
            0..=2 => 0,
            3 => 1,
            _ => 1 + self.below(4),
        };
        self.w.ul(n);
        for _ in 0..n {
            let i = self.below(12);
            self.w.ul(i);
        }
        if !self.w.binary && junk_ok && self.chance(0.05) {
            let blanks = if self.chance(0.5) { 1001 } else { 300 };
            self.w.buf.extend(std::iter::repeat_n(b' ', blanks));
        }
        self.w.eol();
    }

    fn record_attrs(&mut self, lists: usize, junk_ok: bool) {
        if self.version > 30 && self.persist {
            for _ in 0..lists {
                self.attrs(junk_ok);
            }
        }
    }

    fn topo(&mut self) {
        if self.version > 31 {
            self.index();
            self.index();
            self.index();
        }
    }

    /// Trailing blanks after a record line (ASCII; only before another record).
    fn trailing(&mut self, last: bool) {
        if !self.w.binary && !last && self.chance(0.03) {
            self.w.buf.extend(std::iter::repeat_n(b' ', 300));
        }
    }

    fn count(&mut self, max: u64) -> u64 {
        self.below(max + 1)
    }

    fn section(&mut self, title: &str, comments: usize, n: u64) {
        self.w.line("");
        self.w.line(&format!("//{title}"));
        self.w.ul(n);
        self.w.text(&format!("{title} In Brep"));
        self.w.eol();
        for k in 0..comments {
            self.w.line(&format!("//columns {k}"));
        }
    }

    fn brep(&mut self) -> bool {
        let attrs = self.version > 30 && self.persist;
        self.w.line("//Brep Starts");
        if attrs {
            self.attrs(false);
        } else {
            self.double();
            self.double();
            self.double();
            self.w.text("- Color");
            self.w.eol();
        }
        if self.version <= 43 {
            self.double();
        }
        self.w.text("- Brep Tolerance");
        self.w.eol();
        self.boolean();
        self.w.text("- Manifold");
        self.w.eol();
        self.boolean();
        self.w.text("- Allow Composites");
        self.w.eol();
        self.w.line("");
        self.w.line("//Region Data");
        let n = self.count(3);
        self.w.ul(n);
        self.w.text("Regions In Brep");
        self.w.eol();
        self.index();
        self.w.text("- Start Region");
        self.w.eol();
        self.w.line("//Index, Number Of Shells, First Shell, Is void Region");
        for k in 0..n {
            let last = k + 1 == n;
            if !self.w.binary {
                self.w.ul(k);
            }
            self.topo();
            self.index();
            self.index();
            self.boolean();
            self.trailing(last);
            self.w.eol();
            self.record_attrs(1, !last);
        }

        let n = self.count(3);
        self.section("Shell Data", 3, n);
        for k in 0..n {
            let last = k + 1 == n;
            if !self.w.binary {
                self.w.ul(k);
            }
            self.topo();
            let ty = if self.chance(0.1) { 3 + self.below(5) } else { self.below(3) };
            self.w.ul(ty);
            match ty {
                0 => {
                    if self.w.binary {
                        self.index();
                        self.index();
                        self.boolean();
                    } else {
                        self.boolean();
                        self.index();
                        self.index();
                    }
                }
                1 | 2 => self.index(),
                _ => {}
            }
            self.trailing(last);
            self.w.eol();
            self.record_attrs(1, !last);
        }
        self.composite_list("Composite Face Data");

        let n = self.count(4);
        self.section("Faceuse Data", 1, n);
        for k in 0..n {
            let last = k + 1 == n;
            if !self.w.binary {
                self.w.ul(k);
            }
            self.topo();
            self.index();
            self.boolean();
            self.w.eol();
            self.record_attrs(1, !last);
        }

        let n = self.count(3);
        self.section("Face Data", 2, n);
        for k in 0..n {
            let last = k + 1 == n;
            let (a, b, c, d) = self.domain();
            if self.w.binary {
                self.topo();
                for v in [a, b, c, d] {
                    self.w.db(v);
                }
                self.double();
                self.index();
                self.index();
                self.index();
                self.boolean();
            } else {
                self.w.ul(k);
                self.topo();
                self.double();
                self.index();
                self.index();
                self.index();
                self.boolean();
                self.w.eol();
                for v in [a, b, c, d] {
                    self.w.db(v);
                }
                self.w.eol();
            }
            self.record_attrs(1, !last);
        }

        let n = self.count(3);
        self.section("Loop Data", 2, n);
        for k in 0..n {
            let last = k + 1 == n;
            if !self.w.binary {
                self.w.ul(k);
            }
            self.topo();
            let ty = if self.chance(0.1) { 2 + self.below(4) } else { self.below(2) };
            self.w.ul(ty);
            match ty {
                0 => {
                    self.index();
                    self.index();
                }
                1 => self.index(),
                _ => {}
            }
            self.w.eol();
            self.record_attrs(3, !last);
        }
        self.composite_list("Composite Edge Data");

        let n = self.count(4);
        self.section("Edgeuse Data", 4, n);
        for k in 0..n {
            let last = k + 1 == n;
            if !self.w.binary {
                self.w.ul(k);
            }
            self.topo();
            if self.version > 31 {
                self.index();
                self.index();
                self.index();
            }
            let ty = if self.chance(0.1) { 4 + self.below(4) } else { self.below(4) };
            self.w.ul(ty);
            if self.w.binary {
                if ty <= 3 {
                    self.index();
                    self.index();
                }
                self.boolean();
                self.index();
                if ty == 0 {
                    self.signed();
                    self.signed();
                }
            } else {
                match ty {
                    0 => {
                        self.index();
                        self.index();
                        self.boolean();
                        self.index();
                        self.signed();
                        self.signed();
                    }
                    1..=3 => {
                        self.index();
                        self.index();
                        self.boolean();
                        self.index();
                    }
                    _ => {}
                }
            }
            self.trailing(last);
            self.w.eol();
            self.record_attrs(2, !last);
        }

        let n = self.count(3);
        self.section("Edge Data", 1, n);
        for k in 0..n {
            let last = k + 1 == n;
            if !self.w.binary {
                self.w.ul(k);
            }
            self.topo();
            self.double();
            self.index();
            self.index();
            self.index();
            self.signed();
            let (a, b) = self.interval();
            self.w.db(a);
            self.w.db(b);
            self.w.eol();
            self.record_attrs(1, !last);
        }

        let n = self.count(3);
        self.section("Vertex Data", 1, n);
        for k in 0..n {
            let last = k + 1 == n;
            if !self.w.binary {
                self.w.ul(k);
            }
            self.topo();
            for _ in 0..4 {
                self.double();
            }
            self.w.eol();
            self.record_attrs(1, !last);
        }

        for (title, dim) in [("UV-Curve Data", 2), ("3D-Curve Data", 3)] {
            let n = self.count(2);
            self.section(title, 0, n);
            for k in 0..n {
                if !self.w.binary {
                    self.w.ul(k);
                    self.w.text("- Curve");
                    self.w.eol();
                }
                if !self.curve(dim, 0) {
                    return false;
                }
            }
        }
        let n = self.count(2);
        self.section("Surface Data", 0, n);
        for k in 0..n {
            if !self.w.binary {
                self.w.ul(k);
                self.w.text("- Surface");
                self.w.eol();
            }
            if !self.surface() {
                return false;
            }
        }
        true
    }

    fn composite_list(&mut self, title: &str) {
        let n = if self.chance(0.7) { 0 } else { 1 + self.below(3) };
        self.w.line("");
        self.w.line(&format!("//{title}"));
        self.w.ul(n);
        self.w.text("Composites In Brep");
        self.w.eol();
        if n != 0 {
            self.w.line("//Index, Count, Items");
            for k in 0..n {
                if !self.w.binary {
                    self.w.ul(k);
                }
                let m = self.below(4);
                self.w.ul(m);
                for _ in 0..m {
                    self.index();
                }
                self.w.eol();
            }
        }
    }

    fn domain(&mut self) -> (f64, f64, f64, f64) {
        let (a, b) = self.interval();
        let (c, d) = self.interval();
        (a, b, c, d)
    }

    /// An interval, sometimes inverted (`SetMinMax` then fails).
    fn interval(&mut self) -> (f64, f64) {
        let a = self.value();
        let b = self.value();
        if self.chance(0.3) {
            (a, b)
        } else if a > b {
            (b, a)
        } else {
            (a, b)
        }
    }

    /// Random non-decreasing knots for `n + 1` points of degree `p`.
    fn knots(&mut self, n: usize, p: usize) -> Vec<f64> {
        let scale = match self.below(4) {
            0 => 1e-6,
            1 => 1e6,
            _ => 1.0,
        };
        let shift = self.rng.uniform(-5.0, 5.0) * scale;
        let clamped = self.chance(0.7);
        let total = n + p + 2;
        let mut inner: Vec<f64> = (0..total).map(|_| self.rng.unit()).collect();
        inner.sort_by(f64::total_cmp);
        let mut k: Vec<f64> = if clamped {
            let mut v = vec![0.0; p + 1];
            let interior = n - p;
            let mut mids: Vec<f64> = (0..interior).map(|_| self.rng.uniform(0.05, 0.95)).collect();
            mids.sort_by(f64::total_cmp);
            v.extend(mids);
            v.extend(std::iter::repeat_n(1.0, p + 1));
            v
        } else {
            inner
        };
        // keep the domain [U[p], U[n+1]] non-degenerate
        let lo = k.get(p).copied().unwrap_or(0.0);
        let hi = k.get(n + 1).copied().unwrap_or(1.0);
        if lo.partial_cmp(&hi) != Some(std::cmp::Ordering::Less) {
            for (i, x) in k.iter_mut().enumerate() {
                *x = i as f64;
            }
        }
        for x in &mut k {
            *x = *x * scale + shift;
        }
        k
    }

    fn type_header(&mut self, ty: i64, dim: Option<u64>) {
        if self.w.binary {
            self.w.lg(ty);
            if let Some(d) = dim {
                self.w.ul(d);
            }
        } else {
            let word = if self.chance(0.5) { "Curve" } else { "Object" };
            self.w.text(&format!("{word} Type"));
            self.w.lg(ty);
            self.w.eol();
            if let Some(d) = dim {
                self.w.text("Curve Dim");
                self.w.ul(d);
                self.w.eol();
            }
        }
    }

    fn point_value(&mut self, scale: f64) -> f64 {
        self.rng.uniform(-10.0, 10.0) * scale
    }

    fn bspline_curve(&mut self, read_dim: i16) {
        let n = 1 + self.below(6) as usize;
        let p = 1 + self.below(n.min(4) as u64) as usize;
        let rat = i16::from(self.chance(0.4));
        self.w.lg(n as i64);
        self.w.eol();
        self.w.sh(p as i16);
        self.w.eol();
        self.w.sh(rat);
        self.w.eol();
        self.w.sh(read_dim);
        self.w.eol();
        if self.version > 32 {
            let kt = self.below(5);
            self.w.ul(kt);
            self.w.eol();
            let form = self.below(7);
            self.w.ul(form);
            self.w.eol();
        }
        let scale = match self.below(5) {
            0 => 1e-200,
            1 => 1e200,
            2 => 1e-6,
            _ => 1.0,
        };
        for _ in 0..=n {
            for _ in 0..read_dim {
                let v = self.point_value(scale);
                self.w.db(v);
            }
            if rat == 1 {
                let wt = self.rng.uniform(0.2, 3.0);
                self.w.db(wt);
            }
            self.w.eol();
        }
        for k in self.knots(n, p) {
            self.w.db(k);
            self.w.eol();
        }
    }

    /// Writes a curve; false when upstream stops reading after it (an error type).
    fn curve(&mut self, dim: u64, depth: u32) -> bool {
        if self.version > 32 {
            if self.chance(0.03) {
                self.type_header(1, Some(dim));
                return false;
            }
            let read_dim = if self.chance(0.8) { dim } else { 5 - dim };
            self.type_header(14033, Some(read_dim));
            self.bspline_curve(read_dim as i16);
        } else {
            let r = self.below(100);
            if r < 3 {
                self.type_header(14001, None);
                return false;
            }
            if r < 15 && depth < 3 {
                self.type_header(14032, None);
                let segs = self.below(4);
                let closed = if segs == 0 { 0 } else { self.below(3) };
                self.w.ul(segs);
                let d = 2 + self.below(2);
                self.w.ul(d);
                self.w.ul(closed);
                self.w.eol();
                for _ in 0..segs {
                    let sense = match self.below(3) {
                        0 => 0,
                        1 => 1,
                        _ => 5,
                    };
                    self.w.ul(sense);
                    self.double();
                    self.double();
                    self.w.eol();
                    if !self.curve(dim, depth + 1) {
                        return false;
                    }
                }
            } else {
                self.type_header(14033, None);
                let read_dim = if self.chance(0.8) { dim } else { 5 - dim };
                self.bspline_curve(read_dim as i16);
            }
        }
        if self.version > 30 && self.persist {
            self.attrs(false);
        }
        true
    }

    fn bspline_surface(&mut self) {
        let n = 1 + self.below(4) as usize;
        let m = 1 + self.below(4) as usize;
        let p = 1 + self.below(n.min(3) as u64) as usize;
        let q = 1 + self.below(m.min(3) as u64) as usize;
        let rat = i16::from(self.chance(0.3));
        self.w.lg(n as i64);
        self.w.lg(m as i64);
        self.w.eol();
        self.w.sh(p as i16);
        self.w.sh(q as i16);
        self.w.eol();
        self.w.sh(rat);
        self.w.eol();
        if self.version > 32 {
            let kt = self.below(5);
            self.w.ul(kt);
            self.w.eol();
            let form = self.below(12);
            self.w.ul(form);
            self.w.eol();
        }
        let scale = if self.chance(0.2) { 1e100 } else { 1.0 };
        for _ in 0..(n + 1) * (m + 1) {
            for _ in 0..3 {
                let v = self.point_value(scale);
                self.w.db(v);
            }
            if rat == 1 {
                let wt = self.rng.uniform(0.2, 3.0);
                self.w.db(wt);
            }
            self.w.eol();
        }
        for k in self.knots(n, p) {
            self.w.db(k);
            self.w.eol();
        }
        for k in self.knots(m, q) {
            self.w.db(k);
            self.w.eol();
        }
    }

    /// Writes a surface; false when upstream stops reading after it.
    fn surface(&mut self) -> bool {
        if self.version > 32 {
            if self.chance(0.03) {
                self.type_header(1, None);
                return false;
            }
            self.type_header(15002, None);
            self.bspline_surface();
        } else {
            let r = self.below(100);
            if r < 3 {
                self.type_header(15001, None);
                return false;
            }
            if r < 10 {
                self.type_header(15400, None);
                self.double();
                self.w.eol();
                self.bspline_surface();
            } else {
                self.type_header(15002, None);
                self.bspline_surface();
            }
        }
        if self.version > 30 && self.persist {
            self.attrs(false);
        }
        true
    }
}

const VERSIONS: &[u64] = &[29, 30, 31, 32, 33, 34, 38, 40, 41, 42, 43, 44, 45, 50];

fn generate(rng: &mut Rng) -> Inputs {
    let binary = rng.chance(0.5);
    let version = rng.choose(VERSIONS).copied().unwrap_or(30);
    let persist = rng.chance(0.7);
    let pre = rng.below(7);
    let mode_pick = rng.below(100);
    let mode = match mode_pick {
        0..=54 => "db",
        55..=69 => "file",
        70..=82 => "curve",
        83..=95 => "surface",
        _ => "construct",
    };
    let mut g = G {
        rng,
        w: W {
            binary,
            buf: Vec::new(),
            mid: false,
        },
        version,
        persist,
    };
    let mut dim = 0;
    let mut source = 0;
    let mut version_out = version;
    let mut persist_out = persist;
    match mode {
        "db" => {
            g.brep();
        }
        "file" => {
            g.persist = false;
            persist_out = false;
            let headed = version != 30 || g.chance(0.5);
            if headed {
                if g.w.binary {
                    g.w.ch(b'V');
                    g.w.ul(version);
                    if version > 41 {
                        g.w.ch(b'H');
                        let h = g.below(50);
                        g.w.ul(h);
                    }
                } else {
                    if g.chance(0.1) {
                        g.w.buf.push(b'\n');
                    }
                    g.w.text("Version");
                    g.w.ul(version);
                    g.w.eol();
                    if version > 41 {
                        g.w.text("Healer Version");
                        let h = g.below(50);
                        g.w.ul(h);
                        g.w.eol();
                    }
                }
            } else {
                g.version = 30;
                version_out = 30;
            }
            g.brep();
        }
        "curve" => {
            dim = 2 + g.below(2);
            g.curve(dim, 0);
        }
        "surface" => {
            g.surface();
        }
        _ => {
            dim = g.below(3);
            source = g.below(15) as i32;
            version_out = g.below(1000);
        }
    }
    let W { binary, buf, .. } = g.w;
    let (text, data) = if binary {
        (None, Some(buf))
    } else {
        (Some(String::from_utf8_lossy(&buf).into_owned()), None)
    };
    Inputs {
        mode: mode.to_string(),
        binary,
        version: version_out,
        dim,
        persist: persist_out,
        source,
        pre,
        text,
        data,
        path: None,
        skip_lines: 0,
    }
}
