//! Native qualification of a two-source refined Form seam.
use super::{Definition, EdgeCurve, FittedSource, RefinedWeldSeam, convert_source, edge_curve};
use crate::kernel::history::{BodyHistory, EdgeOrigin, FaceOrigin};
use crate::{DocError, Document, FeatureId, FeatureKind, Result, kernel};
use solidcore_record::Vec3;
use solidcore_record::brep::{Curve, Shell};
use solidcraft_geom::bspline::{CurveN, rational_rim_gap, rational_rim_to_native_gap};
use solidcraft_geom::subdivision::hierarchy::FreeRimSide;
use std::collections::{BTreeMap, BTreeSet};

const MAP_TOLERANCE: f64 = 1e-8;
pub(super) const SEW_TOLERANCE: f64 = 1e-7;
const MAX_TOPOLOGY: usize = 65_536;
const MAX_CURVE_CONTROLS: usize = 512;
const WORK: usize = 8_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefinedWeldFace {
    pub source: FeatureId,
    pub face: usize,
}

/// Checked native result with source-scoped face owners in canonical order.
pub struct PreparedRefinedWeld {
    pub body: kernel::Body,
    pub face_owners: Vec<RefinedWeldFace>,
    operands: [kernel::Body; 2],
}

impl PreparedRefinedWeld {
    /// The exact inputs used by the checked history. A timeline consumer must
    /// match the earlier producer bodies and canonical record order before
    /// composing lineage from these operands.
    pub fn operands(&self) -> [&kernel::Body; 2] {
        self.operands.each_ref()
    }

    /// The timeline must also check that both bodies are the expected
    /// unchanged producers. This check binds the history indices to the
    /// actual native operands, including the canonical ordering.
    pub fn matches_operands(&self, operands: [&kernel::Body; 2]) -> bool {
        self.operands
            .iter()
            .zip(operands)
            .all(|(expected, actual)| !actual.is_mesh() && (expected.same(actual) || expected.record() == actual.record()))
    }
}

pub fn prepare_refined_weld(document: &Document, seam: &RefinedWeldSeam, fit_tolerances: [f64; 2]) -> Result<PreparedRefinedWeld> {
    let [a, b] = seam.sources();
    let pairs = seam.replay(document)?;
    let fit = |source: u64, index: usize| {
        let feature = document.feature(source).ok_or_else(|| invalid("refined Weld source Form is missing"))?;
        let FeatureKind::Form { definition, .. } = &feature.kind else {
            return Err(invalid("refined Weld requires retained source Forms"));
        };
        let edges: Vec<_> = pairs.iter().map(|pair| pair[index].edge).collect();
        fit_source(definition, fit_tolerances[index], &edges)
    };
    let inputs = [fit(a.0, 0)?, fit(b.0, 1)?];
    qualify([a, b], &pairs, inputs)
}

pub(super) struct FittedOperand {
    body: kernel::Body,
    owners: Vec<usize>,
    curves: Vec<EdgeCurve>,
}

fn fit_source(definition: &Definition, tolerance: f64, edges: &[[usize; 2]]) -> Result<FittedOperand> {
    let (body, report, source) = convert_source(definition, tolerance)?;
    fitted_operand(definition, body, report.patch_roots, &source, edges)
}

pub(super) fn fitted_operand(
    definition: &Definition,
    body: kernel::Body,
    owners: Vec<usize>,
    source: &FittedSource,
    edges: &[[usize; 2]],
) -> Result<FittedOperand> {
    let curves = edge_curve::fitted_edge_curves(definition, source, edges)?;
    Ok(FittedOperand { body, owners, curves })
}

