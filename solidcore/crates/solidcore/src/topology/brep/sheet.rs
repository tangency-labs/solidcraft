//! A one-face sheet on a surface's whole natural box ([`sheet_on`]), the kernel's one
//! arena-notify `CreateFaceFromSurface` wrapper, a face on a box of a surface's parameters
//! ([`face_on`]), a sheet turned over ([`turn_over_shell`], [`turn_over_sheet`]), and a sheet
//! of several faces taken as the pieces of one surface and made again from it ([`one_surface`],
//! [`sheet_of`]).
//!
//! Not upstream: the composition an operation that rebuilds a face on a new surface (a sheet
//! extended past a side) makes of `new_brep` and `SmBrep::CreateFaceFromSurface`, kept in the
//! kernel so the modelling layer holds no notification plumbing; the turn-over of every
//! face of an open shell ([`crate::topology::face::p1::turn_over`]); and the pieces a split
//! leaves joined back into their surface, so an operation on the surface (an extension past a
//! side) can make the pieces again, cut where they were.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use std::collections::BTreeSet;

use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::tol::USE_DEFAULT;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::math::CPoint;
use crate::nurbs::srf_basic::{Direction, Surface as NurbSurface};
use crate::nurbs::srf_tool::p3::join;
use crate::surface::side::BoxSide;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::topology::brep::p1::faces;
use crate::topology::brep::p2::ArenaNotifyState;
use crate::topology::brep::p3::new_brep;
use crate::topology::brep::p4::stitch_faces;
use crate::topology::brep::p8::{create_face_from_surface, FaceBuildEnv};
use crate::topology::face::p1::turn_over;
use crate::topology::face::p2::{loops, surface as face_surface, uv_domain};
use crate::topology::topology_traverser::Marks;
use crate::topology::{Brep, FaceId, ShellId};
use crate::EntityRef;

/// A new body of one face on `surface` over its whole natural box, at `tolerance`, and that
/// face.
///
/// # Errors
///
/// Those of the box query, `new_brep` and `CreateFaceFromSurface`.
pub fn sheet_on(k: &mut Kernel, surface: Surface, tolerance: f64) -> Result<(Brep, FaceId)> {
    let bx = surface.natural_uv_domain_with_nurb(k)?;
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    b.set_tolerance(tolerance);
    let f = face_on(k, &mut b, surface, &bx)?;
    b.refresh_cached_topology();
    Ok((b, f))
}

/// `SmBrep::CreateFaceFromSurface(surface, bx, face)`: a new face of `b` on `surface` over the
/// box `bx` of its parameters, bounded by one loop of the box's sides, notifying through an
/// arena-only store (default records, no attributes, no context), the new face owning its
/// surface under its arena identity. The kernel's one such wrapper; the caller refreshes `b`'s
/// cached topology when it reads it next.
///
/// # Errors
///
/// Those of `CreateFaceFromSurface`.
pub fn face_on(k: &mut Kernel, b: &mut Brep, surface: Surface, bx: &Extent2d) -> Result<FaceId> {
    let mut state = ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let key_of = env.key_of;
    let this_face = |f: FaceId| key_of(EntityRef::Face(f));
    let mut marks = Marks::default();
    let mut fenv = FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    create_face_from_surface(k, b, Some(surface), bx, &mut fenv)
}

const OP_TURN: &str = "topology::brep::sheet::turn_over_shell";

/// Turns the open shell `s` over: each of its faces ([`turn_over`], `SmFace::SwapUV`), so each
/// face's side is the other one, its edges as they were; the body stays valid. Turning a shell
/// over twice gives it back.
///
/// A loop of [`turn_over`], not `SmBrep::SwapUV` or `OrientSheetSurfaces(same = false)`: those
/// ignore a face's failed swap (upstream's `sm_SwapFaceUV` is void), and a turn-over that stops
/// part way must say so and leave the shell as it was to the caller's rollback.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a closed shell (one whose faces' other uses lie in another
/// shell: turned over, the body would be inside out), before anything changes;
/// [`SolidError::InvalidInput`] for a shell of no faces; those of [`turn_over`].
pub fn turn_over_shell(k: &Kernel, b: &mut Brep, s: ShellId) -> Result<()> {
    let faces = open_shell_faces(b, s)?;
    for f in faces {
        turn_over(k, b, f)?;
    }
    b.refresh_cached_topology();
    Ok(())
}

