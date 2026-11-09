//! Display tessellation plans: the parameter-space triangulation of every face of a body,
//! with the surfaces to evaluate it on, kept apart from the evaluation itself.
//!
//! The planner ([`crate::plan`]) decides *where* the vertices go (in each face's (u, v) parameters, with the
//! shared edge nodes fixed in 3D so neighbouring faces meet exactly) and *how* they are
//! connected. Evaluating the surfaces at those parameters is the bulk of the work and has no
//! branching on topology, so it can run anywhere: [`TessPlan::evaluate`] does it on the CPU,
//! and the GPU evaluator runs the same maths in a compute shader. Both evaluate the kernel's
//! [`EvalSurface`] forms, so the two cannot drift apart.

use crate::{Mesh, RefusedFace};
use solidcore::eval::EvalSurface;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::Vec3;

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
    /// The boundary nodes where the surface has no normal (a pole of higher order), each with
    /// the normal its triangles get there ([`crate::PoleRule`]).
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

    /// Triangle `t` is on this face's own nodes.
    fn has_nodes(&self, t: &[u32; 3]) -> bool {
        t.iter().all(|&i| (i as usize) < self.uv.len())
    }

    /// Why this face of a plan with `surfaces` surfaces is malformed, if it is: a triangle off
    /// its nodes, a surface index past the plan's, or (with no surface) a vertex whose position
    /// or normal is not given. Such a face is refused whole by [`TessPlan::evaluate`].
    fn malformed(&self, surfaces: usize) -> Option<String> {
        if let Some(k) = self.triangles.iter().position(|t| !self.has_nodes(t)) {
            return Some(format!(
                "triangle {k} has a corner past its {} nodes",
                self.uv.len()
            ));
        }
        match self.surface {
            Some(i) if i as usize >= surfaces => {
                Some(format!("its surface {i} is not in the plan"))
            }
            Some(_) => None,
            None => {
                let n = self.uv.len();
                let missing = if self.fixed.len() < n {
                    Some(("position", self.fixed.len()))
                } else if self.normals.len() < n {
                    Some(("normal", self.normals.len()))
                } else {
                    None
                };
                missing.map(|(what, k)| {
                    format!("vertex {k} has no surface to evaluate and no {what} given")
                })
            }
        }
    }
}

/// A body's display tessellation, ready to evaluate.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TessPlan {
    /// The surfaces the faces are evaluated on, in the kernel evaluator's form.
    pub surfaces: Vec<EvalSurface>,
    /// Each face's share, in the body's face order.
    pub faces: Vec<FacePlan>,
    /// B-rep edges as polylines (the shared edge nodes), in the body's edge order; empty for an
    /// edge that could not be sampled. Every face it bounds is then refused, each named in
    /// [`crate::PlanStats::unmeshed`] with its first reason, which is another one where the face
    /// was refused before its edges were sampled (its surface): the edge's own error then shows
    /// nowhere. An edge bounding no face is named in [`crate::PlanStats::unsampled_edges`].
    pub edges: Vec<Vec<Vec3>>,
    /// Per edge: the faces it bounds, in the order of its uses, then any face whose loops run it
    /// although its uses do not list it.
    pub edge_faces: Vec<Vec<u32>>,
}