pub(super) fn qualify(sources: [FeatureId; 2], pairs: &[[FreeRimSide; 2]], inputs: [FittedOperand; 2]) -> Result<PreparedRefinedWeld> {
    let [a, b] = &inputs;
    if a.curves.len() != pairs.len() || b.curves.len() != pairs.len() {
        return Err(invalid("refined Weld lost fitted side ownership"));
    }
    let mut work = WORK;
    let mut rims = [Rim::new(a, pairs.len())?, Rim::new(b, pairs.len())?];
    let mut expected = BTreeMap::new();
    for (index, [first, second]) in pairs.iter().enumerate() {
        let first_curve = a.curves.get(index).ok_or_else(|| invalid("missing first refined Weld rim restriction"))?;
        let second_curve = b.curves.get(index).ok_or_else(|| invalid("missing second refined Weld rim restriction"))?;
        let curves = [first_curve.rational_pieces_bounded(&mut work)?, second_curve.rational_pieces_bounded(&mut work)?];
        let directions = [reversed(first, first_curve)?, reversed(second, second_curve)?];
        if rational_rim_gap(&curves[0], &curves[1], [directions[0], !directions[1]], MAP_TOLERANCE, &mut work).map_err(invalid)?.is_none() {
            return Err(invalid("refined Weld rim curves do not coincide under the selected winding"));
        }
        let [left, right] = &mut rims;
        let x = left.side(first, first_curve, &curves[0], directions[0], &mut work)?;
        let y = right.side(second, second_curve, &curves[1], directions[1], &mut work)?;
        expected.insert(x, y);
    }
    for rim in &rims {
        rim.check_cycle()?;
    }
    let body = kernel::stitch(&[&a.body, &b.body], SEW_TOLERANCE)?;
    let face_owners = check_history(&body, sources, [&a.body, &b.body], [&a.owners, &b.owners], &expected)?;
    let validity = kernel::validity(&body);
    if !validity.ok() {
        return Err(invalid(format!("refined Weld failed native validity: {validity:?}")));
    }
    Ok(PreparedRefinedWeld { body, face_owners, operands: inputs.map(|input| input.body) })
}

fn reversed(side: &FreeRimSide, curve: &EdgeCurve) -> Result<bool> {
    let [a, b] = curve.edge;
    if side.edge == [a, b] {
        Ok(false)
    } else if side.edge == [b, a] {
        Ok(true)
    } else {
        Err(invalid("refined Weld side no longer names its fitted curve"))
    }
}

struct Rim<'a> {
    operand: &'a FittedOperand,
    free: Vec<usize>,
    used: BTreeSet<usize>,
    next: BTreeMap<u32, u32>,
}

impl<'a> Rim<'a> {
    fn new(operand: &'a FittedOperand, side_count: usize) -> Result<Self> {
        let body = &operand.body;
        if body.is_mesh()
            || !body.is_surface()
            || !matches!(body.record().shells.as_slice(), [Shell::Faces { wires, .. }] if wires.is_empty())
            || body.face_count() != operand.owners.len()
            || body.face_count() > MAX_TOPOLOGY
            || body.edge_count() > MAX_TOPOLOGY
            || body.record().vertices.len() > MAX_TOPOLOGY
            || !(3..=256).contains(&side_count)
        {
            return Err(invalid("refined Weld requires bounded, independently fitted single-shell skins"));
        }
        let mut free = Vec::with_capacity(side_count);
        for (id, edge) in body.record().edges.iter().enumerate() {
            match edge.uses.len() {
                1 if free.len() < side_count => free.push(id),
                1 => return Err(invalid("refined Weld requires exactly one native coedge per side and no other free edges")),
                2 => {}
                _ => return Err(invalid("refined Weld source has nonmanifold native edges")),
            }
        }
        if free.len() != side_count {
            return Err(invalid("refined Weld requires exactly one native coedge per side and no other free edges"));
        }
        Ok(Self { operand, free, used: BTreeSet::new(), next: BTreeMap::new() })
    }

    fn side(
        &mut self,
        side: &FreeRimSide,
        curve: &EdgeCurve,
        fitted: &[([f64; 2], CurveN<4>)],
        side_reversed: bool,
        work: &mut usize,
    ) -> Result<usize> {
        let targets = [curve.at(0.0)?, curve.at(1.0)?];
        let record = self.operand.body.record();
        let point = |vertex| {
            let p = record.vertices.get(vertex as usize).ok_or_else(|| invalid("missing native Weld vertex"))?.point;
            Ok::<_, DocError>(Vec3::new(p[0], p[1], p[2]))
        };
        let mut found = None;
        for &id in &self.free {
            let edge = record.edges.get(id).ok_or_else(|| invalid("missing native Weld edge"))?;
            let [face_use] = edge.uses.as_slice() else { return Err(invalid("refined Weld edge is no longer free")) };
            if self.operand.owners.get(face_use.face as usize) != Some(&side.owner) {
                continue;
            }
            let [a, b] = edge.vertices;
            let ends = [point(a)?, point(b)?];
            let [forward, backward] = kernel::form_curves::endpoint_directions(ends, targets, MAP_TOLERANCE);
            if !forward && !backward {
                continue;
            }
            if forward && backward {
                return Err(invalid("refined Weld cannot distinguish the native coedge endpoints"));
            }
            let native = native_curve(edge.parameters(), edge.interval)?;
            if rational_rim_to_native_gap(fitted, &native, [false, backward], MAP_TOLERANCE, work).map_err(invalid)?.is_none() {
                continue;
            }
            let vertices = if backward != side_reversed { [b, a] } else { [a, b] };
            if found.replace((id, vertices)).is_some() {
                return Err(invalid("refined Weld side has ambiguous native coedge coverage"));
            }
        }
        let (id, [a, b]) = found.ok_or_else(|| invalid("refined Weld side does not cover one complete native free coedge"))?;
        if !self.used.insert(id) || self.next.insert(a, b).is_some() {
            return Err(invalid("refined Weld repeated a native rim edge or vertex"));
        }
        Ok(id)
    }

