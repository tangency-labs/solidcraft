//! Readable names of a sheet metal body's faces, from the sheet itself: `<panel>:top`,
//! `<panel>:bottom` and `<panel>:rim.<edge>` (the names sketches on sheet faces keep, see
//! [`SheetBody::panel_face_at`]); a bend's curved faces, `<flange>:bend.inner` and
//! `<flange>:bend.outer`; and where a flange's panel runs on past its bend's ends (toward a
//! neighbouring flange), the rim along the bend there, `<flange>:rim.ext0` and `rim.ext1`. A panel is `base` or a flange's tag (`F<feature>.<k>`), so the names
//! say what each face is and stay with it when sizes change or other flanges come and go.

use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_geom::brep::eval::SurfaceEval;
use solidcraft_geom::prelude::*;

use super::SheetBody;
use crate::{ModelBody, ModelState, apply_point, mat_inverse};

/// The share of a face's sample points that must agree on a name for the face to take it.
const AGREE: f64 = 0.8;

/// The sine of the angle within which a flat face's plane faces along a triangle's normal.
const FACING: f64 = 1e-3;

impl SheetBody {
    /// The readable name of the face of this sheet a point lies on (sheet coordinates, within
    /// `tol`, kept below a quarter of the thickness): a flat face's (see
    /// [`panel_face_at`](Self::panel_face_at)), else a bend's inner or outer face.
    pub fn face_name_at(&self, p: Vec3, tol: f64) -> Option<String> {
        self.panel_face_at(p, tol).map(|(name, _)| name).or_else(|| self.bend_face_at(p, tol)).or_else(|| self.ext_rim_at(p, tol))
    }

    /// [`face_name_at`](Self::face_name_at) for a point on a face whose normal is `normal` (a
    /// mesh triangle's, either way round): a flat face's name is taken only where its plane faces
    /// along `normal`. A rim is as narrow as the sheet is thick, so the centres of its triangles
    /// can lie within `tol` of the panel's top or bottom face beside it.
    fn face_name_facing(&self, p: Vec3, normal: Vec3, tol: f64) -> Option<String> {
        let facing = |plane: &Plane| plane.normal().cross(normal).len() < FACING;
        match self.panel_face_at(p, tol) {
            Some((name, plane)) if facing(&plane) => Some(name),
            Some(_) => self
                .rim_face_at(p, tol)
                .filter(|(_, plane)| facing(plane))
                .map(|(name, _)| name)
                .or_else(|| self.bend_face_at(p, tol))
                .or_else(|| self.ext_rim_at(p, tol)),
            None => self.bend_face_at(p, tol).or_else(|| self.ext_rim_at(p, tol)),
        }
    }

    /// The rim of a flange's panel along its bend line, where the panel runs on past the
    /// bend's start (`<flange>:rim.ext0`) or end (`rim.ext1`).
    fn ext_rim_at(&self, p: Vec3, tol: f64) -> Option<String> {
        if self.flat {
            return None;
        }
        let tol = tol.min(self.t * 0.25);
        for (pi, region) in self.panels() {
            let Some(i) = pi else { continue };
            let Some(f) = self.flanges.get(i) else { continue };
            let Some(inverse) = mat_inverse(&self.world(pi)) else { continue };
            let q = apply_point(&inverse, p);
            if q.z < -tol || q.z > self.t + tol {
                continue;
            }
            let q2 = Vec2::new(q.x, q.y);
            let pts = region.outer.ccw().polyline(1e-3);
            let m = pts.len();
            for k in 0..m {
                let (Some(&a), Some(&b)) = (pts.get(k), pts.get((k + 1) % m)) else { continue };
                let d = b - a;
                if d.len2() < 1e-18 || self.rim_edge(pi, a, b, k).as_deref() != Some("bend") {
                    continue;
                }
                let along = (q2 - a).dot(d) / d.len2();
                if !(-1e-9..=1.0 + 1e-9).contains(&along) || ((a + d * along) - q2).len() > tol {
                    continue;
                }
                let end = if (q2 - f.p).dot(f.d) < f.len / 2.0 { "ext0" } else { "ext1" };
                return Some(format!("{}:rim.{end}", self.panel_name(pi)));
            }
        }
        None
    }

