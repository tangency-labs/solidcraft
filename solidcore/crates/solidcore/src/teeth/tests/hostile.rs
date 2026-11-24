//! Hostile numbers and counts through every public function and method of the tooth modules:
//! nothing may panic, and whatever is accepted must be finite (numbers) or a finite, closed outline
//! (outlines, pairs and plans).
//!
//! The functions are listed by hand in [`PUBLIC`]. The test reads the sources and fails when a
//! public function is there that the list does not name (or the list names one that is gone), and
//! the sweep records every function it calls and fails when one on the list was not called: a new
//! public function fails this test until the sweep calls it.

use std::collections::BTreeSet;
use std::f64::consts::PI;

use super::common::{operation, refusal, Dice, HOSTILE, HOSTILE_COUNTS};
use crate::error::teeth::{PlanRefusal, TeethFailure, Why, CHORD, FILLET_SEPARATION, FIT_DISTANCE};
use crate::error::ErrorKind;
use crate::teeth::involute::{External, Fit, Internal};
use crate::teeth::master_spline::{self, Circle, Form as MasterForm};
use crate::teeth::mesh::{Body, Material};
use crate::teeth::shaft_spline::{
    self, master_refused, refused, Numbers, Reasons, SplineCorner, SplineForm, SplineMake,
    SplineMaster, SplineNumbers, INVOLUTE_FIT, INVOLUTE_SHAFT, SERRATION_SHAFT, STRAIGHT_FIT,
    STRAIGHT_SHAFT,
};
use crate::teeth::straight::{
    self, SerratedHub, Serration, StraightHub, StraightSpline, TipCorner,
};
use crate::teeth::{bspline, fit, involute, mesh, Motion, Outline, Pair, Part, Piece, Point, Role};

/// Every public function and method of the tooth modules, as `module::Type::name` for a method and
/// `module::name` for a function, in the order of the sources.
const PUBLIC: &[&str] = &[
    "bspline::range",
    "bspline::point",
    "bspline::breaks",
    "fit::fitted",
    "fit::fit",
    "fit::solve_symmetric",
    "fit::bezier_pieces",
    "fit::one_piece",
    "fit::cubic_beziers",
    "involute::inv",
    "involute::External::pitch_radius",
    "involute::External::base_radius",
    "involute::External::tip_radius",
    "involute::External::root_radius",
    "involute::External::tooth_thickness",
    "involute::External::form_radius",
    "involute::External::is_undercut",
    "involute::External::half_angle_at",
    "involute::External::outline",
    "involute::Internal::pitch_radius",
    "involute::Internal::base_radius",
    "involute::Internal::form_radius",
    "involute::Internal::outline",
    "involute::hub_for",
    "involute::spline_pair",
    "master_spline::Form::circle",
    "master_spline::turned",
    "master_spline::replaced_period",
    "master_spline::placement",
    "master_spline::master_pair",
    "master_spline::master_spline",
    "mesh::Body::new",
    "mesh::Body::placed",
    "mesh::Body::points",
    "mesh::Body::material",
    "mesh::gap",
    "outline::Piece::role",
    "outline::Piece::start",
    "outline::Piece::end",
    "outline::rotated",
    "outline::Outline::new",
    "outline::Outline::teeth",
    "outline::Outline::tooth",
    "outline::Outline::tolerance",
    "outline::Outline::pitch_angle",
    "outline::Outline::points",
    "outline::Outline::radii",
    "outline::points_of",
    "outline::space_section",
    "outline::space_section_of",
    "pair::Pair::second_at",
    "pair::Pair::bodies",
    "pair::Pair::gap_at",
    "pair::Pair::through",
    "pair::Pair::through_a_cycle",
    "seat::normal",
    "seat::frame_through",
    "seat::angle_zero",
    "seat::off_cylinder",
    "seat::nearer_start",
    "seat::seat_frame",
    "seat::start_candidates",
    "seat::moved_out",
    "seat::into_frame",
    "seat::axis_gap",
    "seat::chord_gap",
    "seat::centre_angle",
    "seat::angles_apart",
    "seat::reach_along",
    "seat::points_apart",
    "section::outline_loop",
    "section::cutter_section",
    "section::chamfer_section",
    "section::chamfer_plane",
    "shaft_spline::module_for_major_radius",
    "shaft_spline::same_size",
    "shaft_spline::fit_to_shaft_within",
    "shaft_spline::tool_margin",
    "shaft_spline::cutter_radius",
    "shaft_spline::plan",
    "straight::StraightSpline::outline",
    "straight::StraightHub::outline",
    "straight::hub_for",
    "straight::spline_pair",
    "straight::turning_play",
    "straight::Serration::outline",
    "straight::Serration::sharp_tip_radius",
    "straight::SerratedHub::outline",
    "straight::SerratedHub::major_diameter",
    "straight::SerratedHub::minor_diameter",
    "straight::serration_hub_for",
    "straight::serration_pair",
    "straight::serration_play",
];

/// The public functions the sources hold, found as the list names them: a line `pub fn name` at the
/// start of a line is a function of the module, an indented one a method of the type of the `impl`
/// it stands in.
fn public_functions_in_the_sources() -> Vec<String> {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("teeth");
    let mut files: Vec<_> = std::fs::read_dir(&folder)
        .expect("the sources")
        .map(|e| e.expect("a file").path())
        .collect();
    files.sort();
    let mut found = Vec::new();
    for file in files
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e == "rs"))
    {
        let module = file
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("a name")
            .to_string();
        let text = std::fs::read_to_string(file).expect("a source");
        let mut within: Option<String> = None;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("impl") {
                // `impl Type {`, `impl<'a> Type<'a> {` or `impl Trait for Type {`: the type is the
                // name after the generics and after any `for`.
                let rest = if rest.starts_with('<') {
                    rest.split_once('>').map_or("", |(_, after)| after)
                } else {
                    rest
                };
                let head = rest.split('{').next().unwrap_or("");
                let head = head.rsplit(" for ").next().unwrap_or(head);
                let name: String = head
                    .trim()
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                within = Some(name);
                continue;
            }
            if line.starts_with('}') {
                within = None;
            }
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("pub fn ") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                match (&within, line.len() != trimmed.len()) {
                    (Some(owner), true) => found.push(format!("{module}::{owner}::{name}")),
                    _ => found.push(format!("{module}::{name}")),
                }
            }
        }
    }
    found
}

/// What the sweep has done: the functions called.
struct Sweep {
    called: BTreeSet<&'static str>,
    /// Accepted results that were not finite.
    not_finite: Vec<String>,
}

impl Sweep {
    /// Calls `f` as the function `name`.
    fn call<T>(&mut self, name: &'static str, f: impl FnOnce() -> T) -> T {
        self.called.insert(name);
        f()
    }

    /// Calls `f`, a public function that can refuse, as the function `name`: a refusal must be a
    /// tooth failure whose operation is that function's own path, `teeth::name`.
    fn checked<T>(
        &mut self,
        name: &'static str,
        f: impl FnOnce() -> crate::Result<T>,
    ) -> crate::Result<T> {
        let out = self.call(name, f);
        if let Err(e) = &out {
            assert!(
                e.teeth_failure().is_some(),
                "{name}: not a tooth failure: {e}"
            );
            assert_eq!(e.operation(), format!("teeth::{name}"), "{name}: {e}");
        }
        out
    }

    /// A call that gives a number: an accepted one must be finite.
    fn number(
        &mut self,
        name: &'static str,
        f: impl FnOnce() -> crate::Result<f64>,
    ) -> crate::Result<f64> {
        let out = self.checked(name, f);
        if let Ok(v) = out {
            if !v.is_finite() && !KNOWN_NOT_FINITE.contains(&name) {
                self.not_finite.push(format!("{name} gave {v}"));
            }
        }
        out
    }

    /// A call that gives an outline: an accepted one must be sound.
    fn outline(
        &mut self,
        name: &'static str,
        f: impl FnOnce() -> crate::Result<Outline>,
    ) -> crate::Result<Outline> {
        let out = self.checked(name, f);
        if let Ok(o) = &out {
            sound(name, o);
        }
        out
    }

    /// A call that gives a pair: both outlines sound, the places and the cycle finite.
    fn pair(
        &mut self,
        name: &'static str,
        f: impl FnOnce() -> crate::Result<Pair>,
    ) -> crate::Result<Pair> {
        let out = self.checked(name, f);
        if let Ok(p) = &out {
            sound(name, &p.first.outline);
            sound(name, &p.second.outline);
            let places = p
                .first
                .axis
                .iter()
                .chain(&p.second.axis)
                .chain([&p.second_start, &p.cycle]);
            assert!(places.into_iter().all(|v| v.is_finite()), "{name}: {p:?}");
        }
        out
    }
}

/// Functions known to give a number that is not finite for some hostile input: none now. A function
/// stands here only while its own area mends it.
const KNOWN_NOT_FINITE: [&str; 0] = [];

/// What every accepted outline must be.
fn sound(name: &str, outline: &Outline) {
    for piece in outline.tooth() {
        let (start, end) = (
            piece.start().expect("a start"),
            piece.end().expect("an end"),
        );
        assert!(
            start.iter().chain(&end).all(|v| v.is_finite()),
            "{name}: {piece:?}"
        );
    }
    let points = outline.points();
    assert!(
        points.len() >= 3 && points.len() <= crate::teeth::MAX_OUTLINE_POINTS,
        "{name}"
    );
    assert!(points.iter().flatten().all(|v| v.is_finite()), "{name}");
    let (low, high) = outline.radii();
    assert!(low.is_finite() && high.is_finite() && low <= high, "{name}");
    assert!(
        outline.tolerance().is_finite() && outline.pitch_angle().is_finite(),
        "{name}"
    );
}

/// Tolerances, hostile and plausible.
const TOLERANCES: [f64; 9] = [
    f64::NAN,
    f64::INFINITY,
    0.0,
    -1.0,
    1e-300,
    1e-9,
    1e-3,
    0.1,
    1e9,
];

/// The plausible tolerance most calls are made with.
const TOL: f64 = 1e-3;

