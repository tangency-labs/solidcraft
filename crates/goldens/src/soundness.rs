//! The Rust side's result bodies measured on their own: a probe enables capture ([`start`]),
//! runs a case's Rust side, and reads what each runner that hands a result body to
//! [`measure`] found ([`take`]). Off (the default), [`measure`] does nothing, so a run
//! produces exactly what it did before.
//!
//! A body is sound when `check_brep` at level 2 reports nothing, its geometry's owners taken
//! as the geometry records them (not the runner's naming); a closed one is also a manifold
//! solid of positive volume.

use std::cell::RefCell;

use solidcore::geom::object::ObjectRef;
use solidcore::kernel::Kernel;
use solidcore::topology::brep::validation::{check_brep_with_keys, CheckLevel};
use solidcore::topology::Brep;
use solidcore::EntityRef;

thread_local! {
    static CAPTURED: RefCell<Option<Vec<Soundness>>> = const { RefCell::new(None) };
}

/// What [`measure`] found of one result body.
#[derive(Debug, Clone, PartialEq)]
pub struct Soundness {
    /// `check_brep` level 2's report lines (none: clean), or the check's error.
    pub check: Result<Vec<String>, String>,
    /// Whether the body is a manifold solid.
    pub manifold: Option<bool>,
    /// Its volume, when it is a manifold solid.
    pub volume: Option<f64>,
    /// Its face, edge and vertex counts.
    pub counts: [usize; 3],
}

impl Soundness {
    /// Whether the check is clean and a manifold body has a positive volume.
    pub fn sound(&self) -> bool {
        self.check.as_ref().is_ok_and(Vec::is_empty)
            && (self.manifold != Some(true) || self.volume.is_some_and(|v| v > 0.0))
    }

    /// One line: `sound`, the check's line count, manifold, volume and counts.
    pub fn line(&self) -> String {
        let check = match &self.check {
            Ok(lines) => format!(
                "check_lines={} first={:?}",
                lines.len(),
                lines
                    .first()
                    .map(|l| l.chars().take(120).collect::<String>())
            ),
            Err(e) => format!("check_error={e}"),
        };
        format!(
            "sound={} {check} manifold={:?} volume={:?} faces={} edges={} vertices={}",
            self.sound(),
            self.manifold,
            self.volume,
            self.counts[0],
            self.counts[1],
            self.counts[2]
        )
    }
}

/// Starts capturing the bodies [`measure`] is handed on this thread.
pub fn start() {
    CAPTURED.with(|c| *c.borrow_mut() = Some(Vec::new()));
}

/// Stops capturing and returns what was measured since [`start`].
pub fn take() -> Vec<Soundness> {
    CAPTURED.with(|c| c.borrow_mut().take().unwrap_or_default())
}

/// Measures `brep` when capture is on. The check builds caches and names the body's
/// geometry owners, so a runner hands it the body after its own outputs are recorded.
pub fn measure(brep: &mut Brep) {
    let on = CAPTURED.with(|c| c.borrow().is_some());
    if !on {
        return;
    }
    let found = measured(brep);
    CAPTURED.with(|c| {
        if let Some(v) = c.borrow_mut().as_mut() {
            v.push(found);
        }
    });
}

fn measured(brep: &mut Brep) -> Soundness {
    let mut k = Kernel::default();
    let counts = [
        solidcore::topology::brep::p1::num_faces(brep),
        solidcore::topology::brep::p1::edges(brep, None).map_or(0, |e| e.len()),
        solidcore::topology::brep::p1::vertices(brep, None).map_or(0, |v| v.len()),
    ];
    // Each geometry's owner is checked against the identity the body names its owner by;
    // a runner's export bookkeeping names them as it pleases, so the check is given the
    // owners the geometry records (an edge's curve's, a face's surface's, an edge use's
    // trim curve's): what it checks is the body's geometry, not its naming
    let mut owners = std::collections::BTreeMap::new();
    for (id, e) in brep.edges().iter() {
        if let Some(o) = e
            .curve
            .as_ref()
            .and_then(solidcore::curve::curve::Curve::owner)
        {
            owners.insert(EntityRef::Edge(id), o);
        }
    }
    for (id, f) in brep.faces().iter() {
        if let Some(o) = f
            .surface
            .as_ref()
            .and_then(solidcore::surface::surface::Surface::owner)
        {
            owners.insert(EntityRef::Face(id), o);
        }
    }
    for (id, u) in brep.edgeuses().iter() {
        if let Some(o) = u
            .uv_trim_curve
            .as_ref()
            .and_then(solidcore::curve::curve::Curve::owner)
        {
            owners.insert(EntityRef::Edgeuse(id), o);
        }
    }
    let keys = |e: EntityRef| {
        owners.get(&e).copied().unwrap_or(ObjectRef {
            type_code: 0,
            key: solidcore::geom::attribute::ObjectKey(0),
        })
    };
    let check = check_brep_with_keys(&mut k, brep, CheckLevel::Level2, &keys)
        .map(|c| c.lines())
        .map_err(|e| e.to_string());
    let manifold = solidcore::query::api_queries::brep_is_manifold_solid(brep).ok();
    let volume = (manifold == Some(true))
        .then(|| solidcore::mass::integrator::properties::brep_volume_default(&k, brep, 1e-6).ok())
        .flatten();
    Soundness {
        check,
        manifold,
        volume,
        counts,
    }
}

/// Runs the Rust side of every case of `unit` named in `$SC_SOUNDNESS_CASES` (one per
/// line) with capture on, and writes one line per case, what [`measure`] found of its
/// result bodies, to `$SC_SOUNDNESS` (a probe run on demand).
///
/// # Panics
///
/// When the unit's cases cannot be read, or `$SC_SOUNDNESS` is not set.
#[allow(clippy::unwrap_used, clippy::expect_used)] // a test probe
pub fn probe(runner: &dyn crate::UnitDiff) {
    use std::fmt::Write as _;
    let root = crate::case::repo_root();
    let names: Vec<String> = std::env::var("SC_SOUNDNESS_CASES")
        .map(|v| v.lines().map(str::to_string).collect())
        .unwrap_or_default();
    let context = crate::CaseContext { root, seeds: 0 };
    let mut report = String::new();
    for case in runner.cases(&context).unwrap() {
        if !names.contains(&case.name) {
            continue;
        }
        start();
        let out = runner.run_rust(&case);
        let found = take();
        let status = match &out {
            Ok(o) => format!(
                "{:?}{}",
                o.ints.get("status"),
                o.error
                    .as_deref()
                    .map(|e| format!(" error={e}"))
                    .unwrap_or_default()
            ),
            Err(e) => format!("harness error {e:?}"),
        };
        let lines: Vec<String> = found.iter().map(Soundness::line).collect();
        let _ = writeln!(
            report,
            "{}\tstatus={status}\t{}",
            case.name,
            lines.join(" | ")
        );
    }
    std::fs::write(std::env::var("SC_SOUNDNESS").expect("SC_SOUNDNESS"), report).unwrap();
}
