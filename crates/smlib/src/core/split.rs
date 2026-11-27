//! Split Face and Split Body: the C API's `smlib_split_faces`, `smlib_split_body` and
//! `smlib_split_body_by_sheet` (the fork's `capi/src/smlib_split.cpp`), each solidcore-ops' own
//! ([`solidcore_ops::split`]) with its tool as the record surface its description makes
//! ([`ops_tool`]). The tool surfaces of `capi/src/smlib_ssi.cpp` (`smlib_surface_plane`,
//! `_cylinder`, `_cone`, `_sphere`, `_torus`, `_nurbs`) for the surface/surface intersection are
//! solidcore-ops' patches ([`super::ssi`]).

use super::Body;
use super::call::ops_as;
use crate::ssi::Spec;
use crate::{Error, Result};

/// `SMLIB_SPLIT_MAX_FACES`, `SMLIB_SPLIT_MAX_TOOLS`.
pub(crate) const MAX_FACES: usize = 100_000;
pub(crate) const MAX_TOOLS: usize = 4096;

/// `spec` as solidcore-ops' split tool: the record surface its numbers describe and the box of
/// its parameters (radians for angles), which the kernel's `from_parameters` makes into the
/// surface `smlib_ssi.cpp`'s constructors make ([`super::ssi::surface`]); a revolution's frame made unit and
/// square from the given axes. [`Error::Argument`] for axes that make no frame. A cone's box is its whole turn; a
/// NURBS net's is its knot range, rows of `nv` points, weighted as homogeneous points.
fn ops_tool(spec: &Spec) -> Result<solidcore_ops::split::Tool> {
    use solidcore_record::brep::{Frame, Surface as R};
    // a revolution's frame as the record holds it: unit, square axes, the x axis along the
    // given one and the y axis turned square to it in their plane, as the kernel's
    // constructors took the given axes
    let frame = |origin: [f64; 3], x: [f64; 3], y: [f64; 3]| -> Result<Frame> {
        let [x_axis, y_axis] = solidcore_ops::affine::orthonormal_axes(x, y).map_err(|_| Error::Argument)?;
        Ok(Frame { origin, x_axis, y_axis })
    };
    let rad = |d: [f64; 2]| [d[0].to_radians(), d[1].to_radians()];
    let (surface, domain) = match spec {
        Spec::Plane { origin, xaxis, yaxis, u, v } => (R::Plane { origin: *origin, u_axis: *xaxis, v_axis: *yaxis }, [*u, *v]),
        Spec::Cylinder { origin, xaxis, yaxis, radius, angle_deg, height } => {
            (R::Cylinder { frame: frame(*origin, *xaxis, *yaxis)?, radius: *radius }, [rad(*angle_deg), *height])
        }
        Spec::Cone { apex, xaxis, yaxis, half_angle_deg, height } => (
            R::Cone { frame: frame(*apex, *xaxis, *yaxis)?, radius_at_origin: 0.0, half_angle: half_angle_deg.to_radians() },
            [[0.0, std::f64::consts::TAU], *height],
        ),
        Spec::Sphere { center, xaxis, yaxis, radius, lon_deg, lat_deg } => {
            (R::Sphere { frame: frame(*center, *xaxis, *yaxis)?, radius: *radius }, [rad(*lon_deg), rad(*lat_deg)])
        }
        Spec::Torus { center, xaxis, yaxis, major_radius, minor_radius, major_deg, minor_deg } => (
            R::Torus { frame: frame(*center, *xaxis, *yaxis)?, major_radius: *major_radius, minor_radius: *minor_radius, tube_start: 0.0 },
            [rad(*major_deg), rad(*minor_deg)],
        ),
        Spec::Nurbs { nv, knots_u, knots_v, points, weights, .. } => {
            let range = |k: &[f64]| [k.first().copied().unwrap_or(0.0), k.last().copied().unwrap_or(0.0)];
            let domain = [range(knots_u), range(knots_v)];
            let rows = points.chunks((*nv).max(1));
            let surface = match weights {
                None => R::Spline { u_knots: knots_u.clone(), v_knots: knots_v.clone(), points: rows.map(<[_]>::to_vec).collect() },
                Some(w) => R::RationalSpline {
                    u_knots: knots_u.clone(),
                    v_knots: knots_v.clone(),
                    homogeneous: rows
                        .enumerate()
                        .map(|(i, row)| {
                            row.iter()
                                .enumerate()
                                .map(|(j, p)| {
                                    let wt = w.get(i * nv + j).copied().unwrap_or(1.0);
                                    [p[0] * wt, p[1] * wt, p[2] * wt, wt]
                                })
                                .collect()
                        })
                        .collect(),
                },
            };
            (surface, domain)
        }
    };
    Ok(solidcore_ops::split::Tool { surface, domain })
}