/// The base and the base with each number, then each count, made hostile in turn, then `mixed` with
/// every number and count hostile at once.
fn variants<T: Clone>(
    base: T,
    numbers: &[fn(&mut T, f64)],
    counts: &[fn(&mut T, u32)],
    dice: &mut Dice,
    mixed: usize,
) -> Vec<T> {
    let mut out = vec![base.clone()];
    for set in numbers {
        for &v in &HOSTILE {
            let mut x = base.clone();
            set(&mut x, v);
            out.push(x);
        }
    }
    for set in counts {
        for &c in &HOSTILE_COUNTS {
            let mut x = base.clone();
            set(&mut x, c);
            out.push(x);
        }
    }
    for _ in 0..mixed {
        let mut x = base.clone();
        for set in numbers {
            set(&mut x, dice.pick(&HOSTILE));
        }
        for set in counts {
            set(&mut x, dice.pick(&HOSTILE_COUNTS));
        }
        out.push(x);
    }
    out
}

/// A hostile number for the variant at `k`, so that each argument meets every hostile number across
/// the variants.
fn arg(k: usize) -> f64 {
    HOSTILE[k % HOSTILE.len()]
}

/// How many accepted variants of each kind are taken through the slow calls (turning a pair);
/// refused ones are always called, as they are refused at once.
const SLOW_ACCEPTED: usize = 4;

fn plain_shaft() -> External {
    External {
        module: 2.0,
        teeth: 18,
        pressure_angle: 20.0,
        addendum: 1.0,
        dedendum: 1.25,
        shift: 0.0,
        tool_tip_radius: 0.3,
        thinning: 0.0,
    }
}

fn formed_hub() -> Internal {
    Internal {
        module: 2.0,
        teeth: 60,
        pressure_angle: 20.0,
        space_width: PI,
        minor_radius: 59.0,
        major_radius: 62.5,
        fillet_radius: 0.5,
    }
}

fn spline_fit() -> Fit {
    Fit {
        side_clearance: 0.06,
        hub_addendum: 0.5,
        tip_clearance: 0.2,
        hub_fillet_radius: 0.3,
        form_clearance: 0.05,
    }
}

/// A pair turned a little: every method of a pair, with hostile turns and reaches.
fn pair_methods(s: &mut Sweep, pair: &Pair, k: usize) {
    let _ = s.call("pair::Pair::second_at", || pair.second_at(arg(k)));
    let _ = s.checked("pair::Pair::bodies", || pair.bodies());
    let _ = s.checked("pair::Pair::gap_at", || pair.gap_at(arg(k), 0.5));
    let _ = s.checked("pair::Pair::gap_at", || pair.gap_at(0.1, arg(k + 7)));
    let _ = s.checked("pair::Pair::through", || {
        pair.through(arg(k), arg(k + 3), 2, 0.5)
    });
    let _ = s.checked("pair::Pair::through_a_cycle", || {
        pair.through_a_cycle(2, 0.5)
    });
}

fn sweep_involute(s: &mut Sweep, dice: &mut Dice) {
    let numbers: [fn(&mut External, f64); 7] = [
        |g, v| g.module = v,
        |g, v| g.pressure_angle = v,
        |g, v| g.addendum = v,
        |g, v| g.dedendum = v,
        |g, v| g.shift = v,
        |g, v| g.tool_tip_radius = v,
        |g, v| g.thinning = v,
    ];
    let counts: [fn(&mut External, u32); 1] = [|g, c| g.teeth = c];
    let mut slow = 0;
    for (k, g) in variants(plain_shaft(), &numbers, &counts, dice, 300)
        .into_iter()
        .enumerate()
    {
        let _ = s.number("involute::External::pitch_radius", || g.pitch_radius());
        let _ = s.number("involute::External::base_radius", || g.base_radius());
        let _ = s.number("involute::External::tip_radius", || g.tip_radius());
        let _ = s.number("involute::External::root_radius", || g.root_radius());
        let _ = s.number("involute::External::tooth_thickness", || {
            g.tooth_thickness()
        });
        let _ = s.number("involute::External::form_radius", || g.form_radius());
        let _ = s.checked("involute::External::is_undercut", || g.is_undercut());
        let _ = s.number("involute::External::half_angle_at", || {
            g.half_angle_at(arg(k))
        });
        let _ = s.number("involute::External::half_angle_at", || {
            g.half_angle_at(18.0)
        });
        let _ = s.outline("involute::External::outline", || g.outline(TOL));
        let hub = s.checked("involute::hub_for", || involute::hub_for(&g, &spline_fit()));
        if let Ok(hub) = hub {
            let _ = s.outline("involute::Internal::outline", || hub.outline(TOL));
        }
        let ok = g.outline(TOL).is_ok();
        if !ok || slow < SLOW_ACCEPTED {
            slow += usize::from(ok);
            let _ = s.pair("involute::spline_pair", || {
                involute::spline_pair(&g, &spline_fit(), TOL)
            });
        }
        let fit_numbers = [arg(k), 0.5, 0.2, 0.3, 0.05];
        let mut f = spline_fit();
        match k % 5 {
            0 => f.side_clearance = fit_numbers[0],
            1 => f.hub_addendum = fit_numbers[0],
            2 => f.tip_clearance = fit_numbers[0],
            3 => f.hub_fillet_radius = fit_numbers[0],
            _ => f.form_clearance = fit_numbers[0],
        }
        let _ = s.checked("involute::hub_for", || {
            involute::hub_for(&plain_shaft(), &f)
        });
    }
    for &t in &TOLERANCES {
        let _ = s.outline("involute::External::outline", || plain_shaft().outline(t));
        let _ = s.outline("involute::Internal::outline", || formed_hub().outline(t));
    }
    for &v in &HOSTILE {
        let _ = s.call("involute::inv", || involute::inv(v));
    }
    let numbers: [fn(&mut Internal, f64); 6] = [
        |g, v| g.module = v,
        |g, v| g.pressure_angle = v,
        |g, v| g.space_width = v,
        |g, v| g.minor_radius = v,
        |g, v| g.major_radius = v,
        |g, v| g.fillet_radius = v,
    ];
    let counts: [fn(&mut Internal, u32); 1] = [|g, c| g.teeth = c];
    for i in variants(formed_hub(), &numbers, &counts, dice, 200) {
        let _ = s.number("involute::Internal::pitch_radius", || i.pitch_radius());
        let _ = s.number("involute::Internal::base_radius", || i.base_radius());
        let _ = s.number("involute::Internal::form_radius", || i.form_radius());
        let _ = s.outline("involute::Internal::outline", || i.outline(TOL));
    }
}

fn sweep_straight(s: &mut Sweep, dice: &mut Dice) {
    let base = StraightSpline {
        teeth: 6,
        major_diameter: 28.0,
        minor_diameter: 23.0,
        tooth_width: 6.0,
        root_round: 0.2,
        tip: TipCorner::Chamfer { size: 0.3 },
    };
    let fit = straight::Fit {
        side_clearance: 0.05,
        major_clearance: 0.3,
        minor_clearance: 0.3,
        hub_root_round: 0.2,
        hub_tip_round: 0.3,
    };
    let numbers: [fn(&mut StraightSpline, f64); 5] = [
        |g, v| g.major_diameter = v,
        |g, v| g.minor_diameter = v,
        |g, v| g.tooth_width = v,
        |g, v| g.root_round = v,
        |g, v| g.tip = TipCorner::Round { radius: v },
    ];
    let counts: [fn(&mut StraightSpline, u32); 1] = [|g, c| g.teeth = c];
    let fit_numbers: [fn(&mut straight::Fit, f64); 5] = [
        |f, v| f.side_clearance = v,
        |f, v| f.major_clearance = v,
        |f, v| f.minor_clearance = v,
        |f, v| f.hub_root_round = v,
        |f, v| f.hub_tip_round = v,
    ];
    let fits = variants(fit, &fit_numbers, &[], dice, 30);
    let mut slow = 0;
    for (k, shaft) in variants(base, &numbers, &counts, dice, 100)
        .into_iter()
        .enumerate()
    {
        let shaft = if k % 4 == 1 {
            StraightSpline {
                tip: TipCorner::Chamfer { size: arg(k) },
                ..shaft
            }
        } else {
            shaft
        };
        let f = fits[k % fits.len()];
        let _ = s.outline("straight::StraightSpline::outline", || shaft.outline(TOL));
        if let Ok(hub) = s.checked("straight::hub_for", || straight::hub_for(&shaft, &f)) {
            let _ = s.outline("straight::StraightHub::outline", || hub.outline(TOL));
        }
        let _ = s.number("straight::turning_play", || {
            straight::turning_play(&shaft, &f)
        });
        let ok = shaft.outline(TOL).is_ok();
        if !ok || slow < SLOW_ACCEPTED {
            slow += usize::from(ok);
            if let Ok(pair) = s.pair("straight::spline_pair", || {
                straight::spline_pair(&shaft, &fit, TOL)
            }) {
                pair_methods(s, &pair, k);
            }
        }
        let _ = s.pair("straight::spline_pair", || {
            straight::spline_pair(&shaft, &f, arg(k + 1))
        });
    }
    let hub = StraightHub {
        teeth: 6,
        major_diameter: 28.6,
        minor_diameter: 23.0,
        space_width: 6.1,
        root_round: 0.2,
        tip_round: 0.3,
    };
    let numbers: [fn(&mut StraightHub, f64); 5] = [
        |g, v| g.major_diameter = v,
        |g, v| g.minor_diameter = v,
        |g, v| g.space_width = v,
        |g, v| g.root_round = v,
        |g, v| g.tip_round = v,
    ];
    let counts: [fn(&mut StraightHub, u32); 1] = [|g, c| g.teeth = c];
    for h in variants(hub, &numbers, &counts, dice, 60) {
        let _ = s.outline("straight::StraightHub::outline", || h.outline(TOL));
    }
    let base = Serration {
        teeth: 36,
        major_diameter: 20.0,
        minor_diameter: 18.8,
        flank_angle: 90.0,
        tip: TipCorner::Round { radius: 0.1 },
        root_round: 0.03,
    };
    let serration_fit = straight::Fit {
        side_clearance: 0.04,
        major_clearance: 0.1,
        minor_clearance: 0.1,
        hub_root_round: 0.03,
        hub_tip_round: 0.05,
    };
    let numbers: [fn(&mut Serration, f64); 5] = [
        |g, v| g.major_diameter = v,
        |g, v| g.minor_diameter = v,
        |g, v| g.flank_angle = v,
        |g, v| g.root_round = v,
        |g, v| g.tip = TipCorner::Chamfer { size: v },
    ];
    let counts: [fn(&mut Serration, u32); 1] = [|g, c| g.teeth = c];
    let fits = variants(serration_fit, &fit_numbers, &[], dice, 30);
    let mut slow = 0;
    for (k, shaft) in variants(base, &numbers, &counts, dice, 100)
        .into_iter()
        .enumerate()
    {
        let shaft = if k % 5 == 2 {
            Serration {
                tip: TipCorner::Plain,
                ..shaft
            }
        } else {
            shaft
        };
        let f = fits[k % fits.len()];
        let _ = s.outline("straight::Serration::outline", || shaft.outline(TOL));
        let _ = s.number("straight::Serration::sharp_tip_radius", || {
            shaft.sharp_tip_radius()
        });
        if let Ok(hub) = s.checked("straight::serration_hub_for", || {
            straight::serration_hub_for(&shaft, &f)
        }) {
            let _ = s.outline("straight::SerratedHub::outline", || hub.outline(TOL));
            let _ = s.number("straight::SerratedHub::major_diameter", || {
                hub.major_diameter()
            });
            let _ = s.number("straight::SerratedHub::minor_diameter", || {
                hub.minor_diameter()
            });
        }
        let loose = SerratedHub { shaft, fit: f };
        let _ = s.outline("straight::SerratedHub::outline", || loose.outline(arg(k)));
        let _ = s.number("straight::SerratedHub::major_diameter", || {
            loose.major_diameter()
        });
        let _ = s.number("straight::SerratedHub::minor_diameter", || {
            loose.minor_diameter()
        });
        let _ = s.number("straight::serration_play", || {
            straight::serration_play(&shaft, &f)
        });
        let ok = shaft.outline(TOL).is_ok();
        if !ok || slow < SLOW_ACCEPTED {
            slow += usize::from(ok);
            if let Ok(pair) = s.pair("straight::serration_pair", || {
                straight::serration_pair(&shaft, &serration_fit, TOL)
            }) {
                pair_methods(s, &pair, k);
            }
        }
        let _ = s.pair("straight::serration_pair", || {
            straight::serration_pair(&shaft, &f, arg(k + 1))
        });
    }
}