impl TessPlan {
    /// Evaluate on the CPU into an indexed mesh (vertices are shared within a face, not
    /// across faces, so creases keep their own normals), with the faces left out of it.
    ///
    /// At a pole of higher order the plan names (a boundary node with no normal,
    /// [`crate::PoleNode`]), the vertex is emitted once per incident triangle with the surface's
    /// limit normal along that triangle, or once with the mean of its triangles' normals where
    /// the plan says the kernel refused that limit ([`crate::PlanStats::averaged_poles`]).
    ///
    /// A face a vertex of which the kernel's evaluator refuses otherwise (no value there, a
    /// point with no normal the plan does not name) is left out whole, nothing substituted, and
    /// returned with why: the caller reports it as unmeshed.
    #[must_use]
    pub fn evaluate(&self) -> (Mesh, Vec<RefusedFace>) {
        let mut out = Mesh::default();
        let mut refused = Vec::new();
        for f in &self.faces {
            // A malformed face is refused whole: nothing stands in for what it lacks, and no
            // triangle of it is dropped.
            if let Some(reason) = f.malformed(self.surfaces.len()) {
                refused.push(RefusedFace {
                    face: f.face as usize,
                    reason,
                });
                continue;
            }
            let (Some(mut positions), Some(mut normals)) =
                (crate::with_room(f.uv.len()), crate::with_room(f.uv.len()))
            else {
                refused.push(RefusedFace {
                    face: f.face as usize,
                    reason: format!("its {} vertices cannot be held", f.uv.len()),
                });
                continue;
            };
            // The nodes at a pole of higher order the plan names (no normal there).
            let mut at_poles: Vec<usize> = Vec::new();
            let surface = f.surface.and_then(|i| self.surfaces.get(i as usize));
            match surface {
                Some(s) => {
                    let mut why = None;
                    for (k, uv) in f.uv.iter().enumerate() {
                        match s.eval(uv[0], uv[1]) {
                            Ok(pn) => {
                                let (p, n) = solidcore_convert::record_pair(pn);
                                positions.push(f.fixed.get(k).copied().unwrap_or(p));
                                normals.push(if f.flip { -n } else { n });
                            }
                            // Its normals are its rule's, below; its point the shared node's.
                            Err(solidcore::eval::EvalError::Degenerate(_))
                                if f.pole(k).is_some() && k < f.fixed.len() =>
                            {
                                positions.push(f.fixed.get(k).copied().unwrap_or_default());
                                normals.push(Vec3::default());
                                at_poles.push(k);
                            }
                            Err(e) => {
                                why = Some(format!(
                                    "vertex {k} at {uv:?}: {}",
                                    crate::eval_message(e)
                                ));
                                break;
                            }
                        }
                    }
                    if let Some(reason) = why {
                        refused.push(RefusedFace {
                            face: f.face as usize,
                            reason,
                        });
                        continue;
                    }
                }
                // No surface: every vertex's position and normal are the plan's own, as many as
                // it has vertices (checked by `malformed` above).
                None => {
                    for (p, n) in f.fixed.iter().zip(&f.normals).take(f.uv.len()) {
                        positions.push(*p);
                        normals.push(if f.flip { -*n } else { *n });
                    }
                }
            }
            // At the poles: one copy per triangle with its limit normal, or the averaged one.
            let mut local = None;
            if let Some(s) = surface
                && !at_poles.is_empty()
            {
                match crate::pole::apply(s, f, &at_poles, positions, normals) {
                    Ok((p, n, t)) => {
                        (positions, normals) = (p, n);
                        local = Some(t);
                    }
                    Err(reason) => {
                        refused.push(RefusedFace {
                            face: f.face as usize,
                            reason,
                        });
                        continue;
                    }
                }
            }
            // The mesh's indices are u32: a face whose vertices would lie past them is refused.
            let base = out.positions.len();
            let at = |i: u32| {
                base.checked_add(i as usize)
                    .and_then(|j| u32::try_from(j).ok())
            };
            let triangles: Option<Vec<[u32; 3]>> = local
                .as_ref()
                .unwrap_or(&f.triangles)
                .iter()
                .map(|t| Some([at(t[0])?, at(t[1])?, at(t[2])?]))
                .collect();
            let Some(triangles) = triangles else {
                refused.push(RefusedFace {
                    face: f.face as usize,
                    reason: format!("its vertices lie past the mesh's {} index", u32::MAX),
                });
                continue;
            };
            out.positions.extend(positions);
            out.normals.extend(normals);
            out.tri_face
                .extend(std::iter::repeat_n(f.face, triangles.len()));
            out.triangles.extend(triangles);
        }
        out.edges.clone_from(&self.edges);
        out.edge_faces.clone_from(&self.edge_faces);
        (out, refused)
    }

