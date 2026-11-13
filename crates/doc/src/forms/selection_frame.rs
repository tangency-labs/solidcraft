//! Local axes from the accepted Form fitting charts and logical guide routes.
use super::*;
use solidcore_record::Vec3;
use solidcraft_geom::subdivision::{fitting::PatchChart, hierarchy::FaceFrameSite};
use std::collections::BTreeMap;

mod native;

const MAX_CHARTS: usize = 65_536;
const WORK: usize = 16_000_000;
const SAME_DIRECTION: f64 = 0.9999;

#[derive(Default)]
pub struct SelectionFrames {
    pub faces: BTreeMap<usize, [Vec3; 3]>,
    pub edges: BTreeMap<[usize; 2], [Vec3; 3]>,
    pub points: BTreeMap<usize, [Vec3; 3]>,
}
pub(super) struct EdgeSample {
    pub chart: usize,
    pub point: Vec3,
    pub tangent: Vec3,
    pub normal: Vec3,
}

pub(super) fn spend(remaining: &mut usize, work: usize) -> Option<()> {
    *remaining = remaining.checked_sub(work)?;
    Some(())
}
fn unit(v: Vec3) -> Option<Vec3> {
    let scale = v.x.abs().max(v.y.abs()).max(v.z.abs());
    (scale > 0.0 && scale.is_finite()).then_some(())?;
    (v / scale).normalized().filter(|v| v.is_finite())
}
pub(super) fn normal(du: Vec3, dv: Vec3) -> Option<Vec3> {
    let cross = unit(du)?.cross(unit(dv)?);
    (cross.len2() > 1e-16).then_some(())?;
    unit(cross)
}
fn axes(x: Vec3, z: Vec3) -> Option<[Vec3; 3]> {
    let x = unit(x)?;
    let z = unit(z)?;
    let tangent = x - z * x.dot(z);
    (tangent.len2() > 1e-16).then_some(())?;
    let x = unit(tangent)?;
    Some([x, unit(z.cross(x))?, z])
}
fn bisector(normals: &BTreeMap<usize, Vec3>) -> Option<Vec3> {
    let z = unit(normals.values().copied().fold(Vec3::ZERO, |sum, n| sum + n))?;
    normals.values().all(|n| n.dot(z) > 1e-6).then_some(z)
}
fn insert_normal(normals: &mut BTreeMap<usize, Vec3>, sample: &EdgeSample) -> Option<()> {
    let first = normals.entry(sample.chart).or_insert(sample.normal);
    (first.dot(sample.normal) >= SAME_DIRECTION).then_some(())
}
fn edge_samples(samples: &[EdgeSample]) -> Option<(Vec3, BTreeMap<usize, Vec3>)> {
    let first = samples.first()?;
    let x = unit(first.tangent)?;
    let mut normals = BTreeMap::new();
    for sample in samples {
        if first.point.dist(sample.point) > 1e-7 || unit(sample.tangent)?.dot(x) < SAME_DIRECTION {
            return None;
        }
        insert_normal(&mut normals, sample)?;
    }
    Some((x, normals))
}

#[derive(Default)]
struct PointFrame {
    point: Option<Vec3>,
    outgoing: Vec<Vec3>,
    normals: BTreeMap<usize, Vec3>,
    incomplete: bool,
}
impl PointFrame {
    fn add(&mut self, samples: Option<&[EdgeSample]>, reverse: bool) -> Option<()> {
        let samples = samples?;
        let (x, _) = edge_samples(samples)?;
        let point = samples.first()?.point;
        if self.outgoing.len() >= 64 || self.point.is_some_and(|first| first.dist(point) > 1e-7) {
            return None;
        }
        self.point = Some(point);
        self.outgoing.push(if reverse { -x } else { x });
        for sample in samples {
            insert_normal(&mut self.normals, sample)?;
        }
        Some(())
    }
    fn finish(self) -> Option<[Vec3; 3]> {
        if self.incomplete {
            return None;
        }
        let z = bisector(&self.normals)?;
        self.outgoing.into_iter().find_map(|x| axes(x, z))
    }
}

impl SelectionFrames {
    /// Native Form Surface guides and partition cells use their accepted B-rep
    /// stations. Unmapped guides and unowned material faces supply no frame.
    pub fn from_profile(definition: &Definition, body: &crate::kernel::Body, owners: Option<&[usize]>, guides: &SelectionGuides) -> Self {
        native::frames(definition, body, owners, guides)
    }