fn sweep_fit(s: &mut Sweep) {
    let outline = plain_shaft().outline(1e-4).expect("a shaft");
    let (teeth, top) = (outline.teeth(), outline.radii().1);
    for (k, &v) in HOSTILE.iter().enumerate() {
        let _ = s.checked("fit::one_piece", || fit::one_piece(outline.tooth(), v));
        let _ = s.checked("outline::space_section", || {
            crate::teeth::outline::space_section(&outline, v, 40.0)
        });
        let _ = s.checked("outline::space_section", || {
            crate::teeth::outline::space_section(&outline, 0.0, v)
        });
        // The same outline held as its plain parts, each of them hostile in turn.
        let count = HOSTILE_COUNTS[k % HOSTILE_COUNTS.len()];
        let section = crate::teeth::outline::space_section_of;
        let _ = s.checked("outline::space_section_of", || {
            section(teeth, outline.tooth(), top, v, 40.0)
        });
        let _ = s.checked("outline::space_section_of", || {
            section(teeth, outline.tooth(), top, 0.0, v)
        });
        let _ = s.checked("outline::space_section_of", || {
            section(teeth, outline.tooth(), v, 0.0, 40.0)
        });
        let _ = s.checked("outline::space_section_of", || {
            section(count, outline.tooth(), top, 0.0, 40.0)
        });
        let _ = s.checked("outline::space_section_of", || {
            section(teeth, &[], top, 0.0, 40.0)
        });
        let points = crate::teeth::outline::points_of;
        let found = [
            s.checked("outline::points_of", || {
                points(teeth, outline.tooth(), v, 0.0)
            }),
            s.checked("outline::points_of", || {
                points(teeth, outline.tooth(), 1e-3, v)
            }),
            s.checked("outline::points_of", || {
                points(count, outline.tooth(), 1e-3, 0.0)
            }),
            s.checked("outline::points_of", || points(teeth, &[], 1e-3, 0.0)),
        ];
        for chain in found.into_iter().flatten() {
            assert!(chain.len() <= crate::teeth::MAX_OUTLINE_POINTS);
            assert!(
                chain.iter().flatten().all(|x| x.is_finite()),
                "points_of at {v}"
            );
        }
        let _ = s.outline("fit::fitted", || fit::fitted(&outline, v));
        for degree in [0, 1, 2, 3, 5, 9, 10, usize::MAX] {
            let _ = s.checked("fit::fit", || fit::fit(&outline, v, degree));
        }
    }
    for degree in [2, 3, 5] {
        if let Ok(fitted) = s.checked("fit::fit", || fit::fit(&outline, 1e-2, degree)) {
            sound("fit::fit", &fitted.outline);
            for piece in fitted.outline.tooth() {
                if let Piece::Spline {
                    degree,
                    knots,
                    control,
                    ..
                } = piece
                {
                    let _ = s.checked("fit::bezier_pieces", || {
                        fit::bezier_pieces(*degree, knots, control)
                    });
                    let _ = s.checked("fit::cubic_beziers", || fit::cubic_beziers(knots, control));
                }
            }
        }
    }
    let mut dice = Dice(0xF17);
    for round in 0..2000 {
        let size = dice.roll() % 6;
        let matrix: Vec<f64> = (0..size * size + round % 3)
            .map(|_| dice.pick(&HOSTILE))
            .collect();
        let right: Vec<f64> = (0..size + round % 2).map(|_| dice.pick(&HOSTILE)).collect();
        let _ = s.checked("fit::solve_symmetric", || {
            fit::solve_symmetric(size, &matrix, &right)
        });
        let identity: Vec<f64> = (0..size * size)
            .map(|i| if i % (size + 1) == 0 { 2.0 } else { 0.1 })
            .collect();
        let _ = s.checked("fit::solve_symmetric", || {
            fit::solve_symmetric(size, &identity, &right)
        });
        let degree = dice.roll() % 12;
        let count = dice.roll() % 14;
        let control: Vec<Point> = (0..count)
            .map(|_| [dice.pick(&HOSTILE), dice.pick(&HOSTILE)])
            .collect();
        let mut knots: Vec<f64> = (0..(count + degree + 1).saturating_sub(round % 2))
            .map(|_| dice.pick(&HOSTILE))
            .collect();
        if round % 3 == 0 {
            knots.sort_by(f64::total_cmp);
        }
        let _ = s.checked("fit::bezier_pieces", || {
            fit::bezier_pieces(degree, &knots, &control)
        });
        let _ = s.checked("fit::cubic_beziers", || {
            fit::cubic_beziers(&knots, &control)
        });
        let _ = s.checked("bspline::range", || {
            bspline::range(degree, &knots, &control)
        });
        let _ = s.checked("bspline::point", || {
            bspline::point(degree, &knots, &control, dice.pick(&HOSTILE))
        });
        let _ = s.checked("bspline::breaks", || {
            bspline::breaks(degree, &knots, &control)
        });
        let piece = Piece::Spline {
            degree,
            knots,
            control,
            role: Role::Tip,
        };
        let _ = s.call("outline::Piece::start", || piece.start());
        let _ = s.call("outline::Piece::end", || piece.end());
        let _ = s.outline("outline::Outline::new", || {
            Outline::new(
                dice.pick(&HOSTILE_COUNTS),
                vec![piece],
                dice.pick(&TOLERANCES),
            )
        });
    }
}

fn sweep_outline_mesh_pair(s: &mut Sweep, dice: &mut Dice) {
    for round in 0..3000 {
        let number = |dice: &mut Dice| dice.pick(&HOSTILE);
        let point = |dice: &mut Dice| [number(dice), number(dice)];
        let pieces: Vec<Piece> = (0..=(round % 4))
            .map(|k| match (round + k) % 4 {
                0 => Piece::Arc {
                    centre: point(dice),
                    radius: number(dice),
                    start: number(dice),
                    sweep: number(dice),
                    role: Role::Tip,
                },
                1 => Piece::Line {
                    from: point(dice),
                    to: point(dice),
                    role: Role::Flank,
                },
                2 => Piece::Curve {
                    points: (0..round % 5).map(|_| point(dice)).collect(),
                    role: Role::Root,
                },
                _ => Piece::Arc {
                    centre: [0.0, 0.0],
                    radius: 1.0 + f64::from(round % 7),
                    start: 0.0,
                    sweep: number(dice),
                    role: Role::Fillet,
                },
            })
            .collect();
        for piece in &pieces {
            let _ = s.call("outline::Piece::role", || piece.role());
            let _ = s.call("outline::Piece::start", || piece.start());
            let _ = s.call("outline::Piece::end", || piece.end());
        }
        let teeth = dice.pick(&HOSTILE_COUNTS);
        let tolerance = dice.pick(&TOLERANCES);
        if let Ok(o) = s.outline("outline::Outline::new", || {
            Outline::new(teeth, pieces, tolerance)
        }) {
            outline_methods(s, &o);
        }
        let p = s.call("outline::rotated", || {
            crate::teeth::outline::rotated(point(dice), number(dice))
        });
        let _ = p;
        let points: Vec<Point> = (0..round % 7).map(|_| point(dice)).collect();
        let material = if round % 2 == 0 {
            Material::Inside
        } else {
            Material::Outside
        };
        if let Ok(body) = s.checked("mesh::Body::new", || Body::new(points, material)) {
            body_methods(s, &body, dice);
        }
    }
    // Plausible shapes with hostile turns, places and reaches.
    let square = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
    let Ok(body) = Body::new(square.to_vec(), Material::Inside) else {
        panic!("a square")
    };
    for _ in 0..500 {
        body_methods(s, &body, dice);
    }
    let Ok(circle) = Outline::new(
        1,
        vec![Piece::Arc {
            centre: [0.0, 0.0],
            radius: 5.0,
            start: 0.0,
            sweep: std::f64::consts::TAU,
            role: Role::Tip,
        }],
        1e-3,
    ) else {
        panic!("a circle")
    };
    outline_methods(s, &circle);
    let Ok(bar) = Outline::new(
        1,
        square
            .iter()
            .zip(square.iter().cycle().skip(1))
            .map(|(&a, &b)| Piece::Line {
                from: a,
                to: b,
                role: Role::Flank,
            })
            .collect(),
        1e-3,
    ) else {
        panic!("a bar")
    };
    let base = Pair {
        first: Part {
            outline: circle.clone(),
            material: Material::Inside,
            axis: [0.0, 0.0],
        },
        second: Part {
            outline: bar,
            material: Material::Inside,
            axis: [7.0, 0.0],
        },
        motion: Motion::Turning { ratio: -1.0 },
        second_start: 0.0,
        cycle: 1.0,
    };
    for (k, &v) in HOSTILE.iter().enumerate() {
        let pairs = [
            Pair {
                first: Part {
                    axis: [v, 0.0],
                    ..base.first.clone()
                },
                ..base.clone()
            },
            Pair {
                second: Part {
                    axis: [0.0, v],
                    ..base.second.clone()
                },
                ..base.clone()
            },
            Pair {
                motion: Motion::Turning { ratio: v },
                ..base.clone()
            },
            Pair {
                motion: Motion::Turning { ratio: arg(k + 5) },
                second_start: v,
                ..base.clone()
            },
            Pair {
                cycle: v,
                ..base.clone()
            },
        ];
        for pair in &pairs {
            pair_methods(s, pair, k);
            let _ = s.checked("pair::Pair::through", || {
                pair.through(0.0, 1.0, HOSTILE_COUNTS[k % HOSTILE_COUNTS.len()].min(3), v)
            });
            let _ = s.checked("pair::Pair::through_a_cycle", || {
                pair.through_a_cycle(HOSTILE_COUNTS[k % HOSTILE_COUNTS.len()], 0.5)
            });
        }
    }
}

