//! Split Face and Split Body: faces cut where tool surfaces meet them, and a solid cut in two by
//! a surface or by a sheet.
//!
//! A face is cut the way the kernel's plane cutter cuts a subset of faces: the face's surface is
//! intersected with the tool over the face's UV domain (the planar section for a plane that
//! reaches over the face, the kernel's one surface/surface intersection otherwise) and the
//! curves go to the kernel's merge of curves on a surface, with the tool as the intersecting
//! surface. A failed merge fails the call. A body is split by slicing copies of it with a
//! one-face sheet on the tool surface, once with the surface turned over; or with a sheet of
//! several faces as it is, its surfaces first turned to agree.
//!
//! A tool is a record surface over a box of its record parameters ([`Tool`]), made into the
//! kernel's by [`from_parameters`]; faces are record indices ([`Order`]). Ops chooses the
//! intersection, sequences the kernel's calls and compares the numbers they return; it computes
//! nothing. Every operation works on copies: its operands are unchanged.

use std::collections::HashSet;

use solidcore::boolean::api_brep::boolean_with_options;
use solidcore::boolean::merge::MergeOptions;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol::approx_tol3d;
use solidcore::kernel::Kernel;
use solidcore::query::brep::BrepFaceServices;
use solidcore::query::face::p1::calculate_bounding_box;
use solidcore::surface::from_parameters::from_parameters;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::TsectCurveType;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::brep::p10::merge_curves_on_surface_through_surface;
use solidcore::topology::brep::p2::{is_manifold_solid, is_sheet};
use solidcore::topology::brep::p3::{brep_copy, brep_copy_mapped, with_kernel_callbacks};
use solidcore::topology::brep::p9::orient_sheet_surfaces;
use solidcore::topology::brep::planar_section::face_plane_section_curves;
use solidcore::topology::brep::sheet::sheet_on;
use solidcore::topology::brep_cache::CacheObject;
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::face::p2 as face_p2;
use solidcore::topology::proto_topology::p1::CopyBrepMap;
use solidcore::topology::{Brep, EdgeId, FaceId, NamingScheme};
use solidcore::EntityRef;
use solidcore_convert::brep::Order;
use solidcore_record::brep::Surface as RecordSurface;

use crate::body::Body;
use crate::error::status;
use crate::session::run_on;
use crate::{Error, Result};

/// The most faces one call splits.
pub const MAX_FACES: usize = 100_000;
/// The most tools one call takes.
pub const MAX_TOOLS: usize = 4096;

/// A tool surface: a record surface over `domain`, `[[u0, u1], [v0, v1]]` in the record's
/// parameters (radians for angles; a spline's own knots), as [`from_parameters`] takes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    /// The surface.
    pub surface: RecordSurface,
    /// The box of its parameters the tool covers.
    pub domain: [[f64; 2]; 2],
}

/// A tool as the kernel holds it: its surface over its natural UV domain, and a plane's own
/// kernel plane for its section.
struct Made {
    surface: Surface,
    domain: Extent2d,
    plane: Option<Plane>,
}

/// The kernel's surface of `tool`.
fn made(k: &Kernel, tool: &Tool) -> solidcore::Result<Made> {
    let (surface, domain) =
        from_parameters(k, &solidcore_convert::surface(&tool.surface), tool.domain)?;
    let plane = match surface.kind() {
        SurfaceKind::Plane(p) => Some(p.clone()),
        _ => None,
    };
    Ok(Made {
        surface,
        domain,
        plane,
    })
}

/// Each tool made, or the first refusal.
fn made_all(k: &Kernel, tools: &[Tool]) -> solidcore::Result<Result<Vec<Made>>> {
    let mut out = Vec::new();
    for t in tools {
        match made(k, t) {
            Ok(m) => out.push(m),
            Err(e) if e.is_unported() || e.is_cancelled() => return Err(e),
            Err(e) => return Ok(Err(status(e))),
        }
    }
    Ok(Ok(out))
}

/// The copies of `ids` (a body's own entities) in a copy of the body, through the copy's map;
/// `None` when one has no copy.
pub(crate) fn copies_of<T: Copy>(
    map: &CopyBrepMap<EntityRef>,
    ids: &[T],
    wrap: fn(T) -> EntityRef,
    unwrap: fn(EntityRef) -> Option<T>,
) -> Option<Vec<T>> {
    ids.iter()
        .map(|&x| map.get_at(Some(wrap(x))).ok().flatten().and_then(unwrap))
        .collect()
}