/// Turns every shell of faces of `b` over ([`turn_over_shell`]): a sheet turned over.
///
/// # Errors
///
/// [`SolidError::Unsupported`] for a body with a closed shell (a solid), before anything
/// changes; those of [`turn_over`].
pub fn turn_over_sheet(k: &Kernel, b: &mut Brep) -> Result<()> {
    let mut faces = Vec::new();
    let shells: Vec<ShellId> = b.shells().ids().collect();
    for s in shells {
        if b.shell(s)?.is_faceuse_shell() {
            faces.extend(open_shell_faces(b, s)?);
        }
    }
    for f in faces {
        turn_over(k, b, f)?;
    }
    b.refresh_cached_topology();
    Ok(())
}

/// The faces of the open shell `s`, each once, in the order of their uses.
fn open_shell_faces(b: &Brep, s: ShellId) -> Result<Vec<FaceId>> {
    let shell = b.shell(s)?;
    if !shell.is_faceuse_shell() {
        return Err(SolidError::invalid_input(OP_TURN, "a shell of no faces"));
    }
    if shell.is_closed(b, s)? {
        return Err(SolidError::Unsupported {
            operation: OP_TURN,
            reason: "a closed shell: turned over, the body would be inside out",
        });
    }
    let mut seen = BTreeSet::new();
    let mut faces = Vec::new();
    for fu in shell.get_faceuses()? {
        if let Some(f) = b.faceuse(fu)?.face {
            if seen.insert(f) {
                faces.push(f);
            }
        }
    }
    Ok(faces)
}

const OP_PIECES: &str = "topology::brep::sheet::one_surface";

/// A sheet's faces as the pieces of one surface ([`one_surface`]): the surface, its whole box,
/// each face with its box on the surface, and the parameters the pieces are cut at.
#[derive(Debug, Clone)]
pub struct Pieces {
    /// The surface the faces are the pieces of: a B-spline surface on their parameters.
    pub surface: Surface,
    /// The surface's whole box: the faces' boxes together.
    pub whole: Extent2d,
    /// Each face and its box, by u interval, then by v interval within it: the order
    /// [`sheet_of`] makes its faces in.
    pub faces: Vec<(FaceId, Extent2d)>,
    /// The u parameters between the pieces, increasing.
    pub u_cuts: Vec<f64>,
    /// The v parameters between the pieces, increasing.
    pub v_cuts: Vec<f64>,
}

impl Pieces {
    /// Whether side `side` of a face's box `bx` is on the same side of the whole box.
    #[must_use]
    pub fn on_side(&self, bx: &Extent2d, side: BoxSide) -> bool {
        side.at(bx) == side.at(&self.whole)
    }
}

