//! A body's display tessellation, ready to evaluate: every face's triangulation in its own
//! parameters, with the surfaces to evaluate it on in the kernel's plan-evaluator form
//! ([`solidcore::eval::plan`]). The CPU evaluates it ([`TessPlan::evaluate`]); a GPU evaluates
//! the same plan in a compute shader.

use solidcore::Vec3 as K;
use solidcore::eval::plan::EvalSurface;
use solidcore_record::Vec3;

use crate::Mesh;

/// One face's share of a [`TessPlan`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FacePlan {
    /// The B-rep face (index in the body's face order).
    pub face: u32,
    /// Index into [`TessPlan::surfaces`]; `None` when every position and normal is given.
    pub surface: Option<u32>,
    /// Parameters of every vertex.
    pub uv: Vec<[f64; 2]>,
    /// Positions of the first `fixed.len()` vertices: the shared edge nodes, used exactly as
    /// they are so the neighbouring face gets bit-identical boundary points.
    pub fixed: Vec<Vec3>,
    /// Normals of every vertex, only for faces without an evaluable surface.
    pub normals: Vec<Vec3>,
    /// The face is reversed against its surface: normals point the other way.
    pub flip: bool,
    /// Counter-clockwise seen from outside (orientation already applied).
    pub triangles: Vec<[u32; 3]>,
    /// The face's boundary nodes at a pole of higher order, with the normal each gets.
    pub poles: Vec<crate::PoleNode>,
}

impl FacePlan {
    /// The rule of node `k` if the plan names it a pole of higher order.
    pub(crate) fn pole(&self, k: usize) -> Option<crate::PoleRule> {
        self.poles
            .iter()
            .find(|p| p.node as usize == k)
            .map(|p| p.rule)
    }

    /// Triangle `t` is on this face's own nodes (the evaluated mesh keeps only those).
    fn has_nodes(&self, t: &[u32; 3]) -> bool {
        t.iter().all(|&i| (i as usize) < self.uv.len())
    }
}

/// A body's display tessellation, ready to evaluate.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TessPlan {
    /// The faces' surfaces, in the plan evaluator's form.
    pub surfaces: Vec<EvalSurface>,
    /// Each face's share of the plan, in the body's face order.
    pub faces: Vec<FacePlan>,
    /// B-rep edges as polylines (the shared edge nodes), in the body's edge order.
    pub edges: Vec<Vec<Vec3>>,
    /// Per edge: the faces it bounds.
    pub edge_faces: Vec<Vec<u32>>,
}

/// One face's nodes as the mesh has them ([`TessPlan::nodes`]).
pub(crate) struct FaceNodes {
    /// Each node's position.
    pub(crate) positions: Vec<Vec3>,
    /// Each node's normal (turned with the face).
    pub(crate) normals: Vec<Vec3>,
    /// The kernel's form of the face's surface, where a node needed it.
    pub(crate) kernel: Option<solidcore::eval::EvalSurface>,
    /// The first node the kernel's evaluator refuses, and why.
    pub(crate) failure: Option<String>,
}

impl TessPlan {
    /// Face `f`'s node positions and normals as [`TessPlan::evaluate`] gives them, before its
    /// pole rules: each node evaluated on the face's surface (a shared edge node's own point
    /// kept), the kernel's form of the surface where a node needed it, and the first node the
    /// kernel's evaluator refuses, if one does (the face is then left out of the mesh).
    pub(crate) fn nodes(&self, f: &FacePlan) -> FaceNodes {
        let surface = f.surface.and_then(|i| self.surfaces.get(i as usize));
        // At a degenerate point (a cone's apex, a sphere's pole) the kernel's exact limit
        // normal, from the second partials; the kernel's form of the face is made only when a
        // vertex of it needs one.
        let mut kernel: Option<solidcore::eval::EvalSurface> = None;
        let (mut positions, mut normals) = (
            Vec::with_capacity(f.uv.len()),
            Vec::with_capacity(f.uv.len()),
        );
        // The first node the kernel's evaluator refuses, with its reason: the face is refused
        // there, not meshed with a normal made up for it. Its pole nodes have their own rule
        // in [`TessPlan::evaluate`] ([`crate::pole::apply`]).
        let mut failure: Option<String> = None;
        for (k, uv) in f.uv.iter().enumerate() {
            let (p, n) = match surface {
                Some(s) => {
                    solidcore_convert::record_pair(if let Some(pn) = s.regular(uv[0], uv[1]) {
                        pn
                    } else {
                        let ks = kernel.get_or_insert_with(|| crate::mesher::poles::kernel_form(s));
                        ks.eval(uv[0], uv[1]).unwrap_or_else(|e| {
                            if failure.is_none() && !f.poles.iter().any(|p| p.node as usize == k) {
                                failure = Some(format!(
                                    "node {k} at ({}, {}): {}",
                                    uv[0],
                                    uv[1],
                                    crate::eval_message(e)
                                ));
                            }
                            s.eval(uv[0], uv[1])
                        })
                    })
                }
                None => (Vec3::ZERO, f.normals.get(k).copied().unwrap_or(Vec3::Z)),
            };
            let p = f.fixed.get(k).copied().unwrap_or(p);
            positions.push(p);
            normals.push(if f.flip { -n } else { n });
        }
        FaceNodes {
            positions,
            normals,
            kernel,
            failure,
        }
    }