/// The name of each edge curve a merge makes.
fn curve_ref(e: EdgeId, _type_code: i64) -> ObjectRef {
    solidcore::topology::loopuse::notify_ref(EntityRef::Edge(e))
}

/// Cuts `face` where `tool` meets it: the faces it was split into besides itself.
fn cut_face(
    k: &mut Kernel,
    b: &mut Brep,
    env: &mut NotifyEnv<'_>,
    face: FaceId,
    tool: &Made,
    tol: f64,
) -> solidcore::Result<Result<Vec<FaceId>>> {
    let Some(surface) = face_p2::surface(b, face)? else {
        return Ok(Err(Error::Status(solidcore::error::SM_ERR)));
    };
    let surface = surface.clone_for_evaluation()?;
    let domain = face_p2::uv_domain(b, face)?;
    // the intersection's accuracy: a quarter of the body's, as the kernel's plane cutter asks
    let approx = approx_tol3d(k, approx_tol3d(k, tol) / 4.0);
    let bx = match calculate_bounding_box(k, &mut *b, face, false) {
        Ok(bx) => bx,
        Err(e) => return Ok(Err(status(e))),
    };
    let merge = |k: &mut Kernel,
                 b: &mut Brep,
                 env: &mut NotifyEnv<'_>,
                 mut c3: Vec<_>,
                 uv: Option<Vec<_>>|
     -> solidcore::Result<Result<Vec<FaceId>>> {
        match merge_curves_on_surface_through_surface(
            k,
            b,
            env,
            &curve_ref,
            face,
            tol,
            &mut c3,
            uv.as_deref(),
            Some(tool.surface.clone()),
        ) {
            Ok((made, _)) => Ok(Ok(made)),
            Err(e) => Ok(Err(status(e))),
        }
    };
    // A plane reaching over the face sections it exactly (the plane cutter's way); a plane
    // strip, like any other tool, is intersected over its own domain.
    if let Some(plane) = &tool.plane {
        let margin = plane.box_margin(&bx);
        let covers = if margin.lower >= -tol {
            true
        } else if margin.upper < -tol {
            false
        } else {
            return Ok(Err(Error::Unsupported(
                "whether the plane reaches over the face is undecided",
            )));
        };
        if covers {
            // as the plane cutter: a face farther from the plane than the tolerance is not cut
            let section = match face_plane_section_curves(
                k,
                b,
                face,
                &surface,
                &domain,
                plane,
                &bx,
                tol,
                Some(approx),
                None,
            ) {
                Ok(Some(s)) => s,
                Ok(None) => return Ok(Ok(Vec::new())),
                Err(e) => return Ok(Err(status(e))),
            };
            if section.curves_3d.is_empty() {
                return Ok(Ok(Vec::new()));
            }
            let uv =
                (section.uv_curves.len() == section.curves_3d.len()).then_some(section.uv_curves);
            return merge(k, b, env, section.curves_3d, uv);
        }
    }
    // the face's surface reaches its face's trimmed cache: the solve runs through the body's
    // face services, as the kernel's own face intersections do
    let mut services = BrepFaceServices::new(&mut *b);
    let found = match surface.global_surface_intersect_with_context(
        k,
        [Some(CacheObject::Surface(face)), None],
        &mut services,
        &domain,
        &tool.surface,
        &tool.domain,
        [true, true],
        Some(approx),
        None,
    ) {
        Ok(f) => f,
        Err(e) => return Ok(Err(status(e))),
    };
    // a single point of contact cuts nothing
    let (mut c3, mut uv1) = (Vec::new(), Vec::new());
    for (i, c) in found.curves_3d.into_iter().enumerate() {
        if found.curve_types.get(i) == Some(&TsectCurveType::Touching) {
            continue;
        }
        c3.push(c);
        uv1.push(found.surface1_uv_curves.get(i).cloned().flatten());
    }
    if c3.is_empty() {
        return Ok(Ok(Vec::new()));
    }
    let all_uv = uv1.iter().all(Option::is_some);
    merge(k, b, env, c3, all_uv.then_some(uv1))
}