/// The one surface `b`'s faces are the pieces of, as a split (`SmBrep::CreateFacesFromSurface`,
/// [`sheet_of`]) leaves them, or `None` where they are not: each face a B-spline surface's whole
/// box bounded by one loop, the boxes tiling one box on one grid of parameters, and each piece
/// continuing its neighbour exactly (the same degrees, rationality and knots across the cut, the
/// row they share the same bit for bit, their parameters meeting). The pieces are joined back
/// ([`join`], in v within each u interval, then in u), the shared row once; a knot at a cut is
/// kept where removing it would move the net at all.
///
/// # Errors
///
/// Those of the face queries and the join.
pub fn one_surface(k: &Kernel, b: &Brep) -> Result<Option<Pieces>> {
    let mut cells: Vec<(FaceId, Extent2d, NurbSurface)> = Vec::new();
    for f in faces(b, None)? {
        let Some(s) = face_surface(b, f)? else {
            return Ok(None);
        };
        let SurfaceKind::BSpline(spline) = s.kind() else {
            return Ok(None);
        };
        let Some(net) = spline.gw_nurb() else {
            return Ok(None);
        };
        let bx = uv_domain(b, f)?;
        if !s.is_whole_box(k, &bx)? || loops(b, f, false, None)?.len() != 1 {
            return Ok(None);
        }
        cells.push((f, bx, net.clone()));
    }
    // the grid: every face's box one cell of it, each cell one face's
    let breaks = |of: &dyn Fn(&Extent2d) -> [f64; 2]| -> Vec<f64> {
        let mut x: Vec<f64> = cells.iter().flat_map(|(_, bx, _)| of(bx)).collect();
        x.sort_by(f64::total_cmp);
        x.dedup();
        x
    };
    let us = breaks(&|bx: &Extent2d| [bx.min().x, bx.max().x]);
    let vs = breaks(&|bx: &Extent2d| [bx.min().y, bx.max().y]);
    let (columns, rows) = (us.len().saturating_sub(1), vs.len().saturating_sub(1));
    if columns.checked_mul(rows) != Some(cells.len()) || cells.is_empty() {
        return Ok(None);
    }
    let interval = |x: &[f64], a: f64, b: f64| {
        x.windows(2)
            .position(|w| matches!(w, [p, q] if *p == a && *q == b))
    };
    let mut grid: Vec<Option<(FaceId, Extent2d, NurbSurface)>> = vec![None; cells.len()];
    for cell in cells {
        let bx = &cell.1;
        let (Some(i), Some(j)) = (
            interval(&us, bx.min().x, bx.max().x),
            interval(&vs, bx.min().y, bx.max().y),
        ) else {
            return Ok(None);
        };
        let Some(slot) = i
            .checked_mul(rows)
            .and_then(|x| x.checked_add(j))
            .and_then(|x| grid.get_mut(x))
        else {
            return Ok(None);
        };
        if slot.is_some() {
            return Ok(None);
        }
        *slot = Some(cell);
    }
    let grid: Vec<(FaceId, Extent2d, NurbSurface)> = grid.into_iter().flatten().collect();
    // each u interval's pieces joined in v, then the columns in u
    let mut joined_columns: Vec<NurbSurface> = Vec::new();
    for i in 0..columns {
        let column = i
            .checked_mul(rows)
            .and_then(|a| Some(a..a.checked_add(rows)?))
            .and_then(|r| grid.get(r))
            .ok_or_else(|| SolidError::invalid_input(OP_PIECES, "a column past the grid"))?;
        let Some(c) = joined(column.iter().map(|(_, _, s)| s), Direction::V)? else {
            return Ok(None);
        };
        joined_columns.push(c);
    }
    let Some(whole_net) = joined(joined_columns.iter(), Direction::U)? else {
        return Ok(None);
    };
    let surface = Surface::from(BSplineSurface::from_gw_nurb(&whole_net));
    let whole = surface.natural_uv_domain_with_nurb(k)?;
    let inner = |x: &[f64]| -> Vec<f64> {
        x.iter()
            .skip(1)
            .take(x.len().saturating_sub(2))
            .copied()
            .collect()
    };
    Ok(Some(Pieces {
        surface,
        whole,
        faces: grid.into_iter().map(|(f, bx, _)| (f, bx)).collect(),
        u_cuts: inner(&us),
        v_cuts: inner(&vs),
    }))
}

/// The pieces of `pieces` joined in `dir`, each continuing the one before exactly, or `None`
/// where one does not.
fn joined<'a>(
    mut pieces: impl Iterator<Item = &'a NurbSurface>,
    dir: Direction,
) -> Result<Option<NurbSurface>> {
    let Some(first) = pieces.next() else {
        return Err(SolidError::invalid_input(OP_PIECES, "no pieces to join"));
    };
    let mut out = first.clone();
    for next in pieces {
        if !continues(&out, next, dir)? {
            return Ok(None);
        }
        // a knot removed only where the net does not move at all
        out = join(&out, next, dir, 0.0)?;
    }
    Ok(Some(out))
}

