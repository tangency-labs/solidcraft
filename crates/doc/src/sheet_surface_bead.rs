//! Formed beads (`sheet.bead`) on a sheet (#6748): a stiffening bead pressed into one planar
//! panel along a straight centreline, kept on the panel as a form: its centreline in the panel's
//! chart, its section, and the face it stands out of. Across, it is a trapezoid with filleted
//! corners; along, each end ramps up from the face over the end length. The sheet keeps its
//! thickness: the skin that stands out (the raised skin) and the skin under it are concentric,
//! and both meet the panel's faces on one rectangle, the footprint. The folded solid leaves the
//! footprint open in its panel's faces and sews the bead's faces in (see
//! [`crate::sheet_surface_fold::solid`]). In the flat the blank stays flat: the flat pattern lists
//! the bead, and the DXF draws it.
//!
//! The bead's geometry is the kernel's (`solidcore::develop::bead`, through `solidcraft_develop`):
//! its section, its faces in space, its footprint and its outline in the flat. This module keeps
//! the bead on its panel, finds the panel, checks what lies on it and words the kernel's refusals.
//!
//! A bead's faces are named by the feature that pressed it, after its centreline's sketch curve
//! and what each face is (see [`face_role`]), never by where it lies or the order it was made in
//! (#7990).

use serde::{Deserialize, Serialize};
use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};
use solidcraft_develop::prelude::*;
use solidcraft_develop::{Bead, BeadAgainst, BeadFace, BeadOutline, BeadPart, BeadRefusal, BeadSection, Fold, FormEdge, FormSurface, Surface};
use solidcraft_kernel::{EdgeSpec, FaceSpec, SurfSpec};

use crate::sheet_surface::Piece;
use crate::{DocError, Result, SurfaceSheet};

/// Most beads on one sheet.
pub const MAX_BEADS: usize = 200;

/// A bead on a sheet, in its panel's chart (see the module notes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceBead {
    /// The `sheet.bead` feature that pressed it.
    pub feature: u64,
    /// The sketch curve its centreline is drawn on, by its id: its faces are named after it.
    #[serde(default)]
    pub curve: String,
    /// The panel it is pressed into.
    pub panel: usize,
    /// The centreline's start in the panel's chart: where the first end ramp's virtual sharp
    /// meets the face.
    pub a: Vec2,
    /// The centreline's end in the panel's chart.
    pub b: Vec2,
    /// It stands out of the panel's far face (the one a thickness into the material), else out
    /// of its near face (the sheet's surface).
    pub far: bool,
    /// From the face it stands out of to its crown on that side.
    pub depth: f64,
    /// Across, between the flanks' virtual sharps on that face.
    pub width: f64,
    /// The flanks' angle to the face (radians).
    pub angle: f64,
    /// The inner radius of the bends at the base.
    pub base_radius: f64,
    /// The inner radius of the bends at the crown.
    pub crown_radius: f64,
    /// Along the centreline, from an end's virtual sharp on the face to the virtual sharp where
    /// the ramp reaches the crown.
    pub end_length: f64,
}

impl SurfaceBead {
    /// The bead as the kernel takes it.
    fn record(&self) -> Bead {
        Bead {
            panel: self.panel,
            a: self.a,
            b: self.b,
            far: self.far,
            section: BeadSection {
                depth: self.depth,
                width: self.width,
                angle: self.angle,
                base_radius: self.base_radius,
                crown_radius: self.crown_radius,
                end_length: self.end_length,
            },
        }
    }

    /// Its footprint in its panel's chart, as a loop, on a sheet `t` thick; none where its section
    /// does not fit (the sheet's build refuses it, saying why).
    pub(crate) fn footprint_loop(&self, t: f64) -> Option<Loop2> {
        let [p0, p1, p2, p3] = solidcraft_develop::bead_footprint(&self.record(), t).ok()?;
        Some(Loop2 { segs: vec![Seg2::Line { a: p0, b: p1 }, Seg2::Line { a: p1, b: p2 }, Seg2::Line { a: p2, b: p3 }, Seg2::Line { a: p3, b: p0 }] })
    }