fn outline_methods(s: &mut Sweep, o: &Outline) {
    let _ = s.call("outline::Outline::teeth", || o.teeth());
    let _ = s.call("outline::Outline::tooth", || o.tooth().len());
    let _ = s.call("outline::Outline::tolerance", || o.tolerance());
    let _ = s.call("outline::Outline::pitch_angle", || o.pitch_angle());
    let _ = s.call("outline::Outline::points", || o.points());
    let _ = s.call("outline::Outline::radii", || o.radii());
}

fn body_methods(s: &mut Sweep, body: &Body, dice: &mut Dice) {
    let _ = s.call("mesh::Body::points", || body.points().len());
    let _ = s.call("mesh::Body::material", || body.material());
    let placed = s.call("mesh::Body::placed", || {
        body.placed(
            dice.pick(&HOSTILE),
            [dice.pick(&HOSTILE), dice.pick(&HOSTILE)],
        )
    });
    let reach = dice.pick(&HOSTILE);
    if let Ok(Some(found)) = s.checked("mesh::gap", || mesh::gap(body, &placed, reach)) {
        assert!(
            found.distance.is_finite() && found.at.iter().all(|v| v.is_finite()),
            "{found:?}"
        );
    }
    let _ = s.checked("mesh::gap", || mesh::gap(&placed, body, 0.5));
}

/// A serration whose flanks are all but parallel (a flank angle of the smallest positive number of
/// degrees): its sharp tip would be too far away to say, and that is refused. It used to come back
/// as a number that is not finite.
#[test]
fn a_serration_with_all_but_parallel_flanks_has_no_sharp_tip_to_give() {
    let serration = Serration {
        teeth: 36,
        major_diameter: 20.0,
        minor_diameter: 18.8,
        flank_angle: f64::MIN_POSITIVE,
        tip: TipCorner::Round { radius: 0.1 },
        root_round: 0.03,
    };
    let radius = serration.sharp_tip_radius();
    assert!(
        radius.as_ref().is_ok_and(|r| r.is_finite()) || radius.is_err(),
        "{radius:?}"
    );
}

#[test]
fn the_list_names_every_public_function_of_the_sources() {
    let found = public_functions_in_the_sources();
    let listed: Vec<String> = PUBLIC.iter().map(ToString::to_string).collect();
    let missing: Vec<&String> = found.iter().filter(|f| !listed.contains(f)).collect();
    let gone: Vec<&String> = listed.iter().filter(|f| !found.contains(f)).collect();
    assert!(
        missing.is_empty() && gone.is_empty(),
        "public functions the sweep does not call: {missing:?}; listed functions the sources no longer have: {gone:?}"
    );
    let unique: BTreeSet<&str> = PUBLIC.iter().copied().collect();
    assert_eq!(unique.len(), PUBLIC.len(), "a function is listed twice");
}

#[test]
fn hostile_numbers_through_every_public_function_never_panic_or_hang() {
    let mut s = Sweep {
        called: BTreeSet::new(),
        not_finite: Vec::new(),
    };
    let mut dice = Dice(0x5EED);
    sweep_involute(&mut s, &mut dice);
    sweep_straight(&mut s, &mut dice);
    sweep_fit(&mut s);
    sweep_outline_mesh_pair(&mut s, &mut dice);
    sweep_master_spline(&mut s, &mut dice);
    sweep_shaft_spline(&mut s, &mut dice);
    sweep_seat_and_section(&mut s);
    let not_called: Vec<&str> = PUBLIC
        .iter()
        .copied()
        .filter(|name| !s.called.contains(name))
        .collect();
    assert!(
        not_called.is_empty(),
        "the sweep did not call {not_called:?}"
    );
    let strangers: Vec<&&str> = s
        .called
        .iter()
        .filter(|name| !PUBLIC.contains(name))
        .collect();
    assert!(
        strangers.is_empty(),
        "the sweep calls functions that are not listed: {strangers:?}"
    );
    assert!(
        s.not_finite.is_empty(),
        "{} results not finite: {:?}",
        s.not_finite.len(),
        s.not_finite
    );
}

