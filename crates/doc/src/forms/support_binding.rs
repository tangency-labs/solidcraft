//! Retain a support wire's logical seam and direction independently of native face UV order.
use super::{BoundarySupport, invalid};
use crate::forms::association::{NamedEdge, SupportRef};
use crate::{ModelBody, ModelState, Result};
use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundCurve {
    edge: NamedEdge,
    /// Incident named edges identify the endpoint even after the body moves or resizes.
    start: Vec<String>,
    end: Vec<String>,
    /// A periodic edge has one vertex: retain its own normalized source-curve orientation.
    forward: bool,
    /// Closed endpoints do not identify an intrinsic parameter direction. Certify that
    /// direction from the complete rational parameterization in the source definition frame.
    intrinsic: Option<IntrinsicCurve>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IntrinsicCurve {
    kind: String,
    degree: usize,
    /// Edge parameters normalized in the shape's complete knot domain. A native
    /// spline's knot scale can follow its size while its normalized seam stays fixed.
    /// The full shape alone does not encode a trimmed spline/polyline interval.
    interval: [f64; 2],
    knots: Vec<f64>,
    weights: Vec<f64>,
    points: Vec<Vec3>,
}
impl IntrinsicCurve {
    fn validate(&self) -> Result<()> {
        if self.kind.is_empty()
            || self.kind.len() > 64
            || !(1..=16).contains(&self.degree)
            || self.interval.iter().any(|v| !v.is_finite())
            || self.interval[1] <= self.interval[0]
            || !(2..=256).contains(&self.points.len())
            || self.weights.len() != self.points.len()
            || self.knots.len() != self.points.len() + self.degree + 1
            || self.knots.iter().any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
            || self.knots.windows(2).any(|w| w[0] > w[1])
            || self.weights.iter().any(|x| !x.is_finite() || *x <= 0.0)
            || self.points.iter().any(|p| !p.is_finite() || p.x.abs().max(p.y.abs()).max(p.z.abs()) > 100_000.0)
        {
            return Err(invalid("invalid intrinsic support parameterization certificate"));
        }
        Ok(())
    }
    fn capture(body: &ModelBody, curve: &crate::kernel::CurveGeometry, index: usize) -> Result<Self> {
        let controls = curve
            .control_points()
            .filter(|c| (2..=256).contains(&c.len()))
            .ok_or_else(|| invalid("periodic support requires a bounded exact rational parameterization"))?;
        let inverse = crate::mat_inverse(&body.source_transform).ok_or_else(|| invalid("support source transform is singular"))?;
        let first = curve.curve.knots.first().copied().ok_or_else(|| invalid("missing intrinsic knots"))?;
        let last = curve.curve.knots.last().copied().ok_or_else(|| invalid("missing intrinsic knots"))?;
        if !first.is_finite() || !last.is_finite() || last <= first {
            return Err(invalid("invalid intrinsic knot interval"));
        }
        let weight = controls.first().map(|c| c[3]).filter(|w| w.is_finite() && *w > 0.0).ok_or_else(|| invalid("invalid intrinsic curve weight"))?;
        let source = body.body.record().edges.get(index).ok_or_else(|| invalid("missing intrinsic source edge"))?;
        let value = serde_json::to_value(source.parameters()).map_err(|e| invalid(e.to_string()))?;
        let kind = value.get("kind").and_then(serde_json::Value::as_str).ok_or_else(|| invalid("missing intrinsic source curve kind"))?.to_owned();
        let points = controls
            .iter()
            .map(|c| {
                if !c[3].is_finite() || c[3] <= 0.0 {
                    return Err(invalid("invalid intrinsic rational control"));
                }
                Ok(crate::apply_point(&inverse, Vec3::new(c[0] / c[3], c[1] / c[3], c[2] / c[3])))
            })
            .collect::<Result<Vec<_>>>()?;
        let out = Self {
            kind,
            degree: curve.curve.degree,
            interval: source.interval.map(|t| (t - first) / (last - first)),
            knots: curve.curve.knots.iter().map(|x| (*x - first) / (last - first)).collect(),
            weights: controls.iter().map(|c| c[3] / weight).collect(),
            points,
        };
        out.validate()?;
        Ok(out)
    }
    fn follows(&self, current: &Self) -> bool {
        if self.kind != current.kind
            || self.degree != current.degree
            || self.interval != current.interval
            || self.points.len() != current.points.len()
            || !normalized_knots_follow(&self.knots, &current.knots)
            || self.weights != current.weights
        {
            return false;
        }
        let n = self.points.len() as f64;
        let before = self.points.iter().copied().fold(Vec3::ZERO, |a, p| a + p) / n;
        let after = current.points.iter().copied().fold(Vec3::ZERO, |a, p| a + p) / n;
        let old_size = self.points.iter().map(|p| (*p - before).len2()).sum::<f64>();
        let new_size = current.points.iter().map(|p| (*p - after).len2()).sum::<f64>();
        let scale = (new_size / old_size).sqrt();
        if !scale.is_finite() || !(1e-9..=1e9).contains(&scale) {
            return false;
        }
        let extent = (new_size / n).sqrt();
        let position = current.points.iter().map(|p| p.x.abs().max(p.y.abs()).max(p.z.abs())).fold(0.0_f64, f64::max);
        let tolerance = extent * 1e-8 + position * 64.0 * f64::EPSILON;
        if !tolerance.is_finite() || tolerance >= extent * 0.01 {
            return false;
        }
        self.points.iter().zip(&current.points).all(|(a, b)| (after + (*a - before) * scale).dist(*b) <= tolerance)
    }
}
/// Knot normalization subtracts and divides native size-dependent parameters.
/// Preserve their order and exact multiplicities while allowing only a bounded
/// floating-point rounding change. This is independent of the CAD fit tolerance.
fn normalized_knots_follow(saved: &[f64], current: &[f64]) -> bool {
    saved.len() == current.len()
        && saved.iter().zip(current).all(|(&a, &b)| {
            a.is_finite()
                && b.is_finite()
                && (0.0..=1.0).contains(&a)
                && (0.0..=1.0).contains(&b)
                && (a == b || (a > 0.0 && a < 1.0 && b > 0.0 && b < 1.0 && a.to_bits().abs_diff(b.to_bits()) <= 8))
        })
        && saved.windows(2).zip(current.windows(2)).all(|(a, b)| a[0].total_cmp(&a[1]) == b[0].total_cmp(&b[1]))
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryBinding {
    curves: Vec<BoundCurve>,
}
fn names_valid(names: &[String]) -> bool {
    !names.is_empty()
        && names.len() <= 64
        && names.windows(2).all(|w| w[0] < w[1])
        && names.iter().all(|name| !name.is_empty() && name.len() <= 4096 && !name.contains('\0'))
}
impl BoundaryBinding {
    /// The same ordered exact boundary identities and parameterization, ignoring
    /// world-space lookup hints. Hints move with an associated source; they do not
    /// turn an ordinary source recompute into a new attachment.
    pub fn same_attachment(&self, other: &Self) -> bool {
        self.curves.len() == other.curves.len()
            && self.curves.iter().zip(&other.curves).all(|(a, b)| {
                a.edge.name == b.edge.name && a.start == b.start && a.end == b.end && a.forward == b.forward && a.intrinsic == b.intrinsic
            })
    }
    pub fn validate(&self) -> Result<()> {
        if self.curves.is_empty() || self.curves.len() > 1024 {
            return Err(invalid("support curve identity budget exceeded"));
        }
        let name_bytes = self
            .curves
            .iter()
            .flat_map(|c| std::iter::once(&c.edge.name).chain(&c.start).chain(&c.end))
            .try_fold(0usize, |total, name| total.checked_add(name.len()));
        if name_bytes.is_none_or(|bytes| bytes > 1_048_576) {
            return Err(invalid("support identity text budget exceeded"));
        }
        if self.curves.iter().map(|c| &c.edge.name).collect::<BTreeSet<_>>().len() != self.curves.len()
            || self.curves.iter().any(|c| {
                !names_valid(&c.start)
                    || !names_valid(&c.end)
                    || c.edge.name.is_empty()
                    || c.edge.name.len() > 4096
                    || c.edge.name.contains('\0')
                    || !c.edge.pick.is_finite()
                    || c.edge.pick.x.abs().max(c.edge.pick.y.abs()).max(c.edge.pick.z.abs()) > 100_000.0
            })
        {
            return Err(invalid("invalid named support boundary binding"));
        }
        if self.curves.iter().filter_map(|c| c.intrinsic.as_ref()).map(|c| c.points.len()).sum::<usize>() > 4096 {
            return Err(invalid("intrinsic support certificate control budget exceeded"));
        }
        for curve in &self.curves {
            if (curve.start == curve.end) != curve.intrinsic.is_some() {
                return Err(invalid("missing periodic support parameterization certificate"));
            }
            if let Some(intrinsic) = &curve.intrinsic {
                intrinsic.validate()?;
            }
        }
        Ok(())
    }
    pub fn transform_picks(&mut self, matrix: &crate::Mat) {
        for curve in &mut self.curves {
            curve.edge.pick = crate::apply_point(matrix, curve.edge.pick);
        }
    }
    pub(crate) fn remap_names(&mut self, features: &BTreeMap<u64, u64>, bodies: &BTreeMap<String, String>) {
        for curve in &mut self.curves {
            curve.edge.name = crate::remap_topology(&curve.edge.name, features, bodies);
            for name in curve.start.iter_mut().chain(&mut curve.end) {
                *name = crate::remap_topology(name, features, bodies);
            }
            curve.start.sort();
            curve.end.sort();
        }
    }
    pub(crate) fn capture(body: &ModelBody, support: &BoundarySupport) -> Result<Self> {
        // Existing topological names are the same contract ordinary named edge references use.
        // Curve positions never choose which source edge a persisted binding follows.
        let record = body.body.record();
        if record.vertices.len() > 100_000 || record.edges.len() > 100_000 {
            return Err(invalid("support edge identity budget exceeded"));
        }
        let names = crate::naming::edge_names(body);
        if names.len() != record.edges.len() {
            return Err(invalid("support names disagree with exact edge identities"));
        }
        // Keep indices while visiting the body: cloning long names into every
        // incident vertex would multiply untrusted record storage before validation.
        let mut vertices: Vec<Vec<usize>> = vec![Vec::new(); record.vertices.len()];
        for (index, edge) in record.edges.iter().enumerate() {
            for &vertex in &edge.vertices {
                let key = vertices.get_mut(vertex as usize).ok_or_else(|| invalid("support edge has no endpoint"))?;
                if !key.contains(&index) {
                    if key.len() >= 64 {
                        return Err(invalid("support endpoint identity budget exceeded"));
                    }
                    key.push(index);
                }
            }
        }
        let mut identity_bytes = 0usize;
        let mut endpoint_key = |vertex: u32| -> Result<Vec<String>> {
            let indices = vertices.get(vertex as usize).ok_or_else(|| invalid("missing support endpoint identity"))?;
            let mut key = Vec::with_capacity(indices.len());
            for &index in indices {
                let name = names.get(index).ok_or_else(|| invalid("missing support endpoint name"))?;
                identity_bytes = identity_bytes.checked_add(name.len()).ok_or_else(|| invalid("support identity text budget overflow"))?;
                if name.len() > 4096 || identity_bytes > 1_048_576 {
                    return Err(invalid("support identity text budget exceeded"));
                }
                key.push(name.clone());
            }
            key.sort();
            Ok(key)
        };
        let curves = support
            .indices
            .iter()
            .zip(&support.curves)
            .map(|(&index, curve)| {
                let edge = record.edges.get(index).ok_or_else(|| invalid("missing exact bound edge"))?;
                let forward = if curve.range == edge.interval {
                    true
                } else if curve.range == [edge.interval[1], edge.interval[0]] {
                    false
                } else {
                    return Err(invalid("support curve interval lost its edge orientation"));
                };
                let start = edge.vertices[usize::from(!forward)];
                let end = edge.vertices[usize::from(forward)];
                let start_key = endpoint_key(start)?;
                let end_key = endpoint_key(end)?;
                let intrinsic = if start_key == end_key { Some(IntrinsicCurve::capture(body, curve, index)?) } else { None };
                Ok(BoundCurve {
                    edge: NamedEdge {
                        name: names.get(index).cloned().ok_or_else(|| invalid("missing support edge name"))?,
                        pick: curve.at(0.5).ok_or_else(|| invalid("support edge cannot be evaluated"))?.0,
                    },
                    start: start_key,
                    end: end_key,
                    forward,
                    intrinsic,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let out = Self { curves };
        out.validate()?;
        Ok(out)
    }
    fn apply(&self, current: &Self, support: &BoundarySupport) -> Result<BoundarySupport> {
        self.validate()?;
        current.validate()?;
        if self.curves.len() != current.curves.len() {
            return Err(invalid("named support boundary changed its curve count; reattach the support"));
        }
        let order = self
            .curves
            .iter()
            .map(|saved| {
                let index = current
                    .curves
                    .iter()
                    .position(|now| now.edge.name == saved.edge.name)
                    .ok_or_else(|| invalid(format!("named support edge `{}` is missing; reattach the support", saved.edge.name)))?;
                let now = current.curves.get(index).ok_or_else(|| invalid("missing current support identity"))?;
                let reverse = if saved.start != saved.end {
                    if saved.start == now.start && saved.end == now.end {
                        false
                    } else if saved.start == now.end && saved.end == now.start {
                        true
                    } else {
                        return Err(invalid("named support endpoint topology changed; reattach the support"));
                    }
                } else {
                    if now.start != saved.start || now.end != saved.end {
                        return Err(invalid("periodic support endpoint topology changed; reattach the support"));
                    }
                    if !saved.intrinsic.as_ref().zip(now.intrinsic.as_ref()).is_some_and(|(old, new)| old.follows(new)) {
                        return Err(invalid("periodic support intrinsic parameterization changed; reattach the support"));
                    }
                    saved.forward != now.forward
                };
                Ok((index, reverse))
            })
            .collect::<Result<Vec<_>>>()?;
        support.reordered(&order)
    }
}
impl SupportRef {
    pub fn resolve_bound(
        &self,
        state: &ModelState,
        adjacent_faces: bool,
        binding: Option<&BoundaryBinding>,
    ) -> Result<(BoundarySupport, BoundaryBinding)> {
        let support = self.resolve_with(state, adjacent_faces)?;
        let body = state.body(self.body()).ok_or_else(|| invalid("named support body is missing"))?;
        let current = BoundaryBinding::capture(body, &support)?;
        match binding {
            Some(binding) => Ok((binding.apply(&current, &support)?, binding.clone())),
            None => Ok((support, current)),
        }
    }
    pub(crate) fn bind_support(&self, state: &ModelState, support: &BoundarySupport) -> Result<BoundaryBinding> {
        BoundaryBinding::capture(state.body(self.body()).ok_or_else(|| invalid("named support body is missing"))?, support)
    }
}
impl crate::walk::Walk for BoundaryBinding {
    fn walk_refs<'a>(&'a self, _f: &mut dyn FnMut(crate::walk::RefItem<'a>)) {}
    fn walk_refs_mut<'a>(&'a mut self, _f: &mut dyn FnMut(crate::walk::RefItemMut<'a>)) {}
    fn walk_geometry<'a>(&'a self, f: &mut dyn FnMut(crate::walk::GeoItem<'a>)) {
        for curve in &self.curves {
            f(crate::walk::Geo::Pick(&curve.edge.pick));
        }
    }
    fn walk_geometry_mut<'a>(&'a mut self, f: &mut dyn FnMut(crate::walk::GeoItemMut<'a>)) {
        for curve in &mut self.curves {
            f(crate::walk::Geo::Pick(&mut curve.edge.pick));
        }
    }
}