    /// The kernel's refusal of this bead, in words: what would fit.
    fn refused(&self, why: BeadRefusal) -> DocError {
        DocError::Invalid(match why {
            BeadRefusal::NotFinite => "a bead needs finite sizes on a sheet of positive thickness".into(),
            BeadRefusal::NoLength => "a bead's centreline has no length".into(),
            BeadRefusal::NotPositive => format!(
                "a bead needs a depth, a width and an end length above 0 (depth {}, width {}, end length {})",
                self.depth, self.width, self.end_length
            ),
            BeadRefusal::Angle { angle } => format!("a bead's flanks need an angle between 0° and 90° (got {:.3}°)", angle.to_degrees()),
            BeadRefusal::Radius => format!("a bead's radii must be at least {} mm", solidcraft_develop::MIN_BEAD_RADIUS),
            BeadRefusal::GentleBend { rise } => format!(
                "a bead's gentlest bend rises only {rise:e} mm, too little to build: make its flanks or its end ramps steeper, or its radii larger"
            ),
            BeadRefusal::ShallowFlanks { angle, min_depth } => format!(
                "a bead {} deep is too shallow for its radii at {:.3}°: at least {min_depth:.4} deep, or smaller radii",
                self.depth,
                angle.to_degrees()
            ),
            BeadRefusal::ShallowRamps { angle } => format!(
                "a bead {} deep is too shallow for its radii on its end ramps ({:.3}°): lengthen the end length or make the radii smaller",
                self.depth,
                angle.to_degrees()
            ),
            BeadRefusal::NoCrown { angle, min_width } => {
                format!("a bead {} wide leaves no crown at {:.3}°, {} deep: at least {min_width:.4} wide", self.width, angle.to_degrees(), self.depth)
            }
            BeadRefusal::TooShort { length, min_length } => {
                format!("a bead {length:.4} long is too short for its two end ramps: at least {min_length:.4}")
            }
            BeadRefusal::Crease { off } => format!("a bead's crease could not be traced (off by {off:e} mm)"),
            BeadRefusal::Unplaced => "a bead's panel has no place on the folded sheet".into(),
            BeadRefusal::CrownWidth { crown_width, width } => {
                format!("a bead's crown width ({crown_width}) must be at least 0 and less than its width ({width})")
            }
            BeadRefusal::Undecided => "not supported yet: a bead beside an outline the kernel cannot place it against".into(),
        })
    }
}

/// A bead face's role in its name: `crown`, `left.flank`, `start.ramp`, `right.base_bend`,
/// `end.crown_bend` and so on, then `.top` or `.bottom` (see [`BeadFace`]).
pub(crate) fn face_role(face: BeadFace) -> String {
    let piece = |piece: usize, middle: &'static str| match piece {
        0 => "base_bend",
        1 => middle,
        _ => "crown_bend",
    };
    let what = match face.part {
        BeadPart::Crown => "crown".to_string(),
        BeadPart::Across { left, piece: k } => format!("{}.{}", if left { "left" } else { "right" }, piece(k, "flank")),
        BeadPart::Along { start, piece: k } => format!("{}.{}", if start { "start" } else { "end" }, piece(k, "ramp")),
    };
    format!("{what}.{}", if face.top { "top" } else { "bottom" })
}

/// What a bead does to the folded solid: the footprint its panel's cell leaves open (in the
/// flat, clockwise as a cell's holes run) and the bead's faces that close it, in space, with
/// what each is. `bead` is its place among the sheet's beads.
pub(crate) struct Form {
    pub bead: usize,
    pub panel: usize,
    pub open: Vec<Seg2>,
    /// The footprint's middle in the flat.
    pub centre: Vec2,
    pub faces: Vec<(FaceSpec, BeadFace)>,
}