    pub(super) fn from_faces(definition: &Definition, source: &FittedSource) -> Self {
        let mut out = Self::default();
        let fit = &source.fit;
        if fit.patches.len() > MAX_CHARTS || fit.charts.len() != fit.patches.len() {
            return out;
        }
        let mut charts = BTreeMap::<_, Vec<_>>::new();
        for (chart, surface) in fit.charts.iter().zip(&fit.patches) {
            charts.entry(chart.face).or_default().push((chart, surface));
        }
        let sites = if let Some(hierarchy) = &definition.hierarchy {
            hierarchy.face_frame_sites(source.trimmed_roots).unwrap_or_default()
        } else if definition.cage.faces.iter().all(|face| face.len() == 4) {
            (0..definition.cage.faces.len()).map(|owner| FaceFrameSite { owner, chart: owner, uv: [0.5, 0.5], along_u: [1.0, 0.0] }).collect()
        } else {
            Vec::new()
        };
        let mut remaining = WORK;
        for site in sites {
            let Some(patches) = charts.get(&site.chart) else { continue };
            if spend(&mut remaining, patches.len()).is_none() {
                break;
            }
            let mut containing = patches.iter().filter(|(chart, _)| {
                (0..2)
                    .all(|d| site.uv[d] >= chart.domain[d] && (site.uv[d] < chart.domain[d + 2] || (site.uv[d] == 1.0 && chart.domain[d + 2] == 1.0)))
            });
            if let Some(&(chart, surface)) = containing.next()
                && containing.next().is_none()
                && let Some(frame) = face_axes(chart, surface, site.uv, site.along_u, &mut remaining)
            {
                out.faces.insert(site.owner, frame);
            }
        }
        if let Some(hierarchy) = definition.hierarchy.as_ref().filter(|h| h.has_trim_wall())
            && let Some(surface) = fit.patches.last()
            && let Some(frame) = face_axes(&PatchChart::whole(hierarchy.leaves.len()), surface, [0.5, 0.5], [1.0, 0.0], &mut remaining)
        {
            out.faces.insert(hierarchy.leaves.len(), frame);
        }
        out
    }

    pub(super) fn add_guides(&mut self, definition: &Definition, guides: &SelectionGuides) {
        let SelectionGuides::Subdivision(curves) = guides else { return };
        if curves.len() > MAX_CHARTS {
            return;
        }
        let attachment = definition.hierarchy.as_ref().and_then(|h| h.validated_trim_wall().ok().flatten()).map(|wall| {
            let [a, b] = wall.attachment().edge;
            [a.min(b), a.max(b)]
        });
        let mut points = BTreeMap::<_, PointFrame>::new();
        let mut remaining = WORK;
        // One persistent order for the outgoing reference at each point.
        let mut ordered: Vec<_> = curves.iter().collect();
        ordered.sort_by_key(|curve| curve.edge);
        for curve in ordered {
            if attachment != Some(curve.edge)
                && let Ok(samples) = curve.frame_samples(0.5, &mut remaining)
                && let Some((x, normals)) = edge_samples(&samples)
                && let Some(frame) = bisector(&normals).and_then(|z| axes(x, z))
            {
                self.edges.insert(curve.edge, frame);
            }
            for (index, id) in curve.edge.into_iter().enumerate() {
                let corner = points.entry(id).or_default();
                let samples = curve.frame_samples(index as f64, &mut remaining).ok();
                if corner.add(samples.as_deref(), index == 1).is_none() {
                    corner.incomplete = true;
                }
            }
        }
        self.points = points.into_iter().filter_map(|(id, corner)| corner.finish().map(|frame| (id, frame))).collect();
    }
}

fn face_axes(
    chart: &PatchChart,
    surface: &solidcraft_geom::bspline::NurbsSurface,
    uv: [f64; 2],
    along_u: [f64; 2],
    remaining: &mut usize,
) -> Option<[Vec3; 3]> {
    let [a, b] = surface.degree();
    spend(remaining, surface.knots().iter().map(Vec::len).sum::<usize>() + 4 * (a + 1) * (b + 1))?;
    let domain = surface.domain();
    let mut at = [0.0; 2];
    let mut direction = [0.0; 2];
    for d in 0..2 {
        let width = chart.domain[d + 2] - chart.domain[d];
        let s = ((uv[d] - chart.domain[d]) / width).clamp(0.0, 1.0);
        let [lo, hi] = domain[d];
        at[d] = (lo * (1.0 - s) + hi * s).clamp(lo, hi);
        direction[d] = along_u[d] * (hi - lo) / width;
    }
    let (_, du, dv) = surface.derivs(at)?;
    axes(du * direction[0] + dv * direction[1], normal(du, dv)?)
}