/// The round face a spline sits on and the sections of its tools: hostile points, directions,
/// sizes, angles and counts through every function, on a plain face and frame and on ones made of
/// the hostile numbers themselves. They hand back plain numbers, frames and profiles, so the sweep
/// asks only that no call panics and that a refusal is the function's own.
fn sweep_seat_and_section(s: &mut Sweep) {
    use crate::eval::Frame;
    use crate::teeth::seat::{self, RoundFace};
    use crate::teeth::section::{self, EndChamfer};
    use crate::Vec3;

    let v = |k: usize| Vec3::new(arg(k), arg(k + 1), arg(k + 2));
    let plain_frame = Frame {
        origin: [0.0, 0.0, 0.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    };
    let plain_face = RoundFace {
        axis_point: Vec3::ZERO,
        axis: Vec3::Z,
        radius: 10.0,
        start: 0.0,
        end: 30.0,
    };
    let tooth = [
        Piece::Arc {
            centre: [0.0, 0.0],
            radius: 10.0,
            start: -0.2,
            sweep: 0.4,
            role: Role::Tip,
        },
        Piece::Line {
            from: [10.0 * 0.2_f64.cos(), 10.0 * 0.2_f64.sin()],
            to: [8.0 * 0.3_f64.cos(), 8.0 * 0.3_f64.sin()],
            role: Role::Flank,
        },
    ];
    for k in 0..HOSTILE.len() {
        let hostile_frame = Frame {
            origin: v(k).to_array(),
            x_axis: v(k + 3).to_array(),
            y_axis: v(k + 5).to_array(),
        };
        let hostile_face = RoundFace {
            axis_point: v(k),
            axis: v(k + 1),
            radius: arg(k + 2),
            start: arg(k + 3),
            end: arg(k + 4),
        };
        for plane in [&plain_frame, &hostile_frame] {
            let _ = s.call("seat::normal", || seat::normal(plane));
            let _ = s.call("seat::moved_out", || {
                seat::moved_out(plane, v(k), arg(k + 1))
            });
            let _ = s.call("seat::into_frame", || seat::into_frame(plane));
            let _ = s.call("seat::centre_angle", || {
                seat::centre_angle(plane, v(k), v(k + 2))
            });
            let _ = s.call("section::chamfer_plane", || {
                section::chamfer_plane(plane, arg(k), arg(k + 1))
            });
        }
        let _ = s.call("seat::frame_through", || {
            seat::frame_through(v(k), v(k + 1), v(k + 2))
        });
        let _ = s.call("seat::angle_zero", || seat::angle_zero(v(k)));
        for face in [&plain_face, &hostile_face] {
            let _ = s.call("seat::off_cylinder", || seat::off_cylinder(face, v(k)));
            let _ = s.call("seat::nearer_start", || seat::nearer_start(face, v(k)));
            let _ = s.call("seat::seat_frame", || {
                seat::seat_frame(face, v(k), Some(v(k + 1)))
            });
            let _ = s.call("seat::seat_frame", || seat::seat_frame(face, v(k), None));
            let _ = s.call("seat::start_candidates", || {
                seat::start_candidates(face, v(k), k % 2 == 0)
            });
        }
        let _ = s.call("seat::axis_gap", || {
            seat::axis_gap(v(k), v(k + 1), v(k + 2), v(k + 3))
        });
        let _ = s.call("seat::chord_gap", || {
            seat::chord_gap(v(k), v(k + 1), v(k + 2))
        });
        let _ = s.call("seat::angles_apart", || {
            seat::angles_apart(arg(k), arg(k + 1))
        });
        let _ = s.call("seat::reach_along", || {
            seat::reach_along(v(k), v(k + 1), v(k + 2), v(k + 3))
        });
        let _ = s.call("seat::points_apart", || {
            seat::points_apart([arg(k), arg(k + 1)], [arg(k + 2), 1.0])
        });
        // The loop of an outline: a hostile pitch, largest radius and turn on a real tooth, then
        // hostile counts of teeth.
        let _ = s.checked("section::outline_loop", || {
            section::outline_loop(6, arg(k), &tooth, arg(k + 1), arg(k + 2))
        });
        let teeth = HOSTILE_COUNTS[k % HOSTILE_COUNTS.len()];
        let _ = s.checked("section::outline_loop", || {
            section::outline_loop(teeth.min(2001), 0.1, &tooth, 10.0, arg(k))
        });
        let _ = s.checked("section::cutter_section", || {
            section::cutter_section(
                &tooth,
                [arg(k), 1.0],
                [1.0, arg(k + 1)],
                arg(k + 2),
                arg(k + 3),
                arg(k + 4),
                v(k),
            )
        });
        let _ = s.checked("section::cutter_section", || {
            section::cutter_section(
                &tooth,
                [10.0, 1.0],
                [10.0, -1.0],
                9.0,
                arg(k),
                arg(k + 1),
                Vec3::ZERO,
            )
        });
        for outside in [true, false] {
            let chamfer = EndChamfer {
                radius: arg(k),
                radial: arg(k + 1),
                axial: arg(k + 2),
                past: arg(k + 3),
                flush: Some(arg(k + 4)),
            };
            let _ = s.call("section::chamfer_section", || {
                section::chamfer_section(&chamfer, arg(k + 5), outside)
            });
            let plain = EndChamfer {
                radius: 10.0,
                radial: 1.0,
                axial: 1.0,
                past: 0.0,
                flush: None,
            };
            let _ = s.call("section::chamfer_section", || {
                section::chamfer_section(&plain, arg(k), outside)
            });
        }
    }
}

/// The master spline: hostile angles, teeth and turns through every function, on straight-sided
/// shafts of every count of teeth the sweep picks, an involute shaft with its flanks and fillets
/// fitted as splines, and a serration. The plausible angles stay from -360 to 360, so that they
/// reach the pair and are not refused at once.
fn sweep_master_spline(s: &mut Sweep, dice: &mut Dice) {
    let involute_shaft = External {
        module: 1.5,
        teeth: 20,
        pressure_angle: 30.0,
        addendum: 0.5,
        dedendum: 0.85,
        shift: 0.0,
        tool_tip_radius: 0.4,
        thinning: 0.0,
    };
    let involute_fit = Fit {
        side_clearance: 0.06,
        hub_addendum: 0.5,
        tip_clearance: 0.2,
        hub_fillet_radius: 0.3,
        form_clearance: 0.05,
    };
    // Made at the share of the fit's tolerance that a fit's source must keep.
    let source = 1e-2 * fit::SOURCE_SHARE;
    let involute_pair =
        involute::spline_pair(&involute_shaft, &involute_fit, source).map(|mut pair| {
            pair.first.outline = fit::fitted(&pair.first.outline, 1e-2).expect("the shaft fitted");
            pair.second.outline = fit::fitted(&pair.second.outline, 1e-2).expect("the hub fitted");
            pair
        });
    let serration = Serration {
        teeth: 36,
        major_diameter: 20.0,
        minor_diameter: 18.8,
        flank_angle: 90.0,
        tip: TipCorner::Round { radius: 0.1 },
        root_round: 0.03,
    };
    let serration_fit = straight::Fit {
        side_clearance: 0.02,
        major_clearance: 0.05,
        minor_clearance: 0.05,
        hub_root_round: 0.03,
        hub_tip_round: 0.05,
    };
    let serration_pair = straight::serration_pair(&serration, &serration_fit, TOL);
    let fixed = [
        involute_pair.expect("the involute pair"),
        serration_pair.expect("the serrated pair"),
    ];
    for round in 0..300u32 {
        let shaft = StraightSpline {
            teeth: if round % 3 == 0 {
                dice.pick(&HOSTILE_COUNTS)
            } else {
                2 + (round % 9)
            },
            major_diameter: 28.0,
            minor_diameter: 23.0,
            tooth_width: if round % 5 == 0 {
                dice.pick(&HOSTILE)
            } else {
                3.0
            },
            root_round: 0.3,
            tip: TipCorner::Chamfer { size: 0.3 },
        };
        let fit = straight::Fit {
            side_clearance: 0.05,
            major_clearance: 0.3,
            minor_clearance: 0.2,
            hub_root_round: 0.2,
            hub_tip_round: 0.3,
        };
        let form = if round % 2 == 0 {
            MasterForm::ToothLeftOut
        } else {
            MasterForm::WideTooth
        };
        let angle = if round % 4 == 0 {
            dice.pick(&HOSTILE)
        } else {
            (f64::from(round) * 7.3) % 720.0 - 360.0
        };
        let _ = s.call("master_spline::Form::circle", || form.circle());
        let _ = s.checked("master_spline::placement", || {
            master_spline::placement(dice.pick(&HOSTILE_COUNTS), form, angle).map(|p| {
                assert!(p.turn.is_finite() && p.middle.is_finite(), "{p:?}");
            })
        });
        // A straight-sided shaft two rounds in three, the involute and the serration in turn on the
        // third.
        let pair = match (round % 3, round % 2) {
            (2, 0) => fixed[0].clone(),
            (2, _) => fixed[1].clone(),
            _ => {
                let Ok(pair) = straight::spline_pair(&shaft, &fit, TOL) else {
                    continue;
                };
                pair
            }
        };
        let circle = if round % 5 < 2 {
            Circle::Largest
        } else {
            Circle::Smallest
        };
        let material = if round % 2 == 0 {
            Material::Inside
        } else {
            Material::Outside
        };
        let _ = s.outline("master_spline::turned", || {
            master_spline::turned(&pair.first.outline, angle)
        });
        let _ = s.outline("master_spline::replaced_period", || {
            master_spline::replaced_period(
                &pair.second.outline,
                material,
                circle,
                angle.to_radians(),
            )
            .map(|r| r.outline)
        });
        let tooth = if round % 2 == 0 {
            dice.pick(&HOSTILE_COUNTS)
        } else {
            0
        };
        let mut odd = pair.clone();
        if round % 7 == 0 {
            odd.second_start = dice.pick(&HOSTILE);
        }
        let _ = s.pair("master_spline::master_pair", || {
            master_spline::master_pair(&odd, form, tooth).map(|m| m.0)
        });
        let _ = s.pair("master_spline::master_spline", || {
            master_spline::master_spline(&pair, form, angle).map(|m| m.pair)
        });
    }
}

/// The bases of the plan's sweep: a straight-sided spline, a serration and an involute spline, each
/// a shaft with its hub.
fn plan_bases() -> [SplineNumbers; 3] {
    let both = SplineNumbers {
        make: SplineMake::Both,
        length: 20.0,
        ..SplineNumbers::default()
    };
    [
        SplineNumbers {
            form: SplineForm::Straight,
            teeth: 6,
            major_diameter: Some(28.0),
            minor_diameter: Some(23.0),
            tooth_width: Some(6.0),
            tip_size: Some(0.3),
            side_clearance: Some(0.05),
            major_clearance: Some(0.3),
            minor_clearance: Some(0.2),
            hub_diameter: Some(40.0),
            ..both.clone()
        },
        SplineNumbers {
            form: SplineForm::Serration,
            teeth: 36,
            major_diameter: Some(20.0),
            minor_diameter: Some(18.8),
            flank_angle: Some(90.0),
            tip_size: Some(0.05),
            side_clearance: Some(0.02),
            major_clearance: Some(0.05),
            minor_clearance: Some(0.05),
            hub_diameter: Some(30.0),
            ..both.clone()
        },
        SplineNumbers {
            form: SplineForm::Involute,
            teeth: 20,
            module: Some(1.5),
            side_clearance: Some(0.06),
            tip_clearance: Some(0.2),
            hub_diameter: Some(45.0),
            ..both
        },
    ]
}

/// How many accepted plans of involute teeth the sweep makes: their flanks and fillets are fitted
/// as splines, the slow part of a plan. Refused numbers are always met, as they are refused before
/// the fit.
const SLOW_PLANS: usize = 6;

/// The plan of a shaft spline: every number hostile in turn and several at once for each kind of
/// teeth, with each master spline, tip corner and choice of parts in turn. An accepted plan has
/// sound outlines and finite numbers, a refusal has words. Then the numbers a caller cuts with.
fn sweep_shaft_spline(s: &mut Sweep, dice: &mut Dice) {
    let numbers: [fn(&mut SplineNumbers, f64); 27] = [
        |n, v| n.length = v,
        |n, v| n.module = Some(v),
        |n, v| n.pressure_angle = Some(v),
        |n, v| n.addendum = Some(v),
        |n, v| n.dedendum = Some(v),
        |n, v| n.root_fillet = Some(v),
        |n, v| n.profile_shift = Some(v),
        |n, v| n.thinning = Some(v),
        |n, v| n.hub_addendum = Some(v),
        |n, v| n.tip_clearance = Some(v),
        |n, v| n.form_clearance = Some(v),
        |n, v| n.major_diameter = Some(v),
        |n, v| n.minor_diameter = Some(v),
        |n, v| n.tooth_width = Some(v),
        |n, v| n.flank_angle = Some(v),
        |n, v| n.root_round = Some(v),
        |n, v| n.tip_size = Some(v),
        |n, v| n.major_clearance = Some(v),
        |n, v| n.minor_clearance = Some(v),
        |n, v| n.hub_tip_round = Some(v),
        |n, v| n.side_clearance = Some(v),
        |n, v| n.hub_root_round = Some(v),
        |n, v| n.hub_diameter = Some(v),
        |n, v| n.master_angle = Some(v),
        |n, v| n.cutter_diameter = Some(v),
        |n, v| n.end_chamfer = Some(v),
        |n, v| n.entry_chamfer = Some(v),
    ];
    let counts: [fn(&mut SplineNumbers, u32); 1] = [|n, c| n.teeth = c];
    for base in plan_bases() {
        assert!(
            shaft_spline::plan(&base).is_ok(),
            "the base of the sweep is refused: {base:?}"
        );
        let involute = base.form == SplineForm::Involute;
        let mut slow = 0;
        for (k, mut n) in variants(base, &numbers, &counts, dice, 150)
            .into_iter()
            .enumerate()
        {
            match k % 6 {
                1 => n.master = SplineMaster::ToothOut,
                2 => n.master = SplineMaster::WideTooth,
                3 => n.tip_corner = SplineCorner::Chamfered,
                4 => n.tip_corner = SplineCorner::Rounded,
                5 => (n.make, n.hub_diameter) = (SplineMake::Shaft, None),
                _ => {}
            }
            // The numbers a spline cut on a shaft takes from the shaft, the shaft's radius hostile.
            let mut fitted = n.clone();
            s.call("shaft_spline::fit_to_shaft_within", || {
                shaft_spline::fit_to_shaft_within(&mut fitted, arg(k));
            });
            // What decides whether involute teeth are fitted is the shaft's own outline: once
            // enough plans have been made, the numbers whose shaft is accepted are left out.
            let shaft_is_made =
                involute && involute_shaft_of(&n).is_some_and(|shaft| shaft.outline(TOL).is_ok());
            if shaft_is_made && slow >= SLOW_PLANS {
                continue;
            }
            match s.checked("shaft_spline::plan", || shaft_spline::plan(&n)) {
                Ok(made) => {
                    slow += usize::from(involute);
                    sound("shaft_spline::plan", &made.pair.first.outline);
                    sound("shaft_spline::plan", &made.pair.second.outline);
                    let m = &made.made;
                    let sizes = [
                        m.shaft_major_diameter,
                        m.shaft_minor_diameter,
                        m.hub_major_diameter,
                        m.hub_minor_diameter,
                        m.side_clearance_each,
                        m.tip_gap,
                        m.root_gap,
                        m.turning_play,
                        made.pair.second_start,
                    ];
                    assert!(sizes.iter().all(|v| v.is_finite()), "{n:?} gave {m:?}");
                }
                Err(e) => assert!(
                    matches!(e.teeth_failure(), Some(TeethFailure::Refused(_))),
                    "{n:?}"
                ),
            }
        }
    }
    for (k, &v) in HOSTILE.iter().enumerate() {
        let _ = s.call("shaft_spline::same_size", || shaft_spline::same_size(v));
        let _ = s.call("shaft_spline::tool_margin", || shaft_spline::tool_margin(v));
        let radius = shaft_spline::cutter_radius;
        let found = [
            s.checked("shaft_spline::cutter_radius", || radius(v, 11.5, 14.0, 0.1)),
            s.checked("shaft_spline::cutter_radius", || {
                radius(arg(k + 3), v, 14.0, 0.1)
            }),
            s.checked("shaft_spline::cutter_radius", || {
                radius(arg(k + 3), 11.5, v, 0.1)
            }),
            s.checked("shaft_spline::cutter_radius", || {
                radius(arg(k + 3), 11.5, 14.0, v)
            }),
        ];
        for made in found.into_iter().flatten() {
            assert!(made.is_finite(), "a cutter radius at {v}");
        }
        let module = shaft_spline::module_for_major_radius;
        let name = "shaft_spline::module_for_major_radius";
        let modules = [
            s.call(name, || module(v, 0.5, 0.0, 15.0)),
            s.call(name, || module(20.0, v, 0.0, 15.0)),
            s.call(name, || module(20.0, 0.5, v, 15.0)),
            s.call(name, || module(20.0, 0.5, 0.0, v)),
            s.call(name, || module(arg(k + 1), arg(k + 2), arg(k + 3), v)),
        ];
        for made in modules.into_iter().flatten() {
            assert!(made.is_finite() && made > 0.0, "a module at {v}: {made}");
        }
    }
}

/// The involute shaft the plan would make of the numbers, read the way the plan reads them, where
/// the numbers give a module.
fn involute_shaft_of(n: &SplineNumbers) -> Option<External> {
    Some(External {
        module: n.module?,
        teeth: n.teeth,
        pressure_angle: n.pressure_angle.unwrap_or(30.0),
        addendum: n.addendum.unwrap_or(0.5),
        dedendum: n.dedendum.unwrap_or(0.85),
        shift: n.profile_shift.unwrap_or(0.0),
        tool_tip_radius: n.root_fillet.unwrap_or(0.4),
        thinning: n.thinning.unwrap_or(0.0),
    })
}

/// What every accepted outline of a splined shaft or hub must be. Returns whether one was accepted.
fn sound_with_teeth(outline: crate::Result<Outline>) -> bool {
    let Ok(outline) = outline else { return false };
    assert!(outline.teeth() >= 2);
    for piece in outline.tooth() {
        if let Piece::Curve { points, .. } = piece {
            assert!(points.len() >= 2 && points.iter().flatten().all(|v| v.is_finite()));
        }
    }
    sound("a plausible outline", &outline);
    true
}

#[test]
fn plausible_numbers_give_sound_outlines_or_a_refusal() {
    let mut accepted = 0_u32;
    for teeth in [3, 4, 5, 6, 8, 11, 17, 40, 200] {
        for pressure_angle in [14.5, 20.0, 25.0, 30.0, 37.5, 45.0] {
            for shift in [-0.6, -0.2, 0.0, 0.3, 0.8] {
                for (addendum, dedendum) in [(1.0, 1.25), (0.5, 0.6), (0.8, 1.0), (1.2, 1.5)] {
                    for tool_tip_radius in [0.0, 0.1, 0.25, 0.4] {
                        let e = External {
                            module: 1.75,
                            teeth,
                            pressure_angle,
                            addendum,
                            dedendum,
                            shift,
                            tool_tip_radius,
                            thinning: 0.02,
                        };
                        accepted += u32::from(sound_with_teeth(e.outline(1e-4)));
                    }
                }
            }
        }
    }
    assert!(accepted > 1500, "{accepted}");
}

// -------------------------------------------------------------------------------------------------
// The planner's refusals as data, against the words it gave before they were typed
// -------------------------------------------------------------------------------------------------

/// The planner's names of the numbers the tooth modules check, as it held them before its refusals
/// were typed.
type OldNumbers = &'static [(&'static str, &'static str)];
/// The planner's words of the tooth modules' reasons, as it held them before its refusals were
/// typed: the first fragment the reason contains counts.
type OldReasons = &'static [(&'static str, &'static str)];

const OLD_INVOLUTE_SHAFT: (OldNumbers, OldReasons) = (
    &[
        ("module", "module"),
        ("teeth", "teeth"),
        ("pressure angle", "pressure_angle"),
        ("addendum", "addendum"),
        ("dedendum", "dedendum"),
        ("profile shift", "profile_shift"),
        ("tool tip radius", "root_fillet"),
        ("thinning", "thinning"),
    ],
    &[
        ("come to a point", "addendum"),
        (
            "tip circle is at or below the start of the flank",
            "addendum",
        ),
        ("tool's tip radius is too large", "root_fillet"),
        (
            "root must lie below the pitch circle by more",
            "root_fillet",
        ),
        (
            "root fillets of neighbouring teeth would cross",
            "root_fillet",
        ),
        ("root fillet and the flank would not meet", "root_fillet"),
        ("undercut would cut the tooth through", "profile_shift"),
        ("root circle would be inside the axis", "dedendum"),
        ("thinner than nothing or as wide as the pitch", "thinning"),
    ],
);

const OLD_INVOLUTE_FIT: (OldNumbers, OldReasons) = (
    &[
        ("side clearance", "side_clearance"),
        ("hub addendum", "hub_addendum"),
        ("tip clearance", "tip_clearance"),
        ("form clearance", "form_clearance"),
        ("fillet radius", "hub_root_round"),
        ("space width", "side_clearance"),
        ("minor radius", "hub_addendum"),
        ("major radius", "tip_clearance"),
    ],
    &[
        (
            "hub's teeth would reach into the shaft's root fillet",
            "hub_addendum",
        ),
        (
            "hub's root rounds would reach the shaft's tip corners",
            "hub_root_round",
        ),
        ("spaces would be as wide as the pitch", "side_clearance"),
        (
            "tips of the teeth would be below the base circle",
            "hub_addendum",
        ),
        (
            "roots must lie outside the tips by more than the fillet radius",
            "hub_root_round",
        ),
        ("spaces would meet at the tips", "side_clearance"),
        ("root rounds are too large", "hub_root_round"),
    ],
);

const OLD_STRAIGHT_SHAFT: (OldNumbers, OldReasons) = (
    &[
        ("teeth", "teeth"),
        ("major diameter", "major_diameter"),
        ("minor diameter", "minor_diameter"),
        ("tooth width", "tooth_width"),
        ("flank angle", "flank_angle"),
        ("root round", "root_round"),
        ("chamfer size", "tip_size"),
        ("tip round radius", "tip_size"),
    ],
    &[
        ("flank does not cross both circles", "tooth_width"),
        ("chamfer is too large", "tip_size"),
        ("round at the major circle is too large", "tip_size"),
        ("round at the minor circle is too large", "root_round"),
        ("corners of a tooth would meet", "tip_size"),
        ("neighbouring teeth would meet at the root", "tooth_width"),
        ("corners leave no straight flank", "tip_size"),
        ("minor diameter must be smaller", "minor_diameter"),
    ],
);

const OLD_SERRATION_SHAFT: (OldNumbers, OldReasons) = (
    OLD_STRAIGHT_SHAFT.0,
    &[
        ("flank does not cross both circles", "flank_angle"),
        ("chamfer is too large", "tip_size"),
        ("round at the major circle is too large", "tip_size"),
        ("round at the minor circle is too large", "root_round"),
        ("corners of a tooth would meet", "tip_size"),
        ("neighbouring teeth would meet at the root", "flank_angle"),
        ("corners leave no straight flank", "tip_size"),
        ("minor diameter must be smaller", "minor_diameter"),
    ],
);

const OLD_STRAIGHT_FIT: (OldNumbers, OldReasons) = (
    &[
        ("side clearance", "side_clearance"),
        ("major clearance", "major_clearance"),
        ("minor clearance", "minor_clearance"),
        ("hub root round", "hub_root_round"),
        ("hub tip round", "hub_tip_round"),
        ("space width", "side_clearance"),
        ("root round", "hub_root_round"),
        ("tip round", "hub_tip_round"),
    ],
    &[
        (
            "minor clearance would take the hub's teeth out",
            "minor_clearance",
        ),
        ("hub's root rounds would come closer", "hub_root_round"),
        ("tips of the hub's teeth would come closer", "hub_tip_round"),
        ("hub's root rounds would meet", "hub_root_round"),
        ("hub's teeth would be too narrow", "side_clearance"),
        ("round at the major circle is too large", "hub_root_round"),
        ("round at the minor circle is too large", "hub_tip_round"),
        ("corners leave no straight flank", "hub_root_round"),
        ("flank does not cross both circles", "side_clearance"),
        (
            "neighbouring teeth would meet at the root",
            "side_clearance",
        ),
        ("can turn freely", "side_clearance"),
    ],
);

/// The master spline's two reasons as the planner matched them, letter for letter.
const OLD_NO_SMALLEST: &str =
    "no corner of the teeth lies on the smallest circle: no tooth or space can be closed there";
const OLD_NO_LARGEST: &str = "no corner of the teeth lies on the largest circle: the tops of the \
                              teeth and the bottoms of the spaces need a flat part to join";

/// A tooth module's failure as the planner read it before the failures were typed.
#[derive(Clone, Copy, Debug)]
enum OldError {
    OutOfRange {
        what: &'static str,
        value: f64,
        above: f64,
        up_to: f64,
    },
    Invalid {
        why: &'static str,
    },
}

/// The planner's number in plain words, as it was.
fn old_plain(x: f64) -> String {
    let s = format!("{x:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s != "0" && s != "-0" {
        s.to_string()
    } else if x != 0.0 && x.is_finite() {
        format!("{x:e}")
    } else {
        "0".to_string()
    }
}

/// The planner's key of a refusal, as it was: the first word in backticks, when it is lowercase or
/// `_`.
fn old_key(words: &str) -> Option<String> {
    let is_key = |k: &&str| !k.is_empty() && k.chars().all(|c| c.is_ascii_lowercase() || c == '_');
    words.split('`').nth(1).filter(is_key).map(str::to_string)
}

/// The planner's words for a tooth module's failure, as they were.
fn old_refused(tables: &[(OldNumbers, OldReasons)], e: OldError) -> String {
    match e {
        OldError::OutOfRange {
            what,
            value,
            above,
            up_to,
        } => {
            let key = tables
                .iter()
                .flat_map(|(numbers, _)| numbers.iter())
                .find(|(name, _)| *name == what)
                .map(|(_, key)| *key);
            let name = key.map_or_else(|| format!("the {what}"), |key| format!("`{key}`"));
            // A lower limit a hair above nothing is "more than 0" in plain words.
            let above = if above > 0.0 && above < 1e-4 {
                0.0
            } else {
                above
            };
            if !value.is_finite() {
                format!("{name} must be a number")
            } else if value > up_to {
                format!(
                    "{name} must be at most {}, not {}",
                    old_plain(up_to),
                    old_plain(value)
                )
            } else if value == up_to {
                format!("{name} must be less than {}", old_plain(up_to))
            } else if value < 0.0 && above == 0.0 {
                format!("{name} cannot be negative, not {}", old_plain(value))
            } else {
                format!(
                    "{name} must be more than {}, not {}",
                    old_plain(above),
                    old_plain(value)
                )
            }
        }
        OldError::Invalid { why } => match tables
            .iter()
            .flat_map(|(_, reasons)| reasons.iter())
            .find(|(words, _)| why.contains(words))
        {
            Some((_, key)) => format!(
                "`{key}`: {}",
                why.replace("the tool's tip radius", "the root fillet")
            ),
            None => why.to_string(),
        },
    }
}

/// The planner's words for a failure of the master spline, as they were.
fn old_master_refused(e: OldError) -> String {
    match e {
        OldError::Invalid { why } if why == OLD_NO_SMALLEST => format!(
            "`master_spline`: a tooth cannot be left out of these teeth: {why}; make a wide tooth \
             instead (wide_tooth), or make the shaft's root rounds and the hub's tip rounds smaller"
        ),
        OldError::Invalid { why } if why == OLD_NO_LARGEST => format!(
            "`master_spline`: a wide tooth cannot be made on these teeth: {why}; leave a tooth out \
             instead (tooth_out), or use plain or chamfered tip corners and smaller root rounds in \
             the hub"
        ),
        other => old_refused(&[(&[("teeth", "teeth")], &[])], other),
    }
}

/// Every name a tooth module gives a number it refuses, and one that no table knows.
const WHATS: &[&str] = &[
    "tolerance",
    "module",
    "pressure angle",
    "addendum",
    "dedendum",
    "profile shift",
    "thinning",
    "space width",
    "minor radius",
    "major radius",
    "hub addendum",
    "tip clearance",
    "radius",
    "reach",
    "largest radius",
    "turn",
    "major diameter",
    "minor diameter",
    "tooth width",
    "flank angle",
    "tool tip radius",
    "fillet radius",
    "side clearance",
    "form clearance",
    "chamfer size",
    "tip round radius",
    "major clearance",
    "minor clearance",
    "hub root round",
    "hub tip round",
    "root round",
    "tip round",
    "degree",
    "teeth",
    "points of the outline",
    "tooth",
    "steps",
    "nothing known",
];

/// The limits a refused number is given against: above nothing, above a number, a hair above
/// nothing (named "more than 0"), above a negative number.
const BOUNDS: &[(f64, f64)] = &[
    (0.0, 1000.0),
    (2.0, 2000.0),
    (1e-5, 100_000.0),
    (1e-9, 3.0),
    (-1.0, 19.0),
];

/// A set of the planner's tables, as it asks them.
type Tables = &'static [(Numbers, Reasons)];

/// The same set as it was.
type OldTables = &'static [(OldNumbers, OldReasons)];

/// The table sets the planner asks, each with its copy as it was; the last is the master spline's
/// fallback.
const SETS: &[(Tables, OldTables)] = &[
    (&[INVOLUTE_SHAFT], &[OLD_INVOLUTE_SHAFT]),
    (
        &[INVOLUTE_FIT, INVOLUTE_SHAFT],
        &[OLD_INVOLUTE_FIT, OLD_INVOLUTE_SHAFT],
    ),
    (&[STRAIGHT_SHAFT], &[OLD_STRAIGHT_SHAFT]),
    (&[SERRATION_SHAFT], &[OLD_SERRATION_SHAFT]),
    (
        &[STRAIGHT_FIT, STRAIGHT_SHAFT],
        &[OLD_STRAIGHT_FIT, OLD_STRAIGHT_SHAFT],
    ),
    (
        &[STRAIGHT_FIT, SERRATION_SHAFT],
        &[OLD_STRAIGHT_FIT, OLD_SERRATION_SHAFT],
    ),
    (
        &[(&[("teeth", "teeth")], &[])],
        &[(&[("teeth", "teeth")], &[])],
    ),
];

/// The planner's refusals as data give the keys and the words it gave when it built them from
/// words: every reason and every refused number through every table set it asks, and the master
/// spline's refusals, against a copy of the planner as it was; then each refusal of the planner's
/// own checks against its words written out. The new refusals (a bound not proved) have no old
/// counterpart and are asserted as they are meant.
#[test]
fn the_planner_gives_todays_keys_and_words() {
    let mut compared = 0_usize;
    let mut mismatches: Vec<String> = Vec::new();
    let mut compare = |case: &dyn Fn() -> String, new: TeethFailure, old: &str| {
        compared += 1;
        let found = (new.key().map(str::to_string), new.to_string());
        let wanted = (old_key(old), old.to_string());
        if found != wanted {
            mismatches.push(format!("{}: {found:?}, before {wanted:?}", case()));
        }
    };
    for (set, &(tables, old_tables)) in SETS.iter().enumerate() {
        for &why in Why::ALL {
            let old = old_refused(old_tables, OldError::Invalid { why: why.words() });
            compare(
                &|| format!("set {set}, {why:?}"),
                refused(tables, TeethFailure::Invalid(why)),
                &old,
            );
        }
        for &what in WHATS {
            for &(above, up_to) in BOUNDS {
                for value in [
                    f64::NAN,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    up_to + 1.0,
                    up_to,
                    -1.0,
                    above - 1.0,
                    above,
                    0.5 * (above + up_to),
                    above + 1e-5,
                    1e-5,
                ] {
                    let new = TeethFailure::OutOfRange {
                        what,
                        value,
                        above,
                        up_to,
                    };
                    let old = OldError::OutOfRange {
                        what,
                        value,
                        above,
                        up_to,
                    };
                    compare(
                        &|| format!("set {set}, {what} {value} above {above} up to {up_to}"),
                        refused(tables, new),
                        &old_refused(old_tables, old),
                    );
                }
            }
        }
    }
    for &why in Why::ALL {
        let old = old_master_refused(OldError::Invalid { why: why.words() });
        compare(
            &|| format!("master spline, {why:?}"),
            master_refused(TeethFailure::Invalid(why)),
            &old,
        );
    }
    for value in [f64::NAN, 1.0, 2001.0, 2000.0] {
        let new = TeethFailure::OutOfRange {
            what: "teeth",
            value,
            above: 1.0,
            up_to: 2000.0,
        };
        let old = OldError::OutOfRange {
            what: "teeth",
            value,
            above: 1.0,
            up_to: 2000.0,
        };
        compare(
            &|| format!("master spline, teeth {value}"),
            master_refused(new),
            &old_master_refused(old),
        );
    }
    let per_set = Why::ALL.len() + WHATS.len() * BOUNDS.len() * 11;
    let expected = SETS.len() * per_set + Why::ALL.len() + 4;
    assert_eq!(compared, expected, "every case is compared");
    assert!(
        mismatches.is_empty(),
        "{} of {compared} refusals differ from the words before: {:?}",
        mismatches.len(),
        mismatches.iter().take(8).collect::<Vec<_>>()
    );

    // A bound that could not be proved: the root fillets' separation names the root fillet, a
    // chord or a fitted spline's distance names no number. Each is a refusal of the kind
    // "unsupported", as an undecided certificate is.
    for (set, &(tables, _)) in SETS.iter().enumerate() {
        for what in [CHORD, FIT_DISTANCE] {
            let unproved = refused(
                tables,
                TeethFailure::NotProved {
                    what,
                    asked: 1e-6,
                    reached: 3e-6,
                },
            );
            assert!(
                matches!(
                    unproved,
                    TeethFailure::Refused(PlanRefusal::Unproved { key: None, .. })
                ),
                "set {set}: {unproved:?}"
            );
            assert_eq!(unproved.key(), None, "set {set}");
            assert_eq!(unproved.kind(), ErrorKind::Unsupported, "set {set}");
        }
    }
    // The two involute sets: the shaft's, and the hub's before the shaft's.
    for &(tables, _) in SETS.iter().take(2) {
        let unproved = refused(
            tables,
            TeethFailure::NotProved {
                what: FILLET_SEPARATION,
                asked: 0.0,
                reached: -1e-12,
            },
        );
        assert!(
            matches!(
                unproved,
                TeethFailure::Refused(PlanRefusal::Unproved {
                    key: Some("root_fillet"),
                    what: FILLET_SEPARATION,
                    ..
                })
            ),
            "{unproved:?}"
        );
        assert_eq!(unproved.key(), Some("root_fillet"));
        assert_eq!(unproved.kind(), ErrorKind::Unsupported);
        assert!(
            unproved.to_string().starts_with("`root_fillet`: "),
            "{unproved}"
        );
    }

    // The planner's own checks, each against its words written out.
    let [straight_base, _, involute_base] = plan_bases();
    let made = shaft_spline::plan(&straight_base).expect("the straight-sided base");
    let (root, top) = made.pair.first.outline.radii();
    let margin = shaft_spline::tool_margin(top - root);
    let least = (top + margin) - root + margin;
    let cases: Vec<(SplineNumbers, &str, String)> = vec![
        (
            SplineNumbers {
                length: f64::NAN,
                ..straight_base.clone()
            },
            "length",
            "`length` must be a number".to_string(),
        ),
        (
            SplineNumbers {
                module: Some(f64::NAN),
                ..involute_base.clone()
            },
            "module",
            "`module` must be a number".to_string(),
        ),
        (
            SplineNumbers {
                module: None,
                ..involute_base.clone()
            },
            "module",
            "`module` is required (mm)".to_string(),
        ),
        (
            SplineNumbers {
                length: 0.0,
                ..straight_base.clone()
            },
            "length",
            "`length` must be from 0.001 to 100000 mm, not 0".to_string(),
        ),
        (
            SplineNumbers {
                make: SplineMake::Shaft,
                ..straight_base.clone()
            },
            "hub_diameter",
            "`hub_diameter` is the hub's outside diameter: give it only when `make` is hub or both"
                .to_string(),
        ),
        (
            SplineNumbers {
                hub_diameter: None,
                ..straight_base.clone()
            },
            "hub_diameter",
            "`hub_diameter` is required to make the hub (mm, its outside diameter)".to_string(),
        ),
        (
            SplineNumbers {
                master_angle: Some(10.0),
                ..straight_base.clone()
            },
            "master_angle",
            "`master_angle` is where the master spline sits: give it only with `master_spline` \
             tooth_out or wide_tooth"
                .to_string(),
        ),
        (
            SplineNumbers {
                master: SplineMaster::ToothOut,
                master_angle: Some(400.0),
                ..straight_base.clone()
            },
            "master_angle",
            "`master_angle` must be from -360 to 360 (degrees), not 400".to_string(),
        ),
        (
            SplineNumbers {
                end_chamfer: Some(-1.0),
                ..straight_base.clone()
            },
            "end_chamfer",
            "`end_chamfer` must be from 0 to 100000 mm, not -1".to_string(),
        ),
        (
            SplineNumbers {
                entry_chamfer: Some(2e5),
                ..straight_base.clone()
            },
            "entry_chamfer",
            "`entry_chamfer` must be from 0 to 100000 mm, not 200000".to_string(),
        ),
        (
            SplineNumbers {
                end_chamfer_distance2: Some(0.5),
                ..straight_base.clone()
            },
            "end_chamfer_distance2",
            "`end_chamfer_distance2` shapes the chamfer `end_chamfer` sets: give it only with \
             `end_chamfer`"
                .to_string(),
        ),
        (
            SplineNumbers {
                entry_chamfer_angle: Some(30.0),
                ..straight_base.clone()
            },
            "entry_chamfer_angle",
            "`entry_chamfer_angle` shapes the chamfer `entry_chamfer` sets: give it only with \
             `entry_chamfer`"
                .to_string(),
        ),
        (
            SplineNumbers {
                end_chamfer: Some(0.5),
                end_chamfer_distance2: Some(0.5),
                end_chamfer_angle: Some(30.0),
                ..straight_base.clone()
            },
            "end_chamfer_distance2",
            "`end_chamfer_distance2` and `end_chamfer_angle` both set how far the chamfer \
             `end_chamfer` reaches across the end: give one of them, not both"
                .to_string(),
        ),
        (
            SplineNumbers {
                entry_chamfer: Some(0.5),
                entry_chamfer_distance2: Some(0.0),
                ..straight_base.clone()
            },
            "entry_chamfer_distance2",
            "`entry_chamfer_distance2` must be more than 0 and at most 100000 mm, not 0"
                .to_string(),
        ),
        (
            SplineNumbers {
                end_chamfer: Some(0.5),
                end_chamfer_distance2: Some(f64::NAN),
                ..straight_base.clone()
            },
            "end_chamfer_distance2",
            "`end_chamfer_distance2` must be a number of more than 0 and at most 100000 mm"
                .to_string(),
        ),
        (
            SplineNumbers {
                end_chamfer: Some(0.5),
                end_chamfer_angle: Some(90.0),
                ..straight_base.clone()
            },
            "end_chamfer_angle",
            "`end_chamfer_angle` must be more than 0 and less than 90 (degrees), not 90"
                .to_string(),
        ),
        (
            SplineNumbers {
                entry_chamfer: Some(0.5),
                entry_chamfer_angle: Some(f64::NEG_INFINITY),
                ..straight_base.clone()
            },
            "entry_chamfer_angle",
            "`entry_chamfer_angle` must be a number of more than 0 and less than 90 (degrees)"
                .to_string(),
        ),
        (
            SplineNumbers {
                cutter_diameter: Some(2e5),
                ..straight_base.clone()
            },
            "cutter_diameter",
            "`cutter_diameter` must be a number of at most 100000 mm".to_string(),
        ),
        (
            SplineNumbers {
                cutter_diameter: Some(1.0),
                ..straight_base.clone()
            },
            "cutter_diameter",
            format!(
                "`cutter_diameter` must be more than {} mm: the cutter's radius must reach past \
                 the depth of the teeth",
                old_plain(2.0 * least)
            ),
        ),
        (
            SplineNumbers {
                hub_diameter: Some(2e5),
                ..straight_base.clone()
            },
            "hub_diameter",
            "`hub_diameter` must be at most 100000 mm, not 200000".to_string(),
        ),
        (
            SplineNumbers {
                hub_diameter: Some(28.7),
                ..straight_base.clone()
            },
            "hub_diameter",
            "`hub_diameter` leaves no rim around the hub's teeth: it must be more than 31.4 mm"
                .to_string(),
        ),
        (
            SplineNumbers {
                tooth_width: Some(-1.0),
                ..straight_base.clone()
            },
            "tooth_width",
            "`tooth_width` cannot be negative, not -1".to_string(),
        ),
    ];
    for (n, key, words) in cases {
        let planned = shaft_spline::plan(&n);
        assert_eq!(refusal(&planned), Some((Some(key), words.clone())), "{n:?}");
        assert_eq!(
            operation(&planned),
            Some("teeth::shaft_spline::plan"),
            "{words}"
        );
        let failure = planned
            .as_ref()
            .err()
            .and_then(crate::SolidError::teeth_failure);
        assert!(
            matches!(failure, Some(TeethFailure::Refused(_))),
            "{words}: {failure:?}"
        );
        assert_eq!(
            failure.map(TeethFailure::kind),
            Some(ErrorKind::InvalidInput),
            "{words}"
        );
    }
}

/// Each chamfer as its numbers make it: one size both ways (45 degrees), a second distance across
/// the end, or the size along the face at an angle; none for a chamfer left out or of 0, whatever
/// else is given with a 0.
#[test]
fn the_plan_makes_each_chamfer_as_its_numbers_say() {
    let [base, _, _] = plan_bases();
    let made = |n: SplineNumbers| shaft_spline::plan(&n).expect("the chamfered spline");
    let plain = made(SplineNumbers {
        end_chamfer: Some(0.5),
        ..base.clone()
    });
    assert_eq!(
        plain.end_chamfer,
        Some(shaft_spline::ChamferSize {
            axial: 0.5,
            radial: 0.5
        })
    );
    assert_eq!(plain.entry_chamfer, None);
    let two = made(SplineNumbers {
        entry_chamfer: Some(0.4),
        entry_chamfer_distance2: Some(0.2),
        ..base.clone()
    });
    assert_eq!(
        two.entry_chamfer,
        Some(shaft_spline::ChamferSize {
            axial: 0.4,
            radial: 0.2
        })
    );
    let angled = made(SplineNumbers {
        end_chamfer: Some(0.5),
        end_chamfer_angle: Some(60.0),
        ..base.clone()
    });
    let end = angled.end_chamfer.expect("the angled chamfer");
    assert_eq!(end.axial, 0.5);
    assert!((end.radial - 0.5 * 3f64.sqrt()).abs() < 1e-12, "{end:?}");
    let none = made(SplineNumbers {
        end_chamfer: Some(0.0),
        end_chamfer_angle: Some(30.0),
        ..base.clone()
    });
    assert_eq!(none.end_chamfer, None);
    // Hostile second distances and angles: refused naming the number, never a panic.
    for &h in &HOSTILE {
        for n in [
            SplineNumbers {
                end_chamfer: Some(0.5),
                end_chamfer_distance2: Some(h),
                ..base.clone()
            },
            SplineNumbers {
                entry_chamfer: Some(0.5),
                entry_chamfer_angle: Some(h),
                ..base.clone()
            },
        ] {
            if let Err(e) = shaft_spline::plan(&n) {
                let key = e.teeth_failure().and_then(TeethFailure::key);
                assert!(
                    matches!(key, Some("end_chamfer_distance2" | "entry_chamfer_angle")),
                    "{h}: {e}"
                );
            }
        }
    }
}