/// The folded solid's forms, one for each of the sheet's beads, after each is checked against
/// what the development leaves of its panel.
pub(crate) fn forms(sk: &SurfaceSheet, pieces: &[(Piece, Region2)], fold: &Fold) -> Result<Vec<Form>> {
    let t = sk.sheet.thickness;
    let mut out = Vec::with_capacity(sk.beads.len());
    for (bead, b) in sk.beads.iter().enumerate() {
        sk.check_bead(b, pieces, fold, None, &mut Vec::new())?;
        let form = fold.bead_form(&b.record(), t).map_err(|why| b.refused(why))?;
        let [c0, c1, c2, c3] = form.open;
        let open = vec![Seg2::Line { a: c0, b: c1 }, Seg2::Line { a: c1, b: c2 }, Seg2::Line { a: c2, b: c3 }, Seg2::Line { a: c3, b: c0 }];
        let faces = form
            .faces
            .into_iter()
            .map(|f| {
                let surface = match f.surface {
                    FormSurface::Plane => SurfSpec::Plane,
                    FormSurface::Extruded { a, b, mid, dir } => SurfSpec::Extruded { a, b, mid, dir },
                };
                let edges = f
                    .edges
                    .into_iter()
                    .map(|e| match e {
                        FormEdge::Line { a, b } => EdgeSpec::Line { a, b },
                        FormEdge::Arc { a, b, mid } => EdgeSpec::Arc { a, b, mid },
                        FormEdge::Chain { ctrl } => EdgeSpec::Chain { degree: 3, ctrl: ctrl.into() },
                    })
                    .collect();
                (FaceSpec { loops: vec![edges], surface, probe: Some((f.point, f.out)) }, f.face)
            })
            .collect();
        out.push(Form { bead, panel: b.panel, open, centre: form.centre, faces });
    }
    Ok(out)
}