    /// Which of the body's `faces` faces have triangles in [`TessPlan::evaluate`]'s mesh,
    /// without evaluating it: the faces with triangles that are not malformed. A face the
    /// evaluator refuses at a vertex is only found by evaluating, so it is counted here.
    pub fn faces_with_triangles(&self, faces: usize) -> Vec<bool> {
        let mut has = vec![false; faces];
        for f in &self.faces {
            if !f.triangles.is_empty()
                && f.malformed(self.surfaces.len()).is_none()
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
        let surfaces = self
            .surfaces
            .iter()
            .map(|s| match s {
                EvalSurface::Plane { origin, du, dv } => EvalSurface::Plane {
                    origin: vec3(point(record_vec3(*origin))),
                    du: vec3(vector(record_vec3(*du))),
                    dv: vec3(vector(record_vec3(*dv))),
                },
                // An affine map keeps a surface closed or open.
                EvalSurface::Nurbs(n) => EvalSurface::nurbs_closed(
                    n.degree(),
                    n.knots().clone(),
                    n.count(),
                    // Homogeneous: the translation scales with the weight.
                    n.ctrl()
                        .iter()
                        .map(|c| {
                            let q = vector(Vec3::new(c[0], c[1], c[2]))
                                + Vec3::new(m[3][0], m[3][1], m[3][2]) * c[3];
                            [q.x, q.y, q.z, c[3]]
                        })
                        .collect(),
                    n.closed(),
                ),
                EvalSurface::Revolved {
                    profile,
                    origin,
                    axis,
                    transform,
                    swap,
                } => {
                    // `m` (as rows) after the surface's own affine map.
                    let rows = [
                        [m[0][0], m[1][0], m[2][0], m[3][0]],
                        [m[0][1], m[1][1], m[2][1], m[3][1]],
                        [m[0][2], m[1][2], m[2][2], m[3][2]],
                    ];
                    let t = transform;
                    let columns = [
                        [t[0][0], t[1][0], t[2][0]],
                        [t[0][1], t[1][1], t[2][1]],
                        [t[0][2], t[1][2], t[2][2]],
                        [t[0][3], t[1][3], t[2][3]],
                    ];
                    let composed = rows.map(|row| {
                        let mut out = [0.0; 4];
                        for ((j, slot), column) in out.iter_mut().enumerate().zip(columns) {
                            *slot = row.iter().zip(column).map(|(r, c)| r * c).sum::<f64>()
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
    use solidcore::eval::EvalCurve;

    fn close(a: Vec3, b: Vec3, tol: f64) -> bool {
        a.dist(b) <= tol
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
            origin: solidcore::Vec3::ZERO,
            axis: solidcore::Vec3::Z,
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
        let ((a, refused_a), (b, refused_b)) = (plan.evaluate(), plan.transformed(&m).evaluate());
        // Both faces evaluated, moved and not: three vertices each.
        assert!(
            refused_a.is_empty() && refused_b.is_empty(),
            "{refused_a:?} {refused_b:?}"
        );
        assert_eq!((a.positions.len(), b.positions.len()), (6, 6));
        assert_eq!(a.tri_face, vec![0, 1]);
        assert_eq!(a.triangles, b.triangles);
        for (p, q) in a.positions.iter().zip(&b.positions) {
            assert!(close(apply(*p), *q, 1e-12), "{p:?} -> {q:?}");
        }
        for (n, k) in a.normals.iter().zip(&b.normals) {
            assert!(close(apply(*n) - apply(Vec3::ZERO), *k, 1e-12));
        }
        assert!(close(apply(a.edges[0][0]), b.edges[0][0], 1e-12));
    }

    fn line(a: Vec3, b: Vec3) -> EvalCurve {
        EvalCurve::new(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![[a.x, a.y, a.z, 1.0], [b.x, b.y, b.z, 1.0]],
        )
    }

    /// A plane face: its fixed vertex used exactly, its normals flipped. The face with a
    /// triangle off its nodes is refused whole, with why, and none of its triangles is kept.
    #[test]
    fn evaluate_uses_fixed_points_and_refuses_bad_indices() {
        let face = |triangles: Vec<[u32; 3]>| FacePlan {
            face: 3,
            surface: Some(0),
            uv: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            fixed: vec![Vec3::new(0.0, 0.0, 1e-9)],
            normals: vec![],
            flip: true,
            triangles,
            poles: Vec::new(),
        };
        let plan = TessPlan {
            surfaces: vec![EvalSurface::Plane {
                origin: solidcore::Vec3::ZERO,
                du: solidcore::Vec3::X,
                dv: solidcore::Vec3::Y,
            }],
            faces: vec![face(vec![[0, 1, 2]])],
            edges: vec![],
            edge_faces: vec![],
        };
        let (m, refused) = plan.evaluate();
        assert!(refused.is_empty(), "{refused:?}");
        assert_eq!(m.positions.len(), 3);
        assert_eq!(
            m.positions.first().copied(),
            Some(Vec3::new(0.0, 0.0, 1e-9))
        );
        assert_eq!(m.triangles, vec![[0, 1, 2]]);
        assert_eq!(m.tri_face, vec![3]);
        assert!(m.normals.iter().all(|n| close(*n, -Vec3::Z, 1e-12)));
        let bad = TessPlan {
            faces: vec![face(vec![[0, 1, 2], [0, 1, 7]])],
            ..plan.clone()
        };
        let (m, refused) = bad.evaluate();
        assert!(m.triangles.is_empty() && m.positions.is_empty());
        assert_eq!(
            refused,
            vec![RefusedFace {
                face: 3,
                reason: "triangle 1 has a corner past its 3 nodes".into()
            }]
        );
    }

    /// A face with no surface takes every position and normal from the plan. One missing a
    /// position or a normal is refused with why; nothing (the origin, +z) stands in for it.
    #[test]
    fn a_face_without_a_surface_and_a_vertex_is_refused() {
        let face = |face: u32, fixed: usize, normals: usize| FacePlan {
            face,
            surface: None,
            uv: vec![[0.0, 0.0]; 3],
            fixed: [Vec3::X, Vec3::Y, Vec3::Z]
                .into_iter()
                .take(fixed)
                .collect(),
            normals: vec![Vec3::Z; normals],
            flip: false,
            triangles: vec![[0, 1, 2]],
            poles: Vec::new(),
        };
        let plan = TessPlan {
            faces: vec![
                face(0, 3, 3),
                face(1, 2, 3),
                face(2, 3, 2),
                face(3, 3, 3),
                FacePlan {
                    surface: Some(4),
                    ..face(4, 3, 3)
                },
            ],
            ..TessPlan::default()
        };
        let (m, refused) = plan.evaluate();
        assert_eq!(m.tri_face, vec![0, 3]);
        assert_eq!(
            m.positions,
            vec![Vec3::X, Vec3::Y, Vec3::Z, Vec3::X, Vec3::Y, Vec3::Z]
        );
        let reasons: Vec<(usize, &str)> = refused
            .iter()
            .map(|r| (r.face, r.reason.as_str()))
            .collect();
        assert_eq!(
            reasons,
            vec![
                (
                    1,
                    "vertex 2 has no surface to evaluate and no position given"
                ),
                (2, "vertex 2 has no surface to evaluate and no normal given"),
                (4, "its surface 4 is not in the plan"),
            ]
        );
        // Counted without evaluating: the faces the mesh has.
        assert_eq!(
            plan.faces_with_triangles(5),
            vec![true, false, false, true, false]
        );
    }

    /// Faces with triangles, counted without evaluating, are the faces the mesh has: not a face
    /// whose triangle is off its nodes, nor one with none, nor one out of the body's range.
    #[test]
    fn faces_with_triangles_are_the_faces_the_mesh_has() {
        let only = |face: u32, triangles: Vec<[u32; 3]>| FacePlan {
            face,
            uv: vec![[0.0, 0.0]; 3],
            fixed: vec![Vec3::X, Vec3::Y, Vec3::Z],
            normals: vec![Vec3::Z; 3],
            triangles,
            ..Default::default()
        };
        let plan = TessPlan {
            faces: vec![
                only(0, vec![[0, 1, 9]]),
                only(1, vec![]),
                only(2, vec![[2, 1, 0]]),
                only(3, vec![[0, 1, 2]]),
                only(7, vec![[0, 1, 2]]),
            ],
            ..TessPlan::default()
        };
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