/// A copy of `body` with record faces `faces` (indices into `order`, distinct) cut where each
/// tool meets them, the tools in turn, each cutting the pieces the earlier ones left; and for
/// each face how many faces it became besides itself. `None` when no face is split. `body` is
/// unchanged.
///
/// # Errors
/// - [`Error::Argument`]: no face or tool, too many ([`MAX_FACES`], [`MAX_TOOLS`]), a face
///   twice or past the record's list;
/// - [`Error::Unsupported`]: a plane tool whose reach over a face is undecided;
/// - the kernel's errors (a tool it refuses is its status).
pub fn split_faces(
    body: &Body,
    order: &Order,
    faces: &[usize],
    tools: &[Tool],
) -> Result<Option<(Body, Vec<usize>)>> {
    if faces.is_empty() || faces.len() > MAX_FACES || tools.is_empty() || tools.len() > MAX_TOOLS {
        return Err(Error::Argument);
    }
    let mut seen = HashSet::new();
    let mut picked = Vec::new();
    for &f in faces {
        if !seen.insert(f) {
            return Err(Error::Argument);
        }
        picked.push(*order.faces.get(f).ok_or(Error::Argument)?);
    }
    let r = run_on("split_faces", &[("brep", body)], |k| {
        // the record's faces are the body's own: looked up in it, then mapped to their
        // copies (the copy numbers its entities as it builds them)
        let (mut a, picked) = {
            let g = body.lock();
            let all = brep_p1::faces(&g, None)?;
            if picked.iter().any(|f| !all.contains(f)) {
                return Ok(Err(Error::Argument));
            }
            let (a, map) = brep_copy_mapped(k, &g)?;
            let Some(picked) = copies_of(&map, &picked, EntityRef::Face, |e| match e {
                EntityRef::Face(f) => Some(f),
                _ => None,
            }) else {
                return Ok(Err(Error::Argument));
            };
            (a, picked)
        };
        // each chosen face and, as tools cut it, its pieces
        let mut pieces: Vec<Vec<FaceId>> = picked.iter().map(|&f| vec![f]).collect();
        let mut made = vec![0usize; picked.len()];
        let built = match made_all(k, tools)? {
            Ok(b) => b,
            Err(e) => return Ok(Err(e)),
        };
        let editing = a.editing_enabled();
        let doing = k.context.doing_boolean();
        a.set_editing_enabled(true);
        k.context.set_doing_boolean(true);
        let tol = a.tolerance();
        // the edits notify through the kernel's callbacks with the body's own attributes, so
        // what an application attached follows each cut
        let cut = with_kernel_callbacks(k, &mut a, |k, a, env| {
            let mut total = 0usize;
            for t in &built {
                for (list, m) in pieces.iter_mut().zip(made.iter_mut()) {
                    let now = list.clone();
                    for f in now {
                        let split = match cut_face(k, a, env, f, t, tol)? {
                            Ok(s) => s,
                            Err(e) => return Ok(Err(e)),
                        };
                        *m += split.len();
                        total += split.len();
                        list.extend(split);
                    }
                }
            }
            Ok(Ok(total))
        });
        a.set_editing_enabled(editing);
        k.context.set_doing_boolean(doing);
        a.refresh_cached_topology();
        // the cut's new edges own their curves under the kernel's identities (it records
        // them), and the check reads the owners by the body's naming
        a.set_naming(NamingScheme::Caller);
        Ok(match cut? {
            Err(e) => Err(e),
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some((a, made))),
        })
    })??;
    Ok(r.map(|(b, made)| (Body::new(b), made)))
}

/// A copy of `body` sliced by `sheet` (the slice reads the side from the surfaces, not the
/// faces' uses). `None` when nothing is left.
fn slice(k: &mut Kernel, body: &Body, sheet: Brep) -> solidcore::Result<Result<Option<Brep>>> {
    let a = brep_copy(k, &body.lock())?;
    let options = MergeOptions::new(false, false, false, false, false);
    let r = match boolean_with_options(k, a, sheet, BooleanOperation::Slice, options) {
        Ok(r) => r.result,
        Err(e) => return Ok(Err(status(e))),
    };
    Ok(Ok((!r.face_list().is_empty()).then_some(r)))
}

