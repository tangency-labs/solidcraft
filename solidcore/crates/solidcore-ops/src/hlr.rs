//! Exact orthographic hidden lines for placed record bodies, and the silhouettes of their curved
//! faces: the kernel's [`solidcore::hlr`] on the record. A body is validated, copied into the
//! kernel's kinds through `solidcore-convert` (a blend face as its NURBS form: the blend's
//! definition is no line of a drawing) and prepared once; scenes of its placements are drawn from
//! it. The drawing comes back in the record's vectors and segments.
//!
//! Nothing here computes: every answer is the kernel's, its numbers copied field by field.

use serde::Serialize;
use solidcore::hlr as k;
use solidcore_convert::{record_seg2, record_vec3, vec3};
use solidcore_record::brep::{Brep, Loop, Surface};
use solidcore_record::{Seg2, Vec3};

pub use k::{
    EdgeKind, HlrError, Options, PrepareReport, Prepared, Serial, Source, Stats, Unsupported,
    Visibility, Workers, MAX_COORDINATE, MAX_ITEMS, MAX_THREADS, MAX_TRIM_SAMPLES,
};

/// How exactly a drawn curve stands for its source: the record's data type
/// ([`solidcore_record::CurveAccuracy`]).
pub use solidcore_record::CurveAccuracy;

/// The same accuracy for a curve scaled by `factor` (above 0): a deviation scales with it, and
/// an analytic curve stays analytic ([`k::CurveAccuracy::scaled`]).
#[must_use]
pub fn scaled(accuracy: CurveAccuracy, factor: f64) -> CurveAccuracy {
    self::accuracy(kernel_accuracy(accuracy).scaled(factor))
}

fn kernel_accuracy(a: CurveAccuracy) -> k::CurveAccuracy {
    match a {
        CurveAccuracy::Analytic => k::CurveAccuracy::Analytic,
        CurveAccuracy::Bounded { max_deviation_mm } => {
            k::CurveAccuracy::Bounded { max_deviation_mm }
        }
        CurveAccuracy::Sampled {
            max_sample_deviation_mm,
        } => k::CurveAccuracy::Sampled {
            max_sample_deviation_mm,
        },
    }
}

fn accuracy(a: k::CurveAccuracy) -> CurveAccuracy {
    match a {
        k::CurveAccuracy::Analytic => CurveAccuracy::Analytic,
        k::CurveAccuracy::Bounded { max_deviation_mm } => {
            CurveAccuracy::Bounded { max_deviation_mm }
        }
        k::CurveAccuracy::Sampled {
            max_sample_deviation_mm,
        } => CurveAccuracy::Sampled {
            max_sample_deviation_mm,
        },
    }
}

/// An orthographic view: `d` is the direction of sight; screen coordinates are `(p · x, p · y)`.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// Screen x.
    pub x: Vec3,
    /// Screen y.
    pub y: Vec3,
    /// The direction of sight.
    pub d: Vec3,
}

impl View {
    /// The view looking along `d` with `up` up the screen ([`k::View::new`]).
    ///
    /// # Errors
    /// A direction or up of no direction, or parallel ones.
    pub fn new(d: Vec3, up: Vec3) -> Result<View, HlrError> {
        k::View::new(vec3(d), vec3(up)).map(view_back)
    }
    /// `p` on the screen ([`k::View::screen`]).
    #[must_use]
    pub fn screen(&self, p: Vec3) -> [f64; 2] {
        kernel_view(self).screen(vec3(p))
    }
}

fn kernel_view(v: &View) -> k::View {
    k::View {
        x: vec3(v.x),
        y: vec3(v.y),
        d: vec3(v.d),
    }
}

fn view_back(v: k::View) -> View {
    View {
        x: record_vec3(v.x),
        y: record_vec3(v.y),
        d: record_vec3(v.d),
    }
}

/// One placed body: `world = rotation * local + translation`, rotation columns given.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    /// Into the prepared bodies.
    pub body: usize,
    /// The rotation's columns.
    pub rotation: [Vec3; 3],
    /// The translation.
    pub translation: Vec3,
}

fn kernel_placement(p: &Placement) -> k::Placement {
    k::Placement {
        body: p.body,
        rotation: p.rotation.map(vec3),
        translation: vec3(p.translation),
    }
}

