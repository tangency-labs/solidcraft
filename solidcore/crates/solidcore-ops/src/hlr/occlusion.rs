//! Which parts of a view's edges the exact faces hide, on the record's vectors: the kernel's
//! [`solidcore::hlr::occlusion`], converted at the boundary. Nothing here computes: every answer
//! is the kernel's, its numbers copied field by field.

use super::{kernel_placement, HlrError, Placement, Prepared};
use solidcore::hlr::{occlusion as k, Placement as KernelPlacement};
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::Vec3;

pub use k::EdgePieces;

/// Where the view is seen from, in the record's vectors.
#[derive(Clone, Copy, Debug)]
pub enum Eye {
    /// An orthographic view looking along this direction.
    Along(Vec3),
    /// A perspective view from this point.
    At(Vec3),
}

/// The faces of a view that can hide its edges ([`k::Occluders`], on the record's vectors).
pub struct Occluders<'a> {
    inner: k::Occluders<'a>,
}

impl<'a> Occluders<'a> {
    /// The faces of the placements `occludes` marks hide edges; every placement can be asked
    /// about. Placements must be rigid.
    ///
    /// # Errors
    /// [`HlrError`]: an invalid placement or view, a budget exceeded, cancelled.
    pub fn new(
        defs: &[&'a Prepared],
        scene: &[Placement],
        occludes: &[bool],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Occluders<'a>, HlrError> {
        let scene: Vec<KernelPlacement> = scene.iter().map(kernel_placement).collect();
        k::Occluders::new(defs, &scene, occludes, cancel).map(|inner| Occluders { inner })
    }

    /// Whether edge `edge` of placement `placement` is drawn in a view of the scene (seams
    /// between pieces of one surface are not), and its kind.
    #[must_use]
    pub fn edge_kind(&self, placement: usize, edge: usize) -> Option<super::EdgeKind> {
        self.inner.edge_kind(placement, edge)
    }

    /// The parameter range of an edge's curve.
    #[must_use]
    pub fn edge_domain(&self, placement: usize, edge: usize) -> Option<[f64; 2]> {
        self.inner.edge_domain(placement, edge)
    }

    /// The point of an edge at parameter `t`, in world coordinates.
    #[must_use]
    pub fn edge_point(&self, placement: usize, edge: usize, t: f64) -> Option<Vec3> {
        self.inner.edge_point(placement, edge, t).map(record_vec3)
    }

    /// Points along an edge's parameter range `r` in world coordinates, close enough that no
    /// chord between neighbours is farther than `tol(point)` from the curve at its middle.
    #[must_use]
    pub fn edge_points(
        &self,
        placement: usize,
        edge: usize,
        r: [f64; 2],
        tol: &dyn Fn(Vec3) -> f64,
    ) -> Vec<(f64, Vec3)> {
        self.inner
            .edge_points(placement, edge, r, &|p| tol(record_vec3(p)))
            .into_iter()
            .map(|(t, p)| (t, record_vec3(p)))
            .collect()
    }

    /// The unit normals (world, either side) of the first two faces an edge bounds, at its
    /// parameter `t`; a zero vector for a face it has not or whose surface does not evaluate there.
    #[must_use]
    pub fn edge_normals(&self, placement: usize, edge: usize, t: f64) -> [Vec3; 2] {
        self.inner.edge_normals(placement, edge, t).map(record_vec3)
    }

    /// The visible and hidden stretches of edge `edge` of placement `placement` seen from `eye`.
    /// Samples are at most `spacing(point)` apart; a change between samples is located to a
    /// sixteenth of that.
    ///
    /// # Errors
    /// [`HlrError`]: an invalid placement, edge or eye, cancelled.
    pub fn edge_pieces(
        &self,
        placement: usize,
        edge: usize,
        eye: Eye,
        spacing: &dyn Fn(Vec3) -> f64,
        cancel: &dyn Fn() -> bool,
    ) -> Result<EdgePieces, HlrError> {
        let eye = match eye {
            Eye::Along(d) => k::Eye::Along(vec3(d)),
            Eye::At(q) => k::Eye::At(vec3(q)),
        };
        self.inner
            .edge_pieces(placement, edge, eye, &|p| spacing(record_vec3(p)), cancel)
    }
}