    /// Evaluate on the CPU into an indexed mesh (vertices are shared within a face, not
    /// across faces, so creases keep their own normals), with the faces left out of it.
    #[must_use]
    pub fn evaluate(&self) -> (Mesh, Vec<crate::RefusedFace>) {
        let mut out = Mesh::default();
        let mut refused = Vec::new();
        for f in &self.faces {
            let base = out.positions.len();
            let surface = f.surface.and_then(|i| self.surfaces.get(i as usize));
            let FaceNodes {
                mut positions,
                mut normals,
                mut kernel,
                failure,
            } = self.nodes(f);
            if let Some(reason) = failure {
                refused.push(crate::RefusedFace {
                    face: f.face as usize,
                    reason,
                });
                continue;
            }
            // At the poles of higher order the plan names: one copy per triangle with its limit
            // normal, or the mean of the triangles' normals, by the kernel's evaluator.
            let mut triangles = std::borrow::Cow::Borrowed(&f.triangles);
            if let Some(s) = surface
                && !f.poles.is_empty()
            {
                let at_poles: Vec<usize> = f
                    .poles
                    .iter()
                    .map(|p| p.node as usize)
                    .filter(|&k| k < f.fixed.len())
                    .collect();
                let k = kernel.get_or_insert_with(|| crate::mesher::poles::kernel_form(s));
                match crate::pole::apply(k, f, &at_poles, positions.clone(), normals.clone()) {
                    Ok((p, n, t)) => {
                        (positions, normals) = (p, n);
                        triangles = std::borrow::Cow::Owned(t);
                    }
                    // A normal the rule cannot give refuses the face, nothing substituted.
                    Err(reason) => {
                        refused.push(crate::RefusedFace {
                            face: f.face as usize,
                            reason,
                        });
                        continue;
                    }
                }
            }
            let nodes = positions.len();
            out.positions.extend(positions);
            out.normals.extend(normals);
            for t in triangles.iter() {
                if t.iter().all(|&i| (i as usize) < nodes) {
                    let at = |i: u32| u32::try_from(base + i as usize).unwrap_or(u32::MAX);
                    out.triangles.push([at(t[0]), at(t[1]), at(t[2])]);
                    out.tri_face.push(f.face);
                }
            }
        }
        out.edges.clone_from(&self.edges);
        out.edge_faces.clone_from(&self.edge_faces);
        (out, refused)
    }

    /// Which of the body's `faces` faces have triangles in [`TessPlan::evaluate`]'s mesh,
    /// without evaluating it.
    pub fn faces_with_triangles(&self, faces: usize) -> Vec<bool> {
        let mut has = vec![false; faces];
        for f in &self.faces {
            if f.triangles.iter().any(|t| f.has_nodes(t))
                && let Some(h) = has.get_mut(f.face as usize)
            {
                *h = true;
            }
        }
        has
    }

    /// Vertices across all faces.
    pub fn vertex_count(&self) -> usize {
        self.faces.iter().map(|f| f.uv.len()).sum()
    }