/// Whether `b` continues `a` past its end in `dir` exactly: the same degrees and rationality,
/// the same knots and net size the other way, `b`'s parameters starting where `a`'s end, and
/// `a`'s last row `b`'s first, bit for bit.
fn continues(a: &NurbSurface, b: &NurbSurface, dir: Direction) -> Result<bool> {
    let same = |x: &[f64], y: &[f64]| {
        x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits())
    };
    let bits = |c: &CPoint| [c.x.to_bits(), c.y.to_bits(), c.z.to_bits(), c.w.to_bits()];
    if a.degrees() != b.degrees() || a.is_rational() != b.is_rational() {
        return Ok(false);
    }
    let (across, along) = if dir == Direction::U {
        ((a.v_knots(), b.v_knots()), (a.u_knots(), b.u_knots()))
    } else {
        ((a.u_knots(), b.u_knots()), (a.v_knots(), b.v_knots()))
    };
    let meet = matches!(
        (along.0.last(), along.1.first()),
        (Some(x), Some(y)) if x.to_bits() == y.to_bits()
    );
    if !same(across.0, across.1) || !meet {
        return Ok(false);
    }
    let (count, a_end) = if dir == Direction::U {
        (a.m(), a.n())
    } else {
        (a.n(), a.m())
    };
    for i in 0..=count {
        let (p, q) = if dir == Direction::U {
            (a.cpt(a_end, i)?, b.cpt(0, i)?)
        } else {
            (a.cpt(i, a_end)?, b.cpt(i, 0)?)
        };
        if bits(&p) != bits(&q) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A new sheet of `surface` cut at the u parameters `u_cuts`, then each piece at the v
/// parameters `v_cuts` (each increasing, inside the surface's box), one face a piece on its
/// whole box, stitched at `tolerance`; and each face with its box, by u interval, then by v
/// interval within it (the order [`one_surface`] gives its faces in). The pieces are cut where
/// asked and nowhere else: a knot of low continuity elsewhere (an extension's joint) stays
/// inside its face, as on a sheet of one face ([`sheet_on`]).
///
/// # Errors
///
/// `InvalidInput` for cuts not increasing or not inside the box; those of the split, the face
/// construction and the stitch.
pub fn sheet_of(
    k: &mut Kernel,
    surface: Surface,
    u_cuts: &[f64],
    v_cuts: &[f64],
    tolerance: f64,
) -> Result<(Brep, Vec<(FaceId, Extent2d)>)> {
    let bx = surface.natural_uv_domain_with_nurb(k)?;
    let inside = |cuts: &[f64], lo: f64, hi: f64| {
        cuts.iter().all(|&c| lo < c && c < hi)
            && cuts.windows(2).all(|w| matches!(w, [a, b] if a < b))
    };
    if !inside(u_cuts, bx.min().x, bx.max().x) || !inside(v_cuts, bx.min().y, bx.max().y) {
        return Err(SolidError::invalid_input(
            OP_SHEET_OF,
            "cuts not increasing inside the surface's box",
        ));
    }
    let mut columns = Vec::new();
    let mut rest = surface;
    for &u in u_cuts {
        let (left, right) = rest.split_at(k, u, SurfParamType::U)?;
        columns.push(left);
        rest = right;
    }
    columns.push(rest);
    let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
    b.set_tolerance(tolerance);
    let mut made = Vec::new();
    let mut place = |k: &mut Kernel, b: &mut Brep, s: Surface| -> Result<()> {
        let bx = s.natural_uv_domain_with_nurb(k)?;
        made.push((face_on(k, b, s, &bx)?, bx));
        Ok(())
    };
    for column in columns {
        let mut rest = column;
        for &v in v_cuts {
            let (low, high) = rest.split_at(k, v, SurfParamType::V)?;
            place(k, &mut b, low)?;
            rest = high;
        }
        place(k, &mut b, rest)?;
    }
    b.refresh_cached_topology();
    if made.len() > 1 {
        stitch_faces(k, &mut b, tolerance, None, false)?;
        b.refresh_cached_topology();
    }
    Ok((b, made))
}

const OP_SHEET_OF: &str = "topology::brep::sheet::sheet_of";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::math::NOW;

    /// A row of the crease's net at `x`, `z`: y from 0 to 4.
    fn row(x: f64, z: f64) -> Vec<CPoint> {
        vec![CPoint::new(x, 0.0, z, NOW), CPoint::new(x, 4.0, z, NOW)]
    }

    /// A surface of degree 1 both ways on the rows at `(x, z)`, over u knots `u`.
    fn spline(rows: &[(f64, f64)], u: &[f64]) -> NurbSurface {
        NurbSurface::new(
            rows.iter().map(|&(x, z)| row(x, z)).collect(),
            1,
            1,
            u.to_vec(),
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap()
    }

    /// The crease: flat from x = 0 to 5, then up to z = 5; its knot u = 0.5 the crease.
    fn crease() -> NurbSurface {
        spline(
            &[(0.0, 0.0), (5.0, 0.0), (5.0, 5.0)],
            &[0.0, 0.0, 0.5, 1.0, 1.0],
        )
    }

    fn of(n: &NurbSurface) -> Surface {
        Surface::from(BSplineSurface::from_gw_nurb(n))
    }

    /// The net and knots, bit for bit.
    fn bits(s: &NurbSurface) -> (Vec<u64>, Vec<u64>, Vec<[u64; 4]>) {
        let knots = |x: &[f64]| x.iter().map(|t| t.to_bits()).collect();
        let net = s
            .rows()
            .flatten()
            .map(|c| [c.x.to_bits(), c.y.to_bits(), c.z.to_bits(), c.w.to_bits()])
            .collect();
        (knots(s.u_knots()), knots(s.v_knots()), net)
    }

    #[test]
    fn a_sheet_cut_from_a_surface_gives_the_surface_back() {
        let mut k = Kernel::default();
        let s = crease();
        let (b, made) = sheet_of(&mut k, of(&s), &[0.5], &[], 1e-6).unwrap();
        assert_eq!(made.len(), 2);
        let pieces = one_surface(&k, &b).unwrap().unwrap();
        assert_eq!(pieces.u_cuts, [0.5]);
        assert!(pieces.v_cuts.is_empty());
        // the faces in the order the sheet made them
        let ids = |x: &[(FaceId, Extent2d)]| x.iter().map(|(f, _)| *f).collect::<Vec<_>>();
        assert_eq!(ids(&pieces.faces), ids(&made));
        let SurfaceKind::BSpline(back) = pieces.surface.kind() else {
            panic!("not a B-spline: {:?}", pieces.surface.kind());
        };
        assert_eq!(bits(back.gw_nurb().unwrap()), bits(&s));
        // the first piece is on the whole box's u min side and both v sides, not its u max
        let (_, first) = made[0];
        assert!(pieces.on_side(&first, BoxSide::UMin));
        assert!(!pieces.on_side(&first, BoxSide::UMax));
        assert!(pieces.on_side(&first, BoxSide::VMin) && pieces.on_side(&first, BoxSide::VMax));
    }

    #[test]
    fn faces_that_do_not_continue_each_other_are_not_one_surface() {
        let mut k = Kernel::default();
        let left = spline(&[(0.0, 0.0), (5.0, 0.0)], &[0.0, 0.0, 0.5, 0.5]);
        let sheet = |k: &mut Kernel, right: &NurbSurface| {
            let mut b = new_brep(k, USE_DEFAULT, USE_DEFAULT).unwrap();
            for n in [&left, right] {
                let s = of(n);
                let bx = s.natural_uv_domain_with_nurb(k).unwrap();
                face_on(k, &mut b, s, &bx).unwrap();
            }
            b.refresh_cached_topology();
            b
        };
        // the right piece where the left one ends: one surface
        let meets = spline(&[(5.0, 0.0), (5.0, 5.0)], &[0.5, 0.5, 1.0, 1.0]);
        let b = sheet(&mut k, &meets);
        assert!(one_surface(&k, &b).unwrap().is_some());
        // lifted off it by 1: not
        let lifted = spline(&[(5.0, 1.0), (5.0, 6.0)], &[0.5, 0.5, 1.0, 1.0]);
        let b = sheet(&mut k, &lifted);
        assert!(one_surface(&k, &b).unwrap().is_none());
    }
}
