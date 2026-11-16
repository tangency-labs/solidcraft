//! Skins (lofts) through profiles, with optional planar end caps.
//!
//! Upstream: `source/SMLib/src/SmPrimitiveCreation.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`PrimitiveCreation::create_skin_primitive`], [`PrimitiveCreation::create_skin_primitive_traced`] | nine-argument `CreateSkinPrimitive` |
//! | `skin_primitive_internal` (private) | `sm_CreateSkinPrimitiveInternal` |
//!
//! Deviations: only the ownership-reporting overload is in scope, so the internal
//! function always reports. A curve is transferred when its slot is consumed:
//! planar face construction takes the curves it uses, and composite construction
//! takes every curve of an open profile (upstream reports a lone curve that the
//! composite builder leaves out as not transferred; see `plan/findings.md`).
//! Profile counts past the curve list and missing curves (upstream out-of-range
//! and null reads) are errors. No recursion. Not upstream (tangency-labs/solidcore#3139):
//! [`PrimitiveCreation::create_skin_primitive_traced`] also traces each side face's
//! profile curves (`p3::skin_sources`), building the same skin.
//! Closed nonplanar wires are skinned as composite curves. Only the end caps
//! require planar wires; an interpolated interior wire need not lie in a plane.

use super::PrimitiveCreationAccess;
use crate::curve::composite_curve::CompositeCurve;
use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::math::{max2, min2, BIG_DOUBLE, EFF_ZERO_DEG, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::primitive::primitive_creation::p1::PrimitiveCreation;
use crate::primitive::primitive_creation::p3::skin_sources::{LoopSpans, Segment, SkinTrace};
use crate::sweep::topology_sweep::access::FaceRef;
use crate::topology::{BrepId, FaceId};

const OP: &str = "primitive::primitive_creation::p2::skin";

/// Whether the skin needs wire profiles rather than planar faces. Planarity is
/// checked on the joined B-spline's control hull, not a display polyline.
fn has_nonplanar_wire(
    k: &crate::kernel::Kernel,
    counts: &[usize],
    curves: &[Option<Curve>],
    tol: f64,
) -> Result<bool> {
    let mut start = 0usize;
    for &count in counts {
        let end = start
            .checked_add(count)
            .ok_or_else(|| SolidError::invalid_input(OP, "too many profile curves"))?;
        let profile = curves
            .get(start..end)
            .ok_or_else(|| SolidError::invalid_input(OP, "more profile curves than curves"))?
            .iter()
            .map(|c| {
                c.clone()
                    .ok_or_else(|| SolidError::primitive_missing(OP, "a profile curve"))
            })
            .collect::<Result<Vec<_>>>()?;
        let built = CompositeCurve::build_composites_from_curves(k, profile, tol, 0.0)?;
        for mut composite in built.composites {
            // The same plane tolerance as order_curves_into_loops uses.
            if composite.is_closed()
                && !composite
                    .make_composite_nurb()?
                    .is_planar(tol * 1000.0)?
                    .is_planar
            {
                return Ok(true);
            }
        }
        start = end;
    }
    Ok(false)
}

impl PrimitiveCreation {
    /// Loft the profiles of `curves`, `curves_in_each_profile` curves each, into the
    /// target B-rep (`CreateSkinPrimitive` with ownership reporting). Closed planar
    /// profiles become faces whose loops are lofted. Nonplanar closed wires are
    /// joined and skinned directly, with planar end caps when requested; open
    /// profiles have no caps. `approx_tol` 0 interpolates
    /// the profiles; `degree` is the loft degree (1 linear, 2 parabolic, 3 cubic).
    /// `transferred` gets one flag per curve: `true` when the call consumed it.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for fewer than two profiles, mismatched profiles or a failed cap;
    /// `SM_ERR_INVALID_INPUT` when capping a planar loft; those of the planar face,
    /// composite, skin and stitch steps.
    pub fn create_skin_primitive(
        &mut self,
        a: &mut impl PrimitiveCreationAccess,
        brep: BrepId,
        curves_in_each_profile: &[usize],
        curves: &mut [Option<Curve>],
        approx_tol: f64,
        degree: usize,
        cap_ends: bool,
        start_faces: &mut Vec<FaceId>,
        side_faces: &mut Vec<FaceId>,
        end_faces: &mut Vec<FaceId>,
        transferred: &mut Vec<bool>,
    ) -> Result<()> {
        skin_primitive_internal(
            self,
            a,
            brep,
            curves_in_each_profile,
            curves,
            approx_tol,
            degree,
            cap_ends,
            [start_faces, side_faces, end_faces],
            transferred,
            None,
        )
    }

    /// [`PrimitiveCreation::create_skin_primitive`], also tracing which profile curves
    /// each side face was swept from into `trace` (not upstream:
    /// [`crate::primitive::primitive_creation::p3::skin_sources`],
    /// tangency-labs/solidcore#3139). The B-rep and faces are those
    /// `create_skin_primitive` makes; the trace adds no failure.
    ///
    /// # Errors
    ///
    /// Those of [`PrimitiveCreation::create_skin_primitive`].
    #[allow(clippy::too_many_arguments)] // upstream's arguments and the trace
    pub fn create_skin_primitive_traced(
        &mut self,
        a: &mut impl PrimitiveCreationAccess,
        brep: BrepId,
        curves_in_each_profile: &[usize],
        curves: &mut [Option<Curve>],
        approx_tol: f64,
        degree: usize,
        cap_ends: bool,
        [start_faces, side_faces, end_faces]: [&mut Vec<FaceId>; 3],
        transferred: &mut Vec<bool>,
        trace: &mut SkinTrace,
    ) -> Result<()> {
        skin_primitive_internal(
            self,
            a,
            brep,
            curves_in_each_profile,
            curves,
            approx_tol,
            degree,
            cap_ends,
            [start_faces, side_faces, end_faces],
            transferred,
            Some(trace),
        )
    }
}

/// `sm_CreateSkinPrimitiveInternal`: the shared skin implementation.
fn skin_primitive_internal(
    primitive: &mut PrimitiveCreation,
    a: &mut impl PrimitiveCreationAccess,
    brep: BrepId,
    curves_in_each_profile: &[usize],
    curves: &mut [Option<Curve>],
    approx_tol: f64,
    degree: usize,
    cap_ends: bool,
    [start_faces, side_faces, end_faces]: [&mut Vec<FaceId>; 3],
    transferred: &mut Vec<bool>,
    trace: Option<&mut SkinTrace>,
) -> Result<()> {
    // empty entity maps
    primitive.init_entity_maps();

    // init outputs
    start_faces.clear();
    end_faces.clear();
    side_faces.clear();
    transferred.clear();
    transferred.resize(curves.len(), false);

    // check input, at least two profiles
    if curves_in_each_profile.len() < 2 {
        return Err(SolidError::primitive_failed(
            OP,
            "a skin needs at least two profiles",
        ));
    }

    // local region, brep and tolerance
    let region = primitive
        .region()
        .ok_or_else(|| SolidError::primitive_missing(OP, "the target region"))?;
    let tol = a.brep(brep)?.tolerance();

    // tell brep it's going to be modified, and that we'll be doing a Boolean
    a.set_editing(brep, true)?;
    let doing_boolean = a.kernel(brep)?.context.doing_boolean();
    a.kernel_mut(brep)?.context.set_doing_boolean(true);
    let result = (|| -> Result<()> {
        // make scratch brep
        let scratch = a.new_brep(brep)?;
        let result = skin_with_scratch(
            primitive,
            a,
            (brep, region),
            scratch,
            tol,
            curves_in_each_profile,
            curves,
            approx_tol,
            degree,
            cap_ends,
            [start_faces, side_faces, end_faces],
            transferred,
            trace,
        );
        a.delete_brep(scratch);
        result
    })();
    a.kernel_mut(brep)?.context.set_doing_boolean(doing_boolean);
    result
}

/// The skin's construction once its scratch B-rep exists.
fn skin_with_scratch(
    primitive: &mut PrimitiveCreation,
    a: &mut impl PrimitiveCreationAccess,
    (brep, region): (BrepId, crate::topology::RegionId),
    scratch: BrepId,
    tol: f64,
    curves_in_each_profile: &[usize],
    curves: &mut [Option<Curve>],
    approx_tol: f64,
    degree: usize,
    mut cap_ends: bool,
    [start_faces, side_faces, end_faces]: [&mut Vec<FaceId>; 3],
    transferred: &mut [bool],
    mut trace: Option<&mut SkinTrace>,
) -> Result<()> {
    a.set_editing(scratch, true)?;

    let mut count = 0usize;
    let mut faces: Vec<FaceRef> = Vec::new();
    let mut profile_faces: Vec<FaceId> = Vec::new();
    let mut nurbs = Vec::new();

    let wire_profiles = has_nonplanar_wire(a.kernel(brep)?, curves_in_each_profile, curves, tol)?;

    // profiles are boundaries of planar faces - they must be closed
    let mut open_profiles = false;

    // for every input profile: build a planar face bounded by it (closed planar
    // profiles), or a single composite curve to be lofted
    for (ii, &profile_count) in curves_in_each_profile.iter().enumerate() {
        let profile_start = count;
        count = count
            .checked_add(profile_count)
            .filter(|&end| end <= curves.len())
            .ok_or_else(|| SolidError::invalid_input(OP, "more profile curves than curves"))?;
        let range = profile_start..count;

        if !wire_profiles && (ii == 0 || !open_profiles) {
            // see if this profile is a set of co-planar loops
            let mut profile: Vec<Option<Curve>> = curves
                .get_mut(range.clone())
                .ok_or_else(|| {
                    SolidError::primitive_failed(OP, "a profile's curves are out of range")
                })?
                .iter_mut()
                .map(Option::take)
                .collect();
            let held: Vec<bool> = profile.iter().map(Option::is_some).collect();
            let infinite = a.infinite_region(scratch)?;
            let status = a
                .planar_faces_with_edges((scratch, infinite), &mut profile, tol, &mut profile_faces)
                .map(|edges| {
                    // (the trace: each edge with its curve's index among all the curves)
                    if let Some(t) = trace.as_deref_mut() {
                        t.edge_curves
                            .extend(edges.into_iter().map(|(e, i)| (e, profile_start + i)));
                    }
                });

            // A lower-level error can occur after one or more curves have already
            // transferred to lower-level topology: record those transfers.
            let mut partial_transfer = false;
            for (offset, (slot, &before)) in profile.iter().zip(&held).enumerate() {
                if before && slot.is_none() {
                    if let Some(flag) = transferred.get_mut(profile_start + offset) {
                        *flag = true;
                    }
                    partial_transfer = true;
                }
            }
            // Unconsumed curves stay with the caller.
            let slots = curves.get_mut(range.clone()).ok_or_else(|| {
                SolidError::primitive_failed(OP, "a profile's curves are out of range")
            })?;
            for (target, slot) in slots.iter_mut().zip(profile) {
                if slot.is_some() {
                    *target = slot;
                }
            }

            match status {
                Err(e) if e.is_unported_or_cancelled() => return Err(e),
                Err(e) => {
                    // A partially transferred closed profile is not reinterpreted as open.
                    if partial_transfer {
                        return Err(e);
                    }
                    if ii == 0 {
                        // treat input data like a composite curve
                        open_profiles = true;
                    } else {
                        // input profiles don't all match
                        return Err(SolidError::primitive_failed(
                            OP,
                            "the profiles do not all match",
                        ));
                    }
                }
                Ok(()) => {
                    // planar face construction consumes every curve on success
                    for flag in transferred.get_mut(range.clone()).unwrap_or_default() {
                        *flag = true;
                    }
                }
            }

            // error - more than one face per profile
            if profile_faces.len() > 1 {
                return Err(SolidError::primitive_failed(
                    OP,
                    "a profile makes more than one face",
                ));
            }
            faces.extend(profile_faces.iter().map(|&face| (scratch, face)));
        }

        // when input data has been found to be a sequence of loft curves
        if open_profiles || wire_profiles {
            // build a composite curve from these profile curves
            let brep_tol = a.brep(brep)?.tolerance();
            let mut profile = Vec::new();
            for slot in curves.get_mut(range.clone()).ok_or_else(|| {
                SolidError::primitive_failed(OP, "a profile's curves are out of range")
            })? {
                profile.push(
                    slot.take()
                        .ok_or_else(|| SolidError::primitive_missing(OP, "a profile curve"))?,
                );
            }
            for flag in transferred.get_mut(range.clone()).unwrap_or_default() {
                *flag = true;
            }
            let built = CompositeCurve::build_composites_from_curves(
                a.kernel(brep)?,
                profile,
                brep_tol,
                0.0,
            )?;
            if built.composites.len() != 1 {
                return Err(SolidError::primitive_failed(
                    OP,
                    "an open profile does not make one composite curve",
                ));
            }
            // build a B-spline curve for the composite curve
            let from = built.sources.into_iter().next().unwrap_or_default();
            let mut composite = built.composites.into_iter().next().ok_or_else(|| {
                SolidError::primitive_failed(OP, "an open profile makes no composite curve")
            })?;
            if wire_profiles && !composite.is_closed() {
                return Err(SolidError::primitive_failed(
                    OP,
                    "the profiles do not all match",
                ));
            }
            if let Some(t) = trace.as_deref_mut() {
                // (the trace: the composite's breaks, and each segment's curve among all the
                // curves, or none for a gap line; the curve is the one made untraced)
                let (joined, breaks) = composite.make_composite_nurb_with_breaks()?;
                let segments = from
                    .into_iter()
                    .map(|i| i.map_or(Segment::GapLine, |i| Segment::Curve(profile_start + i)))
                    .collect();
                t.open_loops.push(LoopSpans { breaks, segments });
                nurbs.push(joined);
            } else {
                nurbs.push(composite.make_composite_nurb()?);
            }
        }
    }

    // when data was found to be a sequence of loft curves
    if open_profiles {
        // don't make cap faces
        cap_ends = false;
        // clear out the stale input pointers
        for slot in curves.iter_mut() {
            *slot = None;
        }
    }

    // loft the loops on a sequence of faces or a sequence of loft curves
    primitive.create_skin_from_faces_traced(
        a, brep, &faces, &nurbs, approx_tol, degree, side_faces, trace,
    )?;

    // A planar loft (all profiles coplanar) has no depth to cap. A nonplanar
    // wire already proves that the profiles do not share one plane.
    if cap_ends && !wire_profiles {
        let k = a.kernel(brep)?;
        let &(first_brep, first) = faces
            .first()
            .ok_or_else(|| SolidError::primitive_missing(OP, "the skin's first face"))?;
        let (mut coplanar, point0, normal0) =
            match crate::topology::face::p2::surface(a.brep(first_brep)?, first)? {
                Some(surface) => {
                    let planar = surface.is_planar(k, tol)?;
                    (planar.is_planar, planar.point, planar.normal)
                }
                None => (false, crate::math::Vec3::UNDEF, crate::math::Vec3::UNDEF),
            };
        let coplanar_tol = max2(tol, approx_tol);
        for &(face_brep, face) in faces.iter().skip(1) {
            if !coplanar {
                break;
            }
            coplanar = match crate::topology::face::p2::surface(a.brep(face_brep)?, face)? {
                Some(surface) => {
                    let planar = surface.is_planar(k, tol)?;
                    planar.is_planar
                        && normal0.is_parallel_to(planar.normal, EFF_ZERO_DEG)
                        && normal0.dot(planar.point - point0).abs() <= coplanar_tol
                }
                None => false,
            };
        }
        if coplanar {
            return Err(SolidError::invalid_input(
                OP,
                "cannot cap a planar loft (coplanar profiles)",
            ));
        }
    }

    // When asked - create start and stop cap faces.
    if cap_ends {
        // Only side faces touching the full skin's U endpoints bound the caps.
        let k = a.kernel(brep)?;
        if side_faces.is_empty() {
            return Err(SolidError::primitive_failed(
                OP,
                "the skin made no side faces",
            ));
        }
        let mut skin_u_min = BIG_DOUBLE;
        let mut skin_u_max = -BIG_DOUBLE;
        for &face in side_faces.iter() {
            let surface = crate::topology::face::p2::surface(a.brep(brep)?, face)?
                .ok_or_else(|| SolidError::primitive_missing(OP, "the side face's surface"))?;
            let domain = surface.natural_uv_domain_with_nurb(k)?;
            skin_u_min = min2(skin_u_min, domain.min().x);
            skin_u_max = max2(skin_u_max, domain.max().x);
        }

        let mut start_curves = Vec::new();
        let mut end_curves = Vec::new();
        for &face in side_faces.iter() {
            let surface = crate::topology::face::p2::surface(a.brep(brep)?, face)?
                .ok_or_else(|| SolidError::primitive_missing(OP, "the side face's surface"))?;
            let domain = surface.natural_uv_domain_with_nurb(k)?;
            if (domain.min().x - skin_u_min).abs() <= EFF_ZERO_SQRT {
                let curve = surface
                    .create_iso_parametric_curve(
                        k,
                        SurfParamType::U,
                        domain.min().x,
                        approx_tol / 10.0,
                        None,
                        false,
                    )?
                    .curve;
                start_curves.push(Some(curve));
            }
            if (domain.max().x - skin_u_max).abs() <= EFF_ZERO_SQRT {
                let curve = surface
                    .create_iso_parametric_curve(
                        k,
                        SurfParamType::U,
                        domain.max().x,
                        approx_tol / 10.0,
                        None,
                        false,
                    )?
                    .curve;
                end_curves.push(Some(curve));
            }
        }
        if start_curves.is_empty() || end_curves.is_empty() {
            return Err(SolidError::primitive_failed(
                OP,
                "the skin has no end curves to cap",
            ));
        }

        // from list of all top and bottom curves - create needed planar cap faces
        a.planar_faces((brep, region), &mut start_curves, approx_tol, start_faces)?;
        a.planar_faces((brep, region), &mut end_curves, approx_tol, end_faces)?;
    }

    // stitch all the faces into a solid
    a.stitch_faces(brep, approx_tol)
}