    /// This plan for the body moved by the rotation and translation `m` (by columns, the
    /// translation in `m[3]`): the same parameters and triangles on the moved surfaces, so a
    /// part placed many times is planned once. Only for proper rigid motions (a reflection
    /// would turn the triangles inside out; the caller checks).
    #[must_use]
    pub fn transformed(&self, m: &[[f64; 4]; 4]) -> TessPlan {
        let vector = |v: Vec3| {
            Vec3::new(
                m[0][0] * v.x + m[1][0] * v.y + m[2][0] * v.z,
                m[0][1] * v.x + m[1][1] * v.y + m[2][1] * v.z,
                m[0][2] * v.x + m[1][2] * v.y + m[2][2] * v.z,
            )
        };
        let point = |p: Vec3| vector(p) + Vec3::new(m[3][0], m[3][1], m[3][2]);
        // The same on the evaluator's surfaces, in the kernel's vectors.
        let (kvector, kpoint) = (
            |v: K| solidcore_convert::vec3(vector(solidcore_convert::record_vec3(v))),
            |p: K| solidcore_convert::vec3(point(solidcore_convert::record_vec3(p))),
        );
        let surfaces = self
            .surfaces
            .iter()
            .map(|s| match s {
                EvalSurface::Plane { origin, du, dv } => EvalSurface::Plane {
                    origin: kpoint(*origin),
                    du: kvector(*du),
                    dv: kvector(*dv),
                },
                EvalSurface::Nurbs {
                    degree,
                    knots,
                    count,
                    ctrl,
                    closed,
                } => EvalSurface::Nurbs {
                    degree: *degree,
                    // An affine map keeps a surface closed or open.
                    closed: *closed,
                    knots: knots.clone(),
                    count: *count,
                    // Homogeneous: the translation scales with the weight.
                    ctrl: ctrl
                        .iter()
                        .map(|c| {
                            let q = vector(Vec3::new(c[0], c[1], c[2]))
                                + Vec3::new(m[3][0], m[3][1], m[3][2]) * c[3];
                            [q.x, q.y, q.z, c[3]]
                        })
                        .collect(),
                },
                EvalSurface::Revolved {
                    profile,
                    origin,
                    axis,
                    transform,
                    swap,
                } => {
                    // `m` (as rows) after the surface's own affine map.
                    let r = |i: usize| m.map(|col| col.get(i).copied().unwrap_or(0.0));
                    let rows = [r(0), r(1), r(2)];
                    let t = transform;
                    let composed = rows.map(|row| {
                        let mut out = [0.0; 4];
                        for (j, slot) in out.iter_mut().enumerate() {
                            *slot = row
                                .iter()
                                .zip(t)
                                .map(|(r, tk)| r * tk.get(j).copied().unwrap_or(0.0))
                                .sum::<f64>()
                                + if j == 3 { row[3] } else { 0.0 };
                        }
                        out
                    });
                    EvalSurface::Revolved {
                        profile: profile.clone(),
                        origin: *origin,
                        axis: *axis,
                        transform: composed,
                        swap: *swap,
                    }
                }
            })
            .collect();
        let faces = self
            .faces
            .iter()
            .map(|f| FacePlan {
                fixed: f.fixed.iter().map(|p| point(*p)).collect(),
                normals: f.normals.iter().map(|n| vector(*n)).collect(),
                ..f.clone()
            })
            .collect();
        TessPlan {
            surfaces,
            faces,
            edges: self
                .edges
                .iter()
                .map(|e| e.iter().map(|p| point(*p)).collect())
                .collect(),
            edge_faces: self.edge_faces.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vec::VecExt as _;
    use solidcore::eval::plan::EvalCurve;

    fn close(a: Vec3, b: Vec3, tol: f64) -> bool {
        a.dist(b) <= tol
    }

    fn line(a: Vec3, b: Vec3) -> EvalCurve {
        EvalCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            ctrl: vec![[a.x, a.y, a.z, 1.0], [b.x, b.y, b.z, 1.0]],
        }
    }

    /// A moved plan evaluates to the plan's points moved, on a rational spline surface (the
    /// translation scales with each weight) and a revolved one (composed with its own map).
    #[test]
    fn a_transformed_plan_evaluates_to_the_moved_points() {
        let (c, s) = (0.6f64.cos(), 0.6f64.sin());
        // Turned about z by 0.6 rad, then moved: by columns.
        let m = [
            [c, s, 0.0, 0.0],
            [-s, c, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [5.0, -2.0, 7.0, 1.0],
        ];
        let apply =
            |p: Vec3| Vec3::new(c * p.x - s * p.y + 5.0, s * p.x + c * p.y - 2.0, p.z + 7.0);
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let nurbs = EvalSurface::nurbs(
            [2, 1],
            [vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]],
            [3, 2],
            vec![
                [1.0, 0.0, 0.0, 1.0],
                [w, w, 0.0, w],
                [0.0, 1.0, 0.0, 1.0],
                [1.0, 0.0, 2.0, 1.0],
                [w, w, 2.0 * w, w],
                [0.0, 1.0, 2.0, 1.0],
            ],
        );
        let revolved = EvalSurface::Revolved {
            profile: line(Vec3::new(2.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 1.0)),
            origin: K::ZERO,
            axis: K::Z,
            transform: [
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, -1.0, 2.0],
                [0.0, 1.0, 0.0, 3.0],
            ],
            swap: false,
        };
        let face = |i: u32| FacePlan {
            face: i,
            surface: Some(i),
            uv: vec![[0.1, 0.2], [0.7, 0.4], [0.3, 0.9]],
            fixed: vec![Vec3::new(1.0, 2.0, 3.0)],
            triangles: vec![[0, 1, 2]],
            ..Default::default()
        };
        let plan = TessPlan {
            surfaces: vec![nurbs, revolved],
            faces: vec![face(0), face(1)],
            edges: vec![vec![Vec3::new(1.0, 1.0, 1.0)]],
            edge_faces: vec![vec![0, 1]],
        };
        let (a, b) = (plan.evaluate().0, plan.transformed(&m).evaluate().0);
        assert_eq!(a.triangles, b.triangles);
        for (p, q) in a.positions.iter().zip(&b.positions) {
            assert!(close(apply(*p), *q, 1e-12), "{p:?} -> {q:?}");
        }
        for (n, k) in a.normals.iter().zip(&b.normals) {
            assert!(close(apply(*n) - apply(Vec3::ZERO), *k, 1e-12));
        }
        assert!(close(apply(a.edges[0][0]), b.edges[0][0], 1e-12));
    }