    fn check_cycle(&self) -> Result<()> {
        if self.used.len() != self.free.len() {
            return Err(invalid("refined Weld does not cover the complete free native rim"));
        }
        let (&start, _) = self.next.first_key_value().ok_or_else(|| invalid("missing native Weld rim cycle"))?;
        let mut at = start;
        for count in 1..=self.next.len() {
            at = *self.next.get(&at).ok_or_else(|| invalid("native Weld rim is open"))?;
            if at == start {
                return if count == self.next.len() { Ok(()) } else { Err(invalid("refined Weld source has another free rim")) };
            }
        }
        Err(invalid("native Weld rim does not close"))
    }
}

fn native_curve(curve: &Curve, interval: [f64; 2]) -> Result<CurveN<4>> {
    if interval != [0.0, 1.0] {
        return Err(invalid("refined Weld requires unit native coedge parameters"));
    }
    let (knots, ctrl) = match curve {
        Curve::Line { start: [x, y, z], end: [a, b, c] } => (vec![0., 0., 1., 1.], vec![[*x, *y, *z, 1.], [*a, *b, *c, 1.]]),
        Curve::Spline { knots, points } if bounded_curve(knots.len(), points.len()) => {
            (knots.clone(), points.iter().map(|&[x, y, z]| [x, y, z, 1.]).collect())
        }
        Curve::RationalSpline { knots, homogeneous } if bounded_curve(knots.len(), homogeneous.len()) => (knots.clone(), homogeneous.clone()),
        _ => return Err(invalid("refined Weld requires bounded native spline coedges")),
    };
    let degree = knots.len().checked_sub(ctrl.len() + 1).ok_or_else(|| invalid("invalid native Weld coedge degree"))?;
    Ok(CurveN { degree, knots, ctrl })
}

fn bounded_curve(knots: usize, controls: usize) -> bool {
    (2..=MAX_CURVE_CONTROLS).contains(&controls) && knots > controls + 1 && knots <= controls + 17
}