/// A piece of a drawn line between the points where its visibility can change.
#[derive(Clone, Debug, Serialize)]
pub struct Piece {
    /// Parameter range on the source curve.
    pub t: [f64; 2],
    /// Whether it is seen.
    pub visibility: Visibility,
    /// Drawn by another piece: a hidden or uncertain piece lying on a visible one (hidden lines
    /// never show through visible ones), or a duplicate of a visible or hidden piece.
    pub covered: bool,
    /// The piece on screen (millimetres): lines and conic arcs where the projection is exact,
    /// chords otherwise.
    pub geometry: Vec<Seg2>,
    /// How exactly `geometry` stands for the projected curve.
    pub accuracy: CurveAccuracy,
}

/// One drawn curve: an edge or a silhouette of one placement, in pieces.
#[derive(Clone, Debug, Serialize)]
pub struct Line {
    /// Index into the scene's placements.
    pub placement: usize,
    /// What it draws.
    pub source: Source,
    /// Its pieces, in order along it.
    pub pieces: Vec<Piece>,
}

/// A scene's hidden-line drawing.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Drawing {
    /// The lines.
    pub lines: Vec<Line>,
    /// Faces outside the supported subset: their silhouettes are missing and what they could
    /// hide is uncertain.
    pub unsupported: Vec<Unsupported>,
    /// What the drawing took.
    pub stats: Stats,
}

/// One curved face's silhouette on screen, as [`silhouettes`] gives it.
#[derive(Clone, Debug)]
pub struct Silhouette {
    /// The first face of the faces on its surface, as [`Source::Silhouette`] names it.
    pub face: usize,
    /// Lines and conic arcs where the curve is exact, chords between `points` otherwise.
    pub geometry: Vec<Seg2>,
    /// How exactly `geometry` stands for the silhouette.
    pub accuracy: CurveAccuracy,
    /// Points along it, from its start to its end: on the curve itself where `accuracy` is
    /// analytic, on its sampled approximation (within `accuracy`) otherwise.
    pub points: Vec<[f64; 2]>,
}

/// The body `b` in the kernel's kinds. A blend face is its NURBS form.
pub(crate) fn kernel_body(b: &Brep) -> Result<k::body::Brep, HlrError> {
    let curve = |c| solidcore_convert::curve(c).map_err(|_| HlrError::Unsupported("edge curve"));
    let surface = |s: &Surface| match s {
        Surface::Blend { .. } => Err(HlrError::Unsupported(
            "a blend's label (draw its NURBS form)",
        )),
        s => solidcore_convert::surface(s).map_err(|_| HlrError::Unsupported("face surface")),
    };
    let edges = b
        .edges
        .iter()
        .map(|e| {
            Ok(k::body::Edge {
                curve: curve(&e.curve)?,
                nurbs: e.nurbs.as_ref().map(curve).transpose()?,
                interval: e.interval,
            })
        })
        .collect::<Result<_, HlrError>>()?;
    let faces = b
        .faces
        .iter()
        .map(|f| {
            // A blend's label has no surface of its own: its face is drawn as its NURBS form.
            let (label, form) = match (&f.surface, &f.nurbs) {
                (Surface::Blend { .. }, Some(net)) => (net, None),
                (label, form) => (label, form.as_ref()),
            };
            Ok(k::body::Face {
                surface: surface(label)?,
                nurbs: form.map(surface).transpose()?,
                domain: f.domain,
                loops: f
                    .loops
                    .iter()
                    .map(|l| match l {
                        Loop::Edges { edges } => k::body::Loop::Edges {
                            edges: edges
                                .iter()
                                .map(|u| k::body::OrientedEdge {
                                    edge: u.edge,
                                    forward: u.forward,
                                })
                                .collect(),
                        },
                        Loop::Vertex { vertex } => k::body::Loop::Vertex { vertex: *vertex },
                    })
                    .collect(),
            })
        })
        .collect::<Result<_, HlrError>>()?;
    let vertices = b
        .vertices
        .iter()
        .map(|v| k::body::Vertex { point: v.point })
        .collect();
    Ok(k::body::Brep {
        vertices,
        edges,
        faces,
    })
}