    /// The curved face of a bend a point lies on: `<flange>:bend.inner` (the bend's radius) or
    /// `<flange>:bend.outer` (the radius plus the thickness).
    fn bend_face_at(&self, p: Vec3, tol: f64) -> Option<String> {
        if self.flat {
            return None;
        }
        let tol = tol.min(self.t * 0.25);
        for (i, f) in self.flanges.iter().enumerate() {
            let Some((world, o, d, start, toward)) = self.bend_frame(f) else { continue };
            let Some(inverse) = mat_inverse(&world) else { continue };
            let q = apply_point(&inverse, p) - o;
            let u = q.dot(d);
            let (lo, hi) = self.zone_reach(f);
            if u < lo - tol || u > hi + tol {
                continue;
            }
            let radial = q - d * u;
            let rho = radial.len();
            let side = if (rho - f.radius).abs() <= tol {
                "inner"
            } else if (rho - f.radius - self.t).abs() <= tol {
                "outer"
            } else {
                continue;
            };
            let phi = radial.dot(toward).atan2(radial.dot(start));
            if phi < -1e-6 || phi > f.angle.abs() + 1e-6 {
                continue;
            }
            return Some(format!("{}:bend.{side}", self.panel_name(Some(i))));
        }
        None
    }
}

/// The readable sheet names of a body's faces (by B-rep face index), as
/// [`sheet_face_name`](super::sheet_face_name)s: `sheet:<body>:<panel>:<side>`. `None` for a face
/// the sheet does not name (a hole's wall, a pocket's floor) and for every face of a body that is
/// no folded sheet of `st`.
pub fn body_sheet_face_names(st: &ModelState, b: &ModelBody) -> Vec<Option<String>> {
    let rec = b.body.record();
    let nf = b.body.face_count();
    let Some(sheet) = st.sheets.iter().find(|s| s.body == b.name && !s.flat) else { return vec![None; nf] };
    // Points lie on the surface; a fifth of the thickness tells a flat face from the rim beside it.
    let tol = sheet.t * 0.2;
    let mut names = vec![None; nf];

    for (f, face) in rec.faces.iter().enumerate() {
        let plane_opt = b.body.face_plane(f).or_else(|| b.body.planar_face(f));
        let Ok(eval) = SurfaceEval::new(face.parameters()) else { continue };
        let [[u0, u1], [v0, v1]] = face.domain;
        if !u0.is_finite() || !u1.is_finite() || !v0.is_finite() || !v1.is_finite() {
            continue;
        }

        let mut votes: Vec<(String, usize)> = Vec::new();

        let samples = [0.15, 0.3, 0.5, 0.7, 0.85];
        for &ku in &samples {
            for &kv in &samples {
                let u = u0 + (u1 - u0) * ku;
                let v = v0 + (v1 - v0) * kv;
                let Some((pt, du, dv)) = eval.derivs(u, v) else { continue };
                let normal = du.cross(dv).normalized().or_else(|| plane_opt.as_ref().map(|p| p.normal()));
                let Some(normal) = normal else { continue };
                if let Some(name) = sheet.face_name_facing(pt, normal, tol) {
                    match votes.iter_mut().find(|(x, _)| *x == name) {
                        Some(x) => x.1 += 1,
                        None => votes.push((name, 1)),
                    }
                }
            }
        }

        let total: usize = votes.iter().map(|x| x.1).sum();
        if let Some((name, count)) = votes.into_iter().max_by_key(|x| x.1) {
            if total > 0 && count as f64 >= AGREE * total as f64 {
                names[f] = Some(super::sheet_face_name(&b.name, &name));
            }
        }
    }

    names
}
