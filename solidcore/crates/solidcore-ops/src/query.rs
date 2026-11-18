//! Exact queries on a B-rep record ([`Query`]): points and tangents along its edges, how far a
//! point is from an edge, where a point lies on its faces and which way they face there, and
//! which faces a ray meets. The record's curves, surfaces and pcurves are copied into the
//! kernel's evaluation types through `solidcore-convert` once, when the query is made, and every
//! answer is the kernel's ([`solidcore::eval::query`]); nothing here computes.

use solidcore::eval::query as k;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::brep::{Brep, Loop, Shell};
use solidcore_record::Vec3;

/// A record ready for exact queries (see the module docs).
#[derive(Debug)]
pub struct Query<'a> {
    record: &'a Brep,
    inner: k::Query,
}

impl<'a> Query<'a> {
    /// The record made ready. A curve or surface the conversion refuses (geometry too deep) is
    /// one the queries cannot evaluate, as a malformed one.
    #[must_use]
    pub fn new(record: &'a Brep) -> Query<'a> {
        let mut forward = vec![None; record.faces.len()];
        for shell in &record.shells {
            if let Shell::Faces { faces, .. } = shell {
                for of in faces {
                    if let Some(slot) = forward.get_mut(of.face as usize) {
                        *slot = Some(of.forward);
                    }
                }
            }
        }
        let edges = record
            .edges
            .iter()
            .map(|e| k::Edge {
                curve: solidcore_convert::curve(e.parameters()).ok(),
                interval: e.interval,
                faces: e.uses.iter().map(|u| u.face as usize).collect(),
            })
            .collect();
        let faces = record
            .faces
            .iter()
            .zip(forward)
            .map(|(f, forward)| k::Face {
                parameters: solidcore_convert::surface(f.parameters()).ok(),
                surface: solidcore_convert::surface(&f.surface).ok(),
                domain: f.domain,
                forward,
                loops: f
                    .loops
                    .iter()
                    .map(|l| match l {
                        Loop::Edges { edges } => k::Loop::Edges(
                            edges
                                .iter()
                                .map(|u| k::Use {
                                    edge: u.edge as usize,
                                    forward: u.forward,
                                    pcurve: solidcore_convert::curve2(&u.pcurve),
                                })
                                .collect(),
                        ),
                        Loop::Vertex { vertex } => {
                            k::Loop::Vertex(record.vertices.get(*vertex as usize).map(|v| v.point))
                        }
                    })
                    .collect(),
            })
            .collect();
        Query {
            record,
            inner: k::Query::new(k::Record { edges, faces }),
        }
    }

    /// The record it reads.
    #[must_use]
    pub fn record(&self) -> &'a Brep {
        self.record
    }

    /// Edge `e`'s point and derivative at `t` on its parameter. `None` for a traced
    /// intersection curve (its tangent is not held).
    #[must_use]
    pub fn edge_at(&self, e: usize, t: f64) -> Option<(Vec3, Vec3)> {
        self.inner
            .edge_at(e, t)
            .map(|(p, d)| (record_vec3(p), record_vec3(d)))
    }

    /// Edge `e`'s interval on its parameter, from its start vertex to its end vertex.
    #[must_use]
    pub fn edge_interval(&self, e: usize) -> Option<[f64; 2]> {
        self.inner.edge_interval(e)
    }

    /// The faces edge `e` bounds, once for each time a loop uses it.
    #[must_use]
    pub fn edge_faces(&self, e: usize) -> Vec<usize> {
        self.inner.edge_faces(e)
    }

    /// Points of edge `e` from its start to its end, each with its parameter, as many as keep
    /// the curve within `sag` mm of the chords between them (a sample, not a bound).
    #[must_use]
    pub fn edge_points(&self, e: usize, sag: f64) -> Option<Vec<(f64, Vec3)>> {
        self.inner
            .edge_points(e, sag)
            .map(|v| v.into_iter().map(|(t, p)| (t, record_vec3(p))).collect())
    }

    /// How far `p` is from edge `e`.
    #[must_use]
    pub fn edge_distance(&self, e: usize, p: Vec3, sag: f64) -> Option<f64> {
        self.inner.edge_distance(e, vec3(p), sag)
    }

    /// The parameters on face `f` of `p`, a point within `accept` mm of its surface.
    #[must_use]
    pub fn face_uv(&self, f: usize, p: Vec3, accept: f64) -> Option<[f64; 2]> {
        self.inner.face_uv(f, vec3(p), accept)
    }

    /// Face `f`'s unit normal at its parameters `uv`, out of the material.
    #[must_use]
    pub fn face_normal(&self, f: usize, uv: [f64; 2]) -> Option<Vec3> {
        self.inner.face_normal(f, uv).map(record_vec3)
    }

    /// Whether `p` lies on face `f`: within `accept` mm of its surface, and inside its loops or
    /// within `accept` mm of one of them.
    #[must_use]
    pub fn face_contains(&self, f: usize, p: Vec3, accept: f64) -> bool {
        self.inner.face_contains(f, vec3(p), accept)
    }

    /// The faces `p` lies on, nearest first, each with how far `p` is from it.
    #[must_use]
    pub fn faces_at(&self, p: Vec3, accept: f64) -> Vec<(f64, usize)> {
        self.inner.faces_at(vec3(p), accept)
    }

    /// Where the ray from `o` along `d` meets the faces ahead of `o`: each meeting's distance,
    /// face and point, nearest first.
    ///
    /// # Errors
    /// The first face the ray cannot meet exactly (a torus, a spline, a revolution or a blend).
    pub fn ray(&self, o: Vec3, d: Vec3, accept: f64) -> Result<Vec<(f64, usize, Vec3)>, usize> {
        self.inner.ray(vec3(o), vec3(d), accept).map(|v| {
            v.into_iter()
                .map(|(s, f, q)| (s, f, record_vec3(q)))
                .collect()
        })
    }
}

#[cfg(test)]
mod tests;