/// Both parts, `side` making each side's sheet (`true`: turned over, for the back). A body
/// that is no manifold solid is [`Error::NotSolid`]; a side left empty is `None`, and with
/// `whole` so is a part that is no manifold solid: a slice with a sheet that only nicks a
/// region behaves as a difference, leaving the sheet's piece as a slit in the body and a
/// lamina on the other side, which is no cut.
fn both_sides(
    k: &mut Kernel,
    body: &Body,
    whole: bool,
    mut side: impl FnMut(&mut Kernel, bool) -> solidcore::Result<Result<Brep>>,
) -> solidcore::Result<Result<Option<(Brep, Brep)>>> {
    if !is_manifold_solid(&body.lock())? {
        return Ok(Err(Error::NotSolid));
    }
    let mut parts = [None, None];
    for (part, flip) in parts.iter_mut().zip([true, false]) {
        let sheet = match side(k, flip)? {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };
        *part = match slice(k, body, sheet)? {
            Ok(p) => p,
            Err(e) => return Ok(Err(e)),
        };
    }
    let [Some(back), Some(front)] = parts else {
        return Ok(Ok(None));
    };
    if whole && !(is_manifold_solid(&back)? && is_manifold_solid(&front)?) {
        return Ok(Ok(None));
    }
    Ok(Ok(Some((back, front))))
}

/// The manifold solid `body` cut in two by `tool`: the part behind the surface (its normal
/// points away) and the part in front, each closed by the tool's piece inside the body and
/// each possibly several solids. A tool that misses leaves both parts the whole body. `None`
/// when a side is left empty. `body` is unchanged.
///
/// # Errors
/// [`Error::NotSolid`] for a body that is no manifold solid; the kernel's errors (a tool it
/// refuses is its status).
pub fn split_body(body: &Body, tool: &Tool) -> Result<Option<(Body, Body)>> {
    let r = run_on("split_body", &[("brep", body)], |k| {
        let tolerance = body.lock().tolerance();
        let t = match made_all(k, std::slice::from_ref(tool))? {
            Ok(mut t) => match t.pop() {
                Some(t) => t,
                None => return Ok(Err(Error::Argument)),
            },
            Err(e) => return Ok(Err(e)),
        };
        both_sides(k, body, false, |k, flip| {
            let mut surface = t.surface.copy(k)?;
            if flip {
                if let Err(e) = surface.swap_uv(k) {
                    return Ok(Err(status(e)));
                }
            }
            Ok(Ok(sheet_on(k, surface, tolerance)?.0))
        })
    })??;
    Ok(r.map(|(b, f)| (Body::new(b), Body::new(f))))
}

/// The manifold solid `body` cut in two by `sheet` as it is (its faces joined edge to edge; it
/// must cross the body to cut it), as [`split_body`] cuts it: the sheet's surfaces turned to
/// agree with record face `face`'s (an index into `sheet_order`), the part behind that normal
/// and the part in front. `None` when the sheet does not divide the body. Both operands are
/// unchanged.
///
/// # Errors
/// - [`Error::Argument`]: `sheet` no sheet, or `body` itself, or `face` past its list;
/// - [`Error::NotSolid`]: a body that is no manifold solid;
/// - the kernel's errors.
pub fn split_body_by_sheet(
    body: &Body,
    sheet: &Body,
    sheet_order: &Order,
    face: usize,
) -> Result<Option<(Body, Body)>> {
    if std::ptr::eq(body, sheet) {
        return Err(Error::Argument);
    }
    let picked = *sheet_order.faces.get(face).ok_or(Error::Argument)?;
    let r = run_on(
        "split_body_by_sheet",
        &[("brep", body), ("sheet", sheet)],
        |k| {
            {
                let g = sheet.lock();
                if !is_sheet(k, &g)? || !g.face_list().contains(&picked) {
                    return Ok(Err(Error::Argument));
                }
            }
            both_sides(k, body, true, |k, flip| {
                let (mut copy, map) = brep_copy_mapped(k, &sheet.lock())?;
                let Some([picked]) = copies_of(&map, &[picked], EntityRef::Face, |e| match e {
                    EntityRef::Face(f) => Some(f),
                    _ => None,
                })
                .and_then(|v| <[FaceId; 1]>::try_from(v).ok()) else {
                    return Ok(Err(Error::Argument));
                };
                // the UV swaps notify through the kernel's callbacks with the sheet's own
                // attributes, so what is attached to its faces sees each swap
                let turned = with_kernel_callbacks(k, &mut copy, |k, b, env| {
                    orient_sheet_surfaces(k, b, Some(picked), !flip, env)
                });
                copy.refresh_cached_topology();
                Ok(match turned {
                    Ok(()) => Ok(copy),
                    Err(e) => Err(status(e)),
                })
            })
        },
    )??;
    Ok(r.map(|(b, f)| (Body::new(b), Body::new(f))))
}

#[cfg(test)]
mod tests;