impl SurfaceSheet {
    /// Bead `b` in the flat as a flat pattern is read (see [`SurfaceSheet::shown`]): its
    /// footprint, its centreline, its crown, and how a note along it is written.
    pub fn bead_outline(&self, b: &SurfaceBead) -> Result<BeadOutline> {
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let fold = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places)?;
        fold.bead_outline(&b.record(), self.sheet.thickness, true).map_err(|why| b.refused(why))
    }

    /// Where a note `height` high along bead `b` stands in the flat as a flat pattern is read (see
    /// [`SurfaceSheet::shown`]): its anchor and its angle (radians).
    pub fn bead_note(&self, b: &SurfaceBead, height: f64) -> Result<(Vec2, f64)> {
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let fold = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places)?;
        fold.bead_note(&b.record(), self.sheet.thickness, true, height).map_err(|why| b.refused(why))
    }

    /// Where a straight sketch line from `a` to `b`, drawn on the plane through `origin` with unit
    /// normal `normal` (all in the sheet's frame), lies on the sheet: the planar panel whose face
    /// the plane is and that holds the whole line (its ends and its middle), the line's ends in
    /// that panel's chart, and whether the plane is (nearer) its far face.
    pub fn bead_line(&self, origin: Vec3, normal: Vec3, a: Vec3, b: Vec3) -> Result<(usize, Vec2, Vec2, bool)> {
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let fold = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places)?;
        let t = self.sheet.thickness;
        for (i, panel) in self.sheet.panels.iter().enumerate() {
            if !matches!(panel.surface, Surface::Plane) {
                continue;
            }
            let Some((qa, qb, far)) = fold.line_in_chart(i, t, origin, normal, a, b) else { continue };
            if solidcraft_develop::segment_in_region(&panel.region, qa, qb) == Some(true) {
                return Ok((i, qa, qb, far));
            }
        }
        Err(DocError::Invalid("a bead's centreline must lie on a face of a planar panel of the sheet".into()))
    }

    /// Checks a new bead `b` as [`SurfaceSheet::check_bead`] does, laying the sheet out first;
    /// `notes` gets what lies within `margin` of it, and how far it goes past the usual forming
    /// limits.
    pub(crate) fn check_new_bead(&self, b: &SurfaceBead, margin: f64, notes: &mut Vec<String>) -> Result<()> {
        let dev = self.develop().map_err(|e| DocError::Invalid(e.to_string()))?;
        let pieces = crate::sheet_surface::pieces(&dev, &mut crate::sheet::FlatWork::new())?;
        let fold = crate::sheet_surface_convert::fold_onto(&self.sheet, &dev, &self.places)?;
        self.check_bead(b, &pieces, &fold, Some(margin), notes)
    }

    /// Whether bead `b` can be pressed: on a planar panel, its footprint inside what the
    /// development leaves of that panel, clear of the sheet's edges, its cuts, its bends'
    /// zones, its recesses and the other beads. With a `margin`, `notes` gets what lies closer
    /// than that, and how far the bead goes past the usual forming limits.
    pub(crate) fn check_bead(
        &self,
        b: &SurfaceBead,
        pieces: &[(Piece, Region2)],
        fold: &Fold,
        margin: Option<f64>,
        notes: &mut Vec<String>,
    ) -> Result<()> {
        let t = self.sheet.thickness;
        let panel = self.sheet.panels.get(b.panel).ok_or_else(|| DocError::Invalid("a bead's panel is not on the sheet".into()))?;
        if !matches!(panel.surface, Surface::Plane) {
            return Err(DocError::Invalid("not supported yet: a bead on a rolled or free-form panel".into()));
        }
        let bead = b.record();
        let against = |l: &Loop2, near: f64| solidcraft_develop::bead_against(&bead, t, l, near).map_err(|why| b.refused(why));
        // What lies in the flat, moved into the panel's chart: a planar panel's chart and the
        // flat differ by a rigid move, which moves every curve exactly.
        let back = fold.placement(b.panel).map(|m| m.inverse()).ok_or_else(|| DocError::Invalid("a bead's panel has no place in the flat".into()))?;
        // The panel's own pieces (the footprint lies inside one of them), and what it may neither
        // meet nor lie inside nor lie round: the bends' zones, the cut-outs, and the other beads.
        let mut panels: Vec<Region2> = Vec::new();
        let mut obstacles: Vec<(&str, Loop2)> = Vec::new();
        for (piece, region) in pieces {
            match piece {
                Piece::Zone(_) | Piece::Tab(..) => obstacles.push(("a bend", back.loop2(&region.outer))),
                Piece::Panel(i) if *i == b.panel => {
                    let r = back.region(region);
                    obstacles.extend(r.holes.iter().map(|h| ("a cut-out", h.clone())));
                    panels.push(r);
                }
                Piece::Panel(_) => {}
            }
        }
        obstacles.extend(self.sheet.cuts.iter().filter(|c| c.panel == b.panel).map(|c| ("a cut-out", c.region.outer.clone())));
        for o in self.beads.iter().filter(|o| *o != b && o.panel == b.panel) {
            if let Some(l) = o.footprint_loop(t) {
                obstacles.push(("another bead", l));
            }
        }
        // The first outline the footprint comes within `near` of, lies inside or lies round. A
        // recess is its region: inside it is inside one of its outlines and on none of the islands
        // it leaves.
        let hits = |near: f64| -> Result<Option<&str>> {
            for r in &panels {
                if against(&r.outer, near)? == BeadAgainst::Meets {
                    return Ok(Some("the sheet's edge"));
                }
            }
            for (what, l) in &obstacles {
                if against(l, near)? != BeadAgainst::Clear {
                    return Ok(Some(*what));
                }
            }
            for r in self.recesses.iter().filter(|r| r.panel == b.panel) {
                let mut on_island = false;
                for h in &r.region.holes {
                    match against(h, near)? {
                        BeadAgainst::Inside => on_island = true,
                        BeadAgainst::Clear => {}
                        _ => return Ok(Some("a recess")),
                    }
                }
                match against(&r.region.outer, near)? {
                    BeadAgainst::Clear => {}
                    BeadAgainst::Inside if on_island => {}
                    _ => return Ok(Some("a recess")),
                }
            }
            for c in self.countersinks.iter().filter(|c| c.panel == b.panel) {
                if against(&c.outer_loop(), near)? != BeadAgainst::Clear {
                    return Ok(Some("a countersink"));
                }
            }
            Ok(None)
        };
        if let Some(what) = hits(0.0)? {
            return Err(DocError::Invalid(format!("a bead's footprint meets {what}")));
        }
        // Clear of every outline, it lies wholly inside one piece of its panel or off its material.
        let mut on_panel = false;
        for r in &panels {
            on_panel |= against(&r.outer, 0.0)? == BeadAgainst::Inside;
        }
        if !on_panel {
            return Err(DocError::Invalid("a bead must lie on the material of one panel".into()));
        }
        if let Some(m) = margin {
            if let Some(what) = hits(m)? {
                notes.push(format!("a bead lies within {m} mm of {what}"));
            }
            if b.depth > b.width / 3.0 + 1e-9 {
                notes.push(format!("a bead {} deep is deeper than a third of its width ({:.4})", b.depth, b.width / 3.0));
            }
            if b.depth > 3.0 * t + 1e-9 {
                notes.push(format!("a bead {} deep is deeper than 3 × the sheet's thickness ({:.4})", b.depth, 3.0 * t));
            }
        }
        Ok(())
    }
}