/// The body `b` prepared for drawing ([`prepare_cancellable`], never cancelled).
///
/// # Errors
/// See [`prepare_cancellable`].
pub fn prepare(b: &Brep, opts: &Options) -> Result<Prepared, HlrError> {
    prepare_cancellable(b, opts, &|| false)
}

/// The body `b` validated, copied into the kernel's kinds and prepared for drawing
/// ([`k::prepare_cancellable`]), polling `cancel`.
///
/// # Errors
/// An invalid record, a curve or surface the kernel's kinds don't hold, or the kernel's refusal.
pub fn prepare_cancellable(
    b: &Brep,
    opts: &Options,
    cancel: &dyn Fn() -> bool,
) -> Result<Prepared, HlrError> {
    crate::record_check::validate(b).map_err(|_| HlrError::Invalid("portable B-rep"))?;
    k::prepare_cancellable(&kernel_body(b)?, opts, cancel)
}

/// The hidden-line drawing of the scene `scene` of the bodies `defs` in `view`, on the calling
/// thread ([`k::hidden_lines`]).
///
/// # Errors
/// The kernel's refusal: an invalid placement or view, a budget exceeded, cancelled.
pub fn hidden_lines(
    defs: &[Prepared],
    scene: &[Placement],
    view: &View,
    opts: &Options,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<Drawing, HlrError> {
    hidden_lines_on(defs, scene, view, opts, cancel, &Serial)
}

/// [`hidden_lines`] on the threads `workers` runs ([`k::hidden_lines_on`]).
///
/// # Errors
/// As [`hidden_lines`].
pub fn hidden_lines_on(
    defs: &[Prepared],
    scene: &[Placement],
    view: &View,
    opts: &Options,
    cancel: &(dyn Fn() -> bool + Sync),
    workers: &dyn Workers,
) -> Result<Drawing, HlrError> {
    let scene: Vec<k::Placement> = scene.iter().map(kernel_placement).collect();
    let d = k::hidden_lines_on(defs, &scene, &kernel_view(view), opts, cancel, workers)?;
    Ok(Drawing {
        lines: d
            .lines
            .into_iter()
            .map(|l| Line {
                placement: l.placement,
                source: l.source,
                pieces: l
                    .pieces
                    .into_iter()
                    .map(|p| Piece {
                        t: p.t,
                        visibility: p.visibility,
                        covered: p.covered,
                        geometry: p.geometry.iter().map(record_seg2).collect(),
                        accuracy: accuracy(p.accuracy),
                    })
                    .collect(),
            })
            .collect(),
        unsupported: d.unsupported,
        stats: d.stats,
    })
}

/// The silhouettes of `def`'s curved faces placed by `p` and seen in `view`
/// ([`k::silhouettes`]).
///
/// # Errors
/// The kernel's refusal: an invalid placement or view, a budget exceeded.
pub fn silhouettes(
    def: &Prepared,
    p: &Placement,
    view: &View,
    opts: &Options,
) -> Result<Vec<Silhouette>, HlrError> {
    Ok(
        k::silhouettes(def, &kernel_placement(p), &kernel_view(view), opts)?
            .into_iter()
            .map(|s| Silhouette {
                face: s.face,
                geometry: s.geometry.iter().map(record_seg2).collect(),
                accuracy: accuracy(s.accuracy),
                points: s.points,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::brep::Curve;

    #[test]
    fn procedural_and_placed_records_are_not_supported_yet() {
        let o = Options::default();
        // A cylinder side stored as a placed revolution of its seam line, under a placement
        // that is not rigid.
        let (mut placed, _) = crate::body::make_cylinder([0.; 3], 4., 10.)
            .unwrap()
            .to_record()
            .unwrap();
        let side = placed
            .faces
            .iter_mut()
            .find(|f| matches!(f.surface, Surface::Cylinder { .. }))
            .unwrap();
        let profile = Box::new(Curve::Line {
            start: [2., 0., 0.],
            end: [2., 0., 10.],
        });
        let matrix = [
            [2., 0., 0., 0.],
            [0., 2., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ];
        side.surface = Surface::PlacedRevolution {
            profile,
            origin: [0.; 3],
            axis: [0., 0., 1.],
            matrix,
            forward: true,
        };
        assert!(matches!(
            prepare(&placed, &o),
            Err(HlrError::Unsupported("placed revolution surface"))
        ));
        // A box edge stored as the meeting of the planes y = 0 and z = 0, traced from a leader.
        let (mut traced, _) = crate::body::make_box([0.; 3], [1.; 3])
            .unwrap()
            .to_record()
            .unwrap();
        let on_x = |p: &[f64; 3]| p[1] == 0. && p[2] == 0.;
        let edge = traced
            .edges
            .iter_mut()
            .find(|e| matches!(&e.curve, Curve::Line { start, end } if on_x(start) && on_x(end)))
            .unwrap();
        let y0 = Box::new(Surface::Plane {
            origin: [0.; 3],
            u_axis: [1., 0., 0.],
            v_axis: [0., 0., 1.],
        });
        let z0 = Box::new(Surface::Plane {
            origin: [0.; 3],
            u_axis: [1., 0., 0.],
            v_axis: [0., 1., 0.],
        });
        let leader = Box::new(Curve::Polyline {
            points: vec![[0.; 3], [1., 0., 0.]],
        });
        edge.curve = Curve::SurfaceIntersection {
            surfaces: [y0, z0],
            leader,
            tolerance: 1e-6,
        };
        // No NURBS form to follow: a traced edge is drawn from its leader.
        edge.nurbs = None;
        assert!(matches!(
            prepare(&traced, &o),
            Err(HlrError::Unsupported("surface intersection curve"))
        ));
    }

    /// A box of edge `size` with all twelve edges rounded at `r`, drawn along (-1, -2, -3) with
    /// tangent edges off, against its outline in closed form.
    #[allow(clippy::disallowed_methods)] // closed-form oracle, not ops geometry
    fn rounded_box_outline(size: f64, r: f64) {
        use crate::blend::{blend, Blend, Corners};
        use solidcore_record::Vec2;
        use std::f64::consts::PI;

        // 6 planes, 12 rounds and 8 ball corners, each corner a patch on the sphere of radius r
        // about a corner of the inner box, of edge size - 2r.
        let b = crate::body::make_box([0.; 3], [size; 3]).unwrap();
        let all: Vec<usize> = (0..12).collect();
        blend(&b, Blend::Round, &all, &[r; 12], &[], Corners::Filled).unwrap();
        let (rec, _) = b.to_record().unwrap();
        let balls = rec
            .faces
            .iter()
            .filter(|f| matches!(f.surface, Surface::BallPatch { .. }))
            .count();
        assert_eq!((rec.faces.len(), rec.edges.len(), balls), (26, 48, 8));

        let opts = Options {
            tangent_edges: false,
            ..Options::default()
        };
        let def = prepare(&rec, &opts).unwrap();
        let surfaces: Vec<(&str, usize)> = def
            .report
            .surfaces
            .iter()
            .map(|(k, n)| (k.as_str(), *n))
            .collect();
        assert_eq!(surfaces, [("cylinder", 12), ("plane", 6), ("sphere", 8)]);
        assert!(def.report.freeform_faces.is_empty());
        assert_eq!(def.report.tangent_edges, 48);
        // Boundaries are trimmed in chords: 256 to each quarter circle of radius r, which sag
        // about 5e-6 r.
        let chord = def.report.max_trim_chord;
        assert!(chord > 0. && chord <= 5e-5 * r, "trim chord {chord}");

        // Seen along d = (-1, -2, -3) the convex body's outline is the inner box's outline, a
        // hexagon whose sides are its edges seen along d, each edge e of length s = size - 2r
        // drawn s sqrt(1 - (e . d)^2) long, twice each, grown by the circle of radius r: the
        // hexagon's six sides as lines from the rounds, at r from them, and the circle's whole
        // turn as arcs of radius r about its six corners from the ball corners. With tangent
        // edges off nothing else is drawn, and nothing is hidden.
        let view = View::new(Vec3::new(-1., -2., -3.), Vec3::new(0., 0., 1.)).unwrap();
        let scene = [Placement {
            body: 0,
            rotation: [
                Vec3::new(1., 0., 0.),
                Vec3::new(0., 1., 0.),
                Vec3::new(0., 0., 1.),
            ],
            translation: Vec3::new(0., 0., 0.),
        }];
        let d = hidden_lines(&[def], &scene, &view, &opts, &|| false).unwrap();
        assert!(d.unsupported.is_empty());
        let s = size - 2. * r;
        let corners: Vec<Vec2> = (0..8)
            .map(|i| {
                let at = |bit: usize| if i & bit == 0 { r } else { r + s };
                let [x, y] = view.screen(Vec3::new(at(1), at(2), at(4)));
                Vec2 { x, y }
            })
            .collect();
        // The corners lie on their labels' spheres and the rounds on theirs: a radius, a
        // position or a length is off by rounding only, far within a micrometre.
        let near = |x: f64, y: f64| (x - y).abs() <= 1e-6;
        let dist = |p: Vec2, q: Vec2| (q.x - p.x).hypot(q.y - p.y);
        // Distance from p to the segment from a to b.
        let to_segment = |p: Vec2, a: Vec2, b: Vec2| {
            let (ux, uy) = (b.x - a.x, b.y - a.y);
            let t = (((p.x - a.x) * ux + (p.y - a.y) * uy) / (ux * ux + uy * uy)).clamp(0., 1.);
            dist(
                p,
                Vec2 {
                    x: a.x + t * ux,
                    y: a.y + t * uy,
                },
            )
        };
        let on_a_side = |p: Vec2, q: Vec2| {
            corners.iter().any(|a| {
                corners.iter().any(|b| {
                    dist(*a, *b) > 0.
                        && near(to_segment(p, *a, *b), r)
                        && near(to_segment(q, *a, *b), r)
                })
            })
        };
        let mut centres: Vec<usize> = Vec::new();
        let mut centre = |o: Vec2| {
            let at = corners.iter().position(|c| near(dist(*c, o), 0.));
            assert!(at.is_some(), "arc about {o:?}, no corner of the inner box");
            centres.extend(at);
        };
        let (mut lines, mut arcs) = (0., 0.);
        // The outline lines' length with the undecided pieces at their ends (covered, below).
        let mut whole_lines = 0.;
        for l in &d.lines {
            assert!(
                matches!(l.source, Source::Silhouette { .. }),
                "{:?}",
                l.source
            );
            for p in &l.pieces {
                for seg in &p.geometry {
                    if let Seg2::Line { a, b } = *seg {
                        whole_lines += dist(a, b);
                    }
                }
            }
            for p in l.pieces.iter().filter(|p| !p.covered) {
                assert_eq!(p.visibility, Visibility::Visible);
                assert_eq!(p.accuracy, CurveAccuracy::Analytic);
                for seg in &p.geometry {
                    match *seg {
                        Seg2::Line { a, b } => {
                            assert!(on_a_side(a, b), "{seg:?} on no side of the hexagon");
                            lines += dist(a, b);
                        }
                        Seg2::Arc {
                            center,
                            radius,
                            sweep,
                            ..
                        } => {
                            assert!(near(radius, r), "arc radius {radius}");
                            centre(center);
                            arcs += radius * sweep.abs();
                        }
                        // A circular arc as a rational quadratic: its legs to the apex are equal,
                        // its weight is the cosine of half its sweep, a leg is r tan(sweep/2),
                        // and its centre lies past the chord's middle from the apex, as far from
                        // the apex as sqrt(r^2 + leg^2).
                        Seg2::Conic { a, apex, b, w } => {
                            let leg = dist(a, apex);
                            assert!(near(leg, dist(b, apex)), "{seg:?}");
                            // A piece that is a point (where the outline crosses a tangent
                            // edge) has no sweep and draws nothing.
                            if near(leg, 0.) {
                                continue;
                            }
                            let half = w.acos();
                            let radius = leg / half.tan();
                            assert!(near(dist(a, b), 2. * radius * half.sin()), "{seg:?}");
                            assert!(near(radius, r), "conic radius {radius}");
                            let m = Vec2 {
                                x: 0.5 * (a.x + b.x),
                                y: 0.5 * (a.y + b.y),
                            };
                            let k = radius.hypot(leg) / dist(m, apex);
                            centre(Vec2 {
                                x: apex.x + k * (m.x - apex.x),
                                y: apex.y + k * (m.y - apex.y),
                            });
                            arcs += 2. * half * radius;
                        }
                        ref other => panic!("outline piece {other:?}"),
                    }
                }
            }
        }
        // The arcs turn about the hexagon's six corners, the projections of the inner box's
        // corners off the axis of sight.
        centres.sort_unstable();
        centres.dedup();
        assert_eq!(centres.len(), 6, "arcs about corners {centres:?}");
        let sines = [13f64 / 14., 10. / 14., 5. / 14.].map(f64::sqrt);
        let hexagon = 2. * s * sines.iter().sum::<f64>();
        // The rounds' outline lines run the hexagon's sides whole: with the pieces at their ends
        // (MEASURED at size 1000: 1.1e-11 short with rounds built in closed form, exact
        // cylinders; 4.1e-8 short with the fillet sessions' rounds).
        assert!(
            near(whole_lines, hexagon),
            "lines {whole_lines} with their ends, hexagon {hexagon}"
        );
        // Each of the six lines ends on a ball corner tangent to its round, and where a ray
        // along the view from the line hits that corner's face within its boundary band (twice
        // its trims' sag plus 1e-7 of its box's diagonal, r sqrt 3: `hlr`'s classify) the
        // piece is undecided and drawn covered. Near a tangent junction that holds up to about
        // the band's width along the line (ARGUED: the ray grazes both faces there), so each of
        // the 12 ends may leave out up to that much (MEASURED: 7e-8 to 3e-7 per end at size
        // 1000, r 1).
        let ends = 12. * (2. * chord + 1e-7 * 3f64.sqrt() * r);
        assert!(
            lines <= hexagon + 1e-6 && lines >= hexagon - ends - 1e-6,
            "lines {lines}, hexagon {hexagon}, ends up to {ends}"
        );
        // A face is closed to its trim's chords: a silhouette runs on to within twice their sag
        // (plus 1e-9 of the face's size, at most 4r) of them, and they lie within one sag of the
        // boundary. The rounds' outlines end exactly, at the ends of their charts; a corner's
        // runs on along its sphere, past the boundary to the round along axis i, a great circle
        // square to i, which it crosses at the angle between the planes square to d and to i,
        // whose sine is sqrt(1 - d_i^2). Each axis's two outline lines end at four such
        // crossings.
        let slack = 3. * chord + 1e-9 * (1. + 4. * r);
        let past = 4. * slack * sines.iter().map(|s| 1. / s).sum::<f64>();
        let circle = 2. * PI * r;
        assert!(
            arcs >= circle - 1e-6 && arcs <= circle + past + 1e-6,
            "arcs {arcs}, circle {circle}, past it at most {past}"
        );
    }

    #[test]
    fn a_box_with_rounded_corners_is_drawn_with_its_ball_corners_as_spheres() {
        rounded_box_outline(10., 2.);
        // A small round on a large body: the corners lie on their labels' spheres, which a fit
        // to samples of patches this far from the body's centre would not find.
        rounded_box_outline(1000., 1.);
    }

    /// A deviation scales with the curve; an analytic curve stays analytic and a sampled zero
    /// stays sampled.
    #[test]
    fn accuracy_scales_with_the_curve() {
        assert_eq!(
            scaled(CurveAccuracy::Analytic, 0.5),
            CurveAccuracy::Analytic
        );
        assert_eq!(
            scaled(
                CurveAccuracy::Sampled {
                    max_sample_deviation_mm: 0.01
                },
                3.0
            ),
            CurveAccuracy::Sampled {
                max_sample_deviation_mm: 0.03
            }
        );
        assert_eq!(
            scaled(
                CurveAccuracy::Bounded {
                    max_deviation_mm: 0.01
                },
                2.0
            ),
            CurveAccuracy::Bounded {
                max_deviation_mm: 0.02
            }
        );
        assert_eq!(
            scaled(
                CurveAccuracy::Sampled {
                    max_sample_deviation_mm: 0.0
                },
                5.0
            ),
            CurveAccuracy::Sampled {
                max_sample_deviation_mm: 0.0
            }
        );
    }
}
