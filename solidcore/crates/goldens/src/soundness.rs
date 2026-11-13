//! The Rust side's result bodies measured on their own: a probe enables capture ([`start`]),
//! runs a case's Rust side, and reads what each runner that hands a result body to
//! [`measure`] found ([`take`]). Off (the default), [`measure`] does nothing, so a run
//! produces exactly what it did before.
//!
//! A body is sound as the kernel's local operation gates its result
//! ([`validation::soundness`]): `check_brep` at level 2 reports nothing, its geometry's owners
//! taken as the geometry records them (not the runner's naming), and a manifold one has a
//! positive volume. Where the runner named its input ([`input`]) and that was a manifold solid,
//! the body must be one too, of positive volume ([`validation::Soundness::sound`], #13685): a
//! solid's offset that comes back as loose sheets or as nothing is not sound.

use std::cell::RefCell;

use solidcore::kernel::Kernel;
use solidcore::mass::enclosure::VolumeSign;
use solidcore::topology::brep::validation;
use solidcore::topology::Brep;

thread_local! {
    static CAPTURED: RefCell<Option<Vec<Soundness>>> = const { RefCell::new(None) };
    /// Whether the case's input, as the runner named it ([`input`]), is a manifold solid.
    static INPUT_SOLID: RefCell<Option<bool>> = const { RefCell::new(None) };
}

/// What [`measure`] found of one result body.
#[derive(Debug, Clone, PartialEq)]
pub struct Soundness {
    /// `check_brep` level 2's report lines (none: clean), or the check's error.
    pub check: Result<Vec<String>, String>,
    /// Whether the body is a manifold solid.
    pub manifold: Option<bool>,
    /// Its volume's sign, when it is a manifold solid.
    pub volume: Option<VolumeSign>,
    /// Its face, edge and vertex counts.
    pub counts: [usize; 3],
    /// Whether the input of the operation it came from was a manifold solid, where the
    /// runner named that input ([`input`]); `None` where it did not.
    pub input_solid: Option<bool>,
}

impl Soundness {
    /// Whether the check is clean and the body is a manifold solid of positive volume, or has
    /// no face (the solid offset to nothing, #2092), where its input was a manifold solid;
    /// where the input was not one (or was not named), a body
    /// that is no manifold solid passes on its check, and a manifold one needs a positive
    /// volume.
    pub fn sound(&self) -> bool {
        let positive = matches!(self.volume, Some(VolumeSign::Positive(_)));
        self.check.as_ref().is_ok_and(Vec::is_empty)
            && if self.input_solid == Some(true) {
                // an empty body is the solid offset to nothing (#2092), an answer
                self.counts[0] == 0 || (self.manifold == Some(true) && positive)
            } else {
                self.manifold != Some(true) || positive
            }
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
            "sound={} {check} manifold={:?} volume={:?} faces={} edges={} vertices={} \
             input_solid={:?}",
            self.sound(),
            self.manifold,
            self.volume,
            self.counts[0],
            self.counts[1],
            self.counts[2],
            self.input_solid
        )
    }
}

/// Starts capturing the bodies [`measure`] is handed on this thread, no input named yet.
pub fn start() {
    CAPTURED.with(|c| *c.borrow_mut() = Some(Vec::new()));
    INPUT_SOLID.with(|i| *i.borrow_mut() = None);
}

/// Names the input of the operation whose result bodies the runner hands to [`measure`]
/// next, when capture is on: whether it is a manifold solid (`None` for no input). Off, it
/// does nothing; the query only reads the body.
pub fn input(brep: Option<&Brep>) {
    let on = CAPTURED.with(|c| c.borrow().is_some());
    if !on {
        return;
    }
    let solid = brep.map(|b| solidcore::topology::brep::p2::is_manifold_solid(b).unwrap_or(false));
    INPUT_SOLID.with(|i| *i.borrow_mut() = solid);
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
    let input_solid = INPUT_SOLID.with(|i| *i.borrow());
    let counts = [
        solidcore::topology::brep::p1::num_faces(brep),
        solidcore::topology::brep::p1::edges(brep, None).map_or(0, |e| e.len()),
        solidcore::topology::brep::p1::vertices(brep, None).map_or(0, |v| v.len()),
    ];
    // the kernel's measure ([`validation::soundness`]): the check given the owners the
    // geometry records (a runner's export bookkeeping names them as it pleases), the manifold
    // test and the volume
    match validation::soundness(&mut k, brep) {
        Ok(found) => Soundness {
            check: Ok(found.check.lines()),
            manifold: Some(found.manifold),
            volume: found.volume,
            counts,
            input_solid,
        },
        Err(e) => Soundness {
            check: Err(e.to_string()),
            manifold: None,
            volume: None,
            counts,
            input_solid,
        },
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
            // (a runner that records its status as the record's first value: that status)
            Ok(o) => format!(
                "{:?}{}",
                o.ints
                    .get("status")
                    .and_then(|s| s.first().map(|&s| s as f64))
                    .or_else(|| o.values.get("record").and_then(|r| r.first().copied())),
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

#[cfg(test)]
mod tests {
    use super::{input, measure, start, take, VolumeSign};
    use solidcore::kernel::Kernel;

    fn cube() -> solidcore::topology::Brep {
        let mut k = Kernel::default();
        solidcore::primitive::api_primitives::create_box(
            &mut k,
            solidcore::Point3::new(0.0, 0.0, 0.0),
            10.0,
            10.0,
            10.0,
        )
        .unwrap()
    }

    #[test]
    fn off_measures_nothing() {
        let _ = take();
        let mut b = cube();
        measure(&mut b);
        assert!(take().is_empty());
    }

    #[test]
    fn a_box_is_sound() {
        let mut b = cube();
        start();
        measure(&mut b);
        let found = take();
        assert_eq!(found.len(), 1);
        let s = &found[0];
        assert!(s.sound(), "{}", s.line());
        assert_eq!(s.manifold, Some(true));
        assert!(
            matches!(s.volume, Some(VolumeSign::Positive(e)) if e.lo <= 1000.0 && 1000.0 <= e.hi),
            "{}",
            s.line()
        );
        assert_eq!(s.counts, [6, 12, 8]);
        assert_eq!(s.input_solid, None);
    }

    /// A solid's result must be a solid (#13685), or nothing (#2092): from a solid input a
    /// box is sound and an empty body is sound, a lone plane sheet is not, though it passes
    /// where the input is no solid.
    #[test]
    fn a_solids_result_must_be_a_solid_or_nothing() {
        let solid = cube();
        let mut b = cube();
        let mut empty = solidcore::topology::Brep::new_with_infinite_region().unwrap();
        let mut sheet = solidcore::primitive::api_primitives::create_plane(
            &mut Kernel::default(),
            solidcore::Point3::new(0.0, 0.0, 0.0),
            10.0,
            10.0,
        )
        .unwrap();
        start();
        input(Some(&solid));
        measure(&mut b);
        measure(&mut empty);
        measure(&mut sheet);
        input(None);
        measure(&mut sheet);
        let found = take();
        assert_eq!(found.len(), 4);
        assert!(found[0].sound(), "{}", found[0].line());
        assert_eq!(found[0].input_solid, Some(true));
        assert!(found[1].sound(), "{}", found[1].line());
        assert!(!found[2].sound(), "{}", found[2].line());
        assert_eq!(found[2].manifold, Some(false));
        assert!(found[3].sound(), "{}", found[3].line());
        assert_eq!(found[3].input_solid, None);
    }
}