    #[test]
    fn evaluate_uses_fixed_points_and_drops_bad_indices() {
        let plan = TessPlan {
            surfaces: vec![EvalSurface::Plane {
                origin: K::ZERO,
                du: K::X,
                dv: K::Y,
            }],
            faces: vec![FacePlan {
                face: 3,
                surface: Some(0),
                uv: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                fixed: vec![Vec3::new(0.0, 0.0, 1e-9)],
                normals: vec![],
                flip: true,
                triangles: vec![[0, 1, 2], [0, 1, 7]],
                poles: vec![],
            }],
            edges: vec![],
            edge_faces: vec![],
        };
        let m = plan.evaluate().0;
        assert_eq!(m.positions.len(), 3);
        assert_eq!(
            m.positions.first().copied(),
            Some(Vec3::new(0.0, 0.0, 1e-9))
        );
        assert_eq!(m.triangles, vec![[0, 1, 2]]);
        assert_eq!(m.tri_face, vec![3]);
        assert!(m.normals.iter().all(|n| close(*n, -Vec3::Z, 1e-12)));
        // Faces with triangles, counted without evaluating, are the faces the mesh has: not a
        // face whose only triangle is off its nodes, nor one with none, nor one out of range.
        let mut plan = plan;
        let only = |face: u32, triangles: Vec<[u32; 3]>| FacePlan {
            face,
            uv: vec![[0.0, 0.0]; 3],
            triangles,
            ..Default::default()
        };
        plan.faces.extend([
            only(0, vec![[0, 1, 9]]),
            only(1, vec![]),
            only(2, vec![[2, 1, 0]]),
            only(7, vec![[0, 1, 2]]),
        ]);
        let has = plan.faces_with_triangles(5);
        let mut evaluated = vec![false; 5];
        for f in plan.evaluate().0.tri_face {
            if let Some(h) = evaluated.get_mut(f as usize) {
                *h = true;
            }
        }
        assert_eq!(has, evaluated);
        assert_eq!(has, vec![false, false, true, true, false]);
    }
}