fn check_history(
    body: &kernel::Body,
    sources: [FeatureId; 2],
    operands: [&kernel::Body; 2],
    owners: [&[usize]; 2],
    expected: &BTreeMap<usize, usize>,
) -> Result<Vec<RefinedWeldFace>> {
    let [Shell::Faces { faces: shell, wires }] = body.record().shells.as_slice() else {
        return Err(invalid("refined Weld did not produce one closed native shell"));
    };
    if body.is_mesh()
        || body.is_surface()
        || !wires.is_empty()
        || body.face_count() > 2 * MAX_TOPOLOGY
        || body.edge_count() > 2 * MAX_TOPOLOGY
        || body.record().vertices.len() > 2 * MAX_TOPOLOGY
        || shell.len() != body.face_count()
    {
        return Err(invalid("refined Weld did not produce one bounded closed native shell"));
    }
    let mut senses = vec![None; body.face_count()];
    for face in shell {
        let sense = senses.get_mut(face.face as usize).ok_or_else(|| invalid("missing sewn Weld shell face"))?;
        if sense.replace(face.forward).is_some() {
            return Err(invalid("sewn Weld shell repeated a face"));
        }
    }
    let history = body.history().ok_or_else(|| invalid("refined Weld sewing has no exact topology history"))?;
    history.check(body, &operands)?;
    let mut faces = BTreeSet::new();
    let mut output_sources = Vec::with_capacity(body.face_count());
    let face_owners = history
        .faces
        .iter()
        .map(|provenance| {
            let [FaceOrigin::Face { operand, face }] = provenance.origins.as_slice() else {
                return Err(invalid("refined Weld sewing did not retain one exact source face"));
            };
            let source = *sources.get(*operand).ok_or_else(|| invalid("refined Weld sewing names a missing face operand"))?;
            let logical =
                *owners.get(*operand).and_then(|owner| owner.get(*face)).ok_or_else(|| invalid("refined Weld sewing lost logical face ownership"))?;
            if !faces.insert((*operand, *face)) {
                return Err(invalid("refined Weld sewing split one of the source faces"));
            }
            output_sources.push((*operand, *face));
            Ok(RefinedWeldFace { source, face: logical })
        })
        .collect::<Result<Vec<_>>>()?;
    if faces.len() != operands.iter().map(|body| body.face_count()).sum::<usize>() {
        return Err(invalid("refined Weld sewing lost a source face"));
    }
    let second: BTreeSet<_> = expected.values().copied().collect();
    let mut joined = BTreeSet::new();
    let mut seen = BTreeSet::new();
    for (id, provenance) in history.edges.iter().enumerate() {
        let output = body.record().edges.get(id).ok_or_else(|| invalid("missing sewn Weld edge"))?;
        let [a, b] = output.uses.as_slice() else { return Err(invalid("sewn Weld edge is not manifold")) };
        let side = |face: u32, upper: bool| {
            let source = *output_sources.get(face as usize).ok_or_else(|| invalid("missing sewn Weld edge face"))?;
            let sense = senses.get(face as usize).copied().flatten().ok_or_else(|| invalid("missing sewn Weld coedge orientation"))?;
            Ok::<_, DocError>((source, upper ^ sense))
        };
        let (a, first_forward) = side(a.face, a.upper)?;
        let (b, second_forward) = side(b.face, b.upper)?;
        if first_forward == second_forward {
            return Err(invalid("sewn Weld coedges do not traverse in opposite directions"));
        }
        let source_uses = |operand: usize, edge: usize| {
            operands
                .get(operand)
                .and_then(|body| body.record().edges.get(edge))
                .map(|edge| edge.uses.as_slice())
                .ok_or_else(|| invalid("sewn Weld lost the source edge uses"))
        };
        let connects = |x, y| (a, b) == (x, y) || (a, b) == (y, x);
        match provenance.origins.as_slice() {
            [EdgeOrigin::Edge { operand, edge }] => {
                let selected = match operand {
                    0 => expected.contains_key(edge),
                    1 => second.contains(edge),
                    _ => return Err(invalid("refined Weld sewing names a missing edge operand")),
                };
                let [x, y] = source_uses(*operand, *edge)? else { return Err(invalid("retained Weld edge was not interior")) };
                if selected || !connects((*operand, x.face as usize), (*operand, y.face as usize)) || !seen.insert((*operand, *edge)) {
                    return Err(invalid("refined Weld sewing changed a retained or selected source edge"));
                }
            }
            [EdgeOrigin::Edge { operand: 0, edge: x }, EdgeOrigin::Edge { operand: 1, edge: y }]
            | [EdgeOrigin::Edge { operand: 1, edge: y }, EdgeOrigin::Edge { operand: 0, edge: x }] => {
                let ([left], [right]) = (source_uses(0, *x)?, source_uses(1, *y)?) else {
                    return Err(invalid("selected Weld source edges were not both free"));
                };
                if !connects((0, left.face as usize), (1, right.face as usize))
                    || expected.get(x) != Some(y)
                    || !joined.insert(*x)
                    || !seen.insert((0, *x))
                    || !seen.insert((1, *y))
                {
                    return Err(invalid("refined Weld sewing joined an unexpected native edge pair"));
                }
            }
            _ => return Err(invalid("refined Weld sewing introduced or merged unqualified edges")),
        }
    }
    if joined.len() != expected.len() || seen.len() != operands.iter().map(|body| body.edge_count()).sum::<usize>() {
        return Err(invalid("refined Weld sewing lost a source edge or a selected seam pair"));
    }
    Ok(face_owners)
}

fn invalid(why: impl std::fmt::Display) -> DocError {
    DocError::Invalid(why.to_string())
}