/// solidcore-ops' refusal of a split as the capi's: a tool that ends inside a face as
/// [`Error::ToolStopsShort`], the rest as solidcore-ops' blends and stitches are worded
/// ([`super::blend::refused`]).
fn split_error(call: &'static str, e: solidcore_ops::Error, detail: Option<String>) -> Error {
    match e {
        solidcore_ops::Error::ToolStopsShort => Error::ToolStopsShort,
        solidcore_ops::Error::NotSolid => Error::NotSolid,
        e => super::blend::refused(call, e, detail),
    }
}

/// `smlib_split_faces`: a copy of `body` with faces `faces` (indices in its face order, distinct)
/// cut where each tool meets them, the tools in turn, each cutting the pieces the earlier ones
/// left; and for each face how many faces it became besides itself
/// ([`solidcore_ops::split::split_faces`]). `None` when no face is split (`SMLIB_ERR_NO_CUT`), and
/// [`Error::ToolStopsShort`] when none is but a tool ran into a face and ended inside it.
pub(crate) fn split_faces(body: &Body, faces: &[usize], tools: &[&Spec]) -> Result<Option<(Body, Vec<usize>)>> {
    if faces.is_empty() || faces.len() > MAX_FACES || tools.is_empty() || tools.len() > MAX_TOOLS {
        return Err(Error::Argument);
    }
    if body.kernel_left_unfinished() {
        return Err(Error::Internal("smlib_split_faces: the body was left unfinished by an earlier panic".to_owned()));
    }
    let order = body.own_order();
    let tools = tools.iter().map(|t| ops_tool(t)).collect::<Result<Vec<_>>>()?;
    ops_as("smlib_split_faces", &[("brep", body)], split_error, || solidcore_ops::split::split_faces(body, &order, faces, &tools))
}

/// `smlib_split_body`: the manifold solid `body` cut in two by the tool: the part behind the
/// surface (its normal points away) and the part in front; a tool that closes on itself cuts it
/// into the part inside and the part outside ([`solidcore_ops::split::split_body`]). A side left
/// empty is `None` (`SMLIB_ERR_NO_CUT`), and so is a tool that ends inside the body; a body that
/// is no manifold solid is [`Error::NotSolid`].
pub(crate) fn split_body(body: &Body, spec: &Spec) -> Result<Option<(Body, Body)>> {
    if body.kernel_left_unfinished() {
        return Err(Error::Internal("smlib_split_body: the body was left unfinished by an earlier panic".to_owned()));
    }
    let tool = ops_tool(spec)?;
    ops_as("smlib_split_body", &[("brep", body)], split_error, || solidcore_ops::split::split_body(body, &tool))
}

/// `smlib_split_body_by_sheet`: the manifold solid `body` cut in two by `sheet` as it is, its
/// surfaces turned to agree with face `face`'s (its face order), for the back all turned over
/// ([`solidcore_ops::split::split_body_by_sheet`]). A sheet that does not divide the body is
/// `None` (`SMLIB_ERR_NO_CUT`). A `sheet` that is no sheet, or a `face` out of range, is
/// [`Error::Argument`].
pub(crate) fn split_body_by_sheet(body: &Body, sheet: &Body, face: usize) -> Result<Option<(Body, Body)>> {
    if std::ptr::eq(body, sheet) {
        return Err(Error::Argument);
    }
    if body.kernel_left_unfinished() || sheet.kernel_left_unfinished() {
        return Err(Error::Internal("smlib_split_body_by_sheet: a body was left unfinished by an earlier panic".to_owned()));
    }
    let order = sheet.own_order();
    ops_as("smlib_split_body_by_sheet", &[("brep", body), ("sheet", sheet)], split_error, || {
        solidcore_ops::split::split_body_by_sheet(body, sheet, &order, face)
    })
}
