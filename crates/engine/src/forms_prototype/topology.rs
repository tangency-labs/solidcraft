//! Topology operations that intentionally materialize a local evaluator do so explicitly.
use super::*;
use solidcraft_geom::subdivision::{
    hierarchy::{AlignedLoop, Hierarchy},
    symmetry::Symmetry,
};
use std::collections::{BTreeMap, BTreeSet};

impl Form {
    pub fn insert_loop(&mut self, edge: [usize; 2], fraction: f64) -> Result<Vec<[usize; 2]>> {
        if self.hierarchy.is_some() {
            return Err(error("whole edge-loop insertion requires a base cage"));
        }
        let (cage, seams) = self.cage.insert_edge_ring(edge, fraction).map_err(error)?;
        self.replace_cage(cage)?;
        Ok(seams)
    }
    pub(crate) fn restore_logical_holes(&mut self, edges: &[[usize; 2]], vertices: &[usize], single: bool, tolerance: f64) -> Result<()> {
        if self.matching.is_some() {
            return Err(error("refined Fill Hole requires detaching Match before changing its openings"));
        }
        let hierarchy = self.hierarchy.as_ref().ok_or_else(|| error("missing refined Form"))?;
        let restored = hierarchy
            .restore_deleted_material(edges, vertices, single)
            .map_err(error)?
            .ok_or_else(|| error("this is an original free boundary. Choose New cap for one four-sided opening"))?;
        self.adopt_fitted_hierarchy(restored, tolerance)
    }
    pub(crate) fn erase_and_fill_logical(&mut self, faces: &[usize], tolerance: f64) -> Result<()> {
        if self.profile.is_some() || self.matching.is_some() || self.symmetry.is_some() {
            return Err(error("refined Erase and Fill requires an independent Form without symmetry"));
        }
        let hierarchy = self.hierarchy.as_ref().ok_or_else(|| error("missing refined Form"))?;
        let capped = hierarchy.erase_and_cap(faces).map_err(error)?;
        self.adopt_fitted_hierarchy(capped, tolerance)
    }
    fn adopt_fitted_hierarchy(&mut self, hierarchy: Hierarchy, tolerance: f64) -> Result<()> {
        let mut definition = self.definition();
        definition.cage = hierarchy.evaluate().map_err(error)?;
        definition.hierarchy = Some(hierarchy);
        // Qualify the exact CAD result before authoring one Undo checkpoint.
        crate::doc::forms::convert(&definition, tolerance)?;
        let hierarchy = definition.hierarchy.take().ok_or_else(|| error("missing fitted hierarchy"))?;
        self.adopt_hierarchy(hierarchy, true)
    }
    pub(crate) fn author_logical_cap(&mut self, edges: &[[usize; 2]], vertices: &[usize], tolerance: f64) -> Result<()> {
        let mut candidate = crate::doc::forms::author_quad_cap(&self.definition(), tolerance, edges, vertices)?;
        let hierarchy = candidate.hierarchy.take().ok_or_else(|| error("missing qualified quad cap hierarchy"))?;
        self.adopt_hierarchy(hierarchy, true)
    }
    pub fn bridge_boundaries(&mut self, edges: &[[usize; 2]], vertices: &[usize]) -> Result<()> {
        if self.hierarchy.as_ref().is_some_and(Hierarchy::has_quad_cap_history) {
            return Err(error("Bridge would replace the reserved or retired quad cap face slots"));
        }
        let local = self.hierarchy.is_some();
        let (cage, remap) = if let Some(h) = &self.hierarchy {
            h.materialize().map_err(error)?
        } else {
            (self.cage.clone(), (0..self.cage.points.len()).map(|i| (i, i)).collect())
        };
        let seeds = if edges.is_empty() {
            if vertices.len() != 2 {
                return Err(error("select one edge on each of two free boundaries"));
            }
            vertices.to_vec()
        } else {
            let mut uses: BTreeMap<(usize, usize), usize> = BTreeMap::new();
            for face in &self.logical.faces {
                for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
                    *uses.entry((a.min(b), a.max(b))).or_default() += 1;
                }
            }
            if edges.iter().any(|e| uses.get(&(e[0].min(e[1]), e[0].max(e[1]))) != Some(&1)) {
                return Err(error("bridge requires free logical boundary edges"));
            }
            edges.iter().map(|e| e[0]).collect()
        };
        let loops = cage.boundary_loops().map_err(error)?;
        let mut selected = BTreeSet::new();
        for seed in seeds {
            let vertex = *remap.get(&seed).ok_or_else(|| error("selected boundary did not survive materialization"))?;
            let id = loops.iter().position(|l| l.contains(&vertex)).ok_or_else(|| error("select a free boundary"))?;
            selected.insert(id);
        }
        if selected.len() != 2 {
            return Err(error("select exactly two distinct boundary loops"));
        }
        let mut seeds = Vec::new();
        for id in selected {
            seeds.push(*loops.get(id).and_then(|l| l.first()).ok_or_else(|| error("empty selected boundary"))?);
        }
        let a = *seeds.first().ok_or_else(|| error("missing first boundary"))?;
        let b = *seeds.get(1).ok_or_else(|| error("missing second boundary"))?;
        self.adopt_base(cage.bridge_transition([a, b], local).map_err(error)?)
    }
}

impl Form {
    /// Trace source logical edges; no display mesh participates in continuation.
    #[cfg(test)]
    pub(crate) fn hierarchical_loop_records(&self, edge: [usize; 2], fraction: f64) -> Result<Vec<([usize; 2], f64)>> {
        self.trace_hierarchical_loop(edge, fraction)?.ok_or_else(|| error("regular Insert Loop encountered a mixed-depth T-junction"))
    }

    // None denotes a genuine mixed-depth continuation. Other invalid requests
    // preserve their original errors instead of taking a different route.
    fn trace_hierarchical_loop(&self, edge: [usize; 2], fraction: f64) -> Result<Option<Vec<InsertRecord>>> {
        if self.profile.is_some() {
            return Err(error("Form Surface keeps its original regular guide grid"));
        }
        if !fraction.is_finite() || !(1e-6..=1.0 - 1e-6).contains(&fraction) || edge[0] == edge[1] {
            return Err(error("insert positions must be inside a nondegenerate logical edge"));
        }
        let hierarchy = self.hierarchy.as_ref().ok_or_else(|| error("missing insertion hierarchy"))?;
        if hierarchy.has_trim_wall() || !hierarchy.dissolved_edges.is_empty() {
            return Err(error("remove the trim wall or dissolved logical regions before Insert Loop"));
        }
        let layout = hierarchy.layout().map_err(error)?;
        let mut uses = BTreeMap::<[usize; 2], Vec<usize>>::new();
        for (id, face) in layout.faces.iter().enumerate().filter(|(_, face)| !face.is_empty()) {
            for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
                let incident = uses.entry(loop_key([a, b])).or_default();
                incident.push(id);
                if incident.len() > 2 {
                    return Err(error("Insert Loop requires manifold logical edges"));
                }
            }
        }
        let seed = loop_key(edge);
        let sides = uses.get(&seed).ok_or_else(|| error("select a source logical edge"))?;
        let first = *sides.first().ok_or_else(|| error("selected edge has no logical face"))?;
        let mut crossed = BTreeSet::new();
        let mut directions = BTreeMap::from([(seed, edge)]);
        let Some((mut a, closed)) = walk_loop(&layout.faces, &uses, edge, first, fraction, &mut crossed, &mut directions)? else { return Ok(None) };
        let initial = (seed, loop_fraction(edge, fraction));
        if closed {
            a.insert(0, initial);
            return Ok(Some(a));
        }
        // Both walks use the same transported edge orientation, even though
        // the first half is reversed to order a boundary-to-boundary strip.
        a.reverse();
        a.push(initial);
        if let Some(&second) = sides.get(1) {
            let Some((b, closed)) = walk_loop(&layout.faces, &uses, edge, second, fraction, &mut crossed, &mut directions)? else { return Ok(None) };
            if closed {
                return Err(error("Insert Loop has inconsistent boundary continuation"));
            }
            a.extend(b);
        }
        Ok(Some(a))
    }

    pub(crate) fn insert_hierarchical_loop(
        &mut self,
        edge: [usize; 2],
        fraction: f64,
        state: Option<&crate::doc::ModelState>,
        tolerance: f64,
    ) -> Result<()> {
        match self.trace_hierarchical_loop(edge, fraction)? {
            Some(records) => self.insert_regular_loop(&records, state, tolerance),
            None => self.insert_aligned_loop(AlignedSeed::Edge(edge, fraction), state, tolerance),
        }
    }

    pub(crate) fn insert_hierarchical_loop_from_leaf(
        &mut self,
        face: usize,
        axis: usize,
        state: Option<&crate::doc::ModelState>,
        tolerance: f64,
    ) -> Result<()> {
        if axis > 1 {
            return Err(error("insert axis must be U or V"));
        }
        let hierarchy = self.hierarchy.as_ref().ok_or_else(|| error("a face/axis loop requires a Form hierarchy"))?;
        let leaves = hierarchy.parameter_sides().map_err(error)?;
        let leaf = leaves.iter().find(|leaf| leaf.id == face).ok_or_else(|| error("select an active rectangular logical face"))?;
        let extent = leaf.domain[axis + 2] - leaf.domain[axis];
        if extent < 2 || !extent.is_multiple_of(2) {
            return Err(error("the requested midpoint is outside the aligned dyadic depth budget"));
        }
        if leaf.sides.iter().all(|side| side.len() == 1) {
            let edge = leaf.sides[axis].first().ok_or_else(|| error("missing regular parameter side"))?.edge;
            if let Some(records) = self.trace_hierarchical_loop(edge, 0.5)? {
                return self.insert_regular_loop(&records, state, tolerance);
            }
        }
        self.insert_aligned_loop(AlignedSeed::Leaf(face, axis), state, tolerance)
    }

    fn insert_regular_loop(&mut self, records: &[InsertRecord], state: Option<&crate::doc::ModelState>, tolerance: f64) -> Result<()> {
        let Some(symmetry) = &self.symmetry else { return self.insert_source_points(records, state, tolerance, true) };
        let mirrored = records
            .iter()
            .map(|&(edge, fraction)| {
                let a = *symmetry.partners.get(edge[0]).ok_or_else(|| error("missing first mirrored insertion control"))?;
                let b = *symmetry.partners.get(edge[1]).ok_or_else(|| error("missing second mirrored insertion control"))?;
                Ok((loop_key([a, b]), loop_fraction([a, b], fraction)))
            })
            .collect::<Result<Vec<_>>>()?;
        self.insert_source_chains(&[records, &mirrored], state, tolerance, true)
    }
    fn insert_aligned_loop(&mut self, seed: AlignedSeed, state: Option<&crate::doc::ModelState>, tolerance: f64) -> Result<()> {
        if self.profile.is_some() {
            return Err(error("Form Surface keeps its original regular guide grid"));
        }
        let mut source = self.definition();
        let mut candidate = self.clone();
        if let Some(matching) = &source.matching {
            let state = state.ok_or_else(|| error("matched insertion requires its source document"))?;
            let (resolved, report) = matching.qualify_aligned_source(&source, state, tolerance)?;
            candidate.refresh_match_preview(resolved.clone(), report)?;
            source = resolved;
        }
        let original = source.hierarchy.as_ref().ok_or_else(|| error("missing insertion hierarchy"))?;
        let mut plans = vec![seed.trace(original)?];
        if let Some(symmetry) = &source.symmetry {
            plans.push(seed.mirrored(&candidate, original, symmetry)?.trace(original)?);
        }
        let cuts: Vec<_> = plans.iter().flat_map(|plan| plan.cuts.iter().copied()).collect();
        if cuts.is_empty() {
            return Ok(());
        }
        let refined = original.split_at(&cuts).map_err(error)?;
        let points: Vec<_> = plans.iter().flat_map(|plan| plan.points.iter().cloned()).collect();
        // Resolve in bounded batches: duplicates from the reflected plan are
        // permitted but never create another source/control identity.
        let mut inserted = Vec::new();
        for batch in points.chunks(256) {
            inserted.extend(refined.aligned_loop_vertices(batch).map_err(error)?);
        }
        let unique: BTreeSet<_> = inserted.iter().copied().collect();
        if unique.len() > 256 {
            return Err(error("aligned Insert Loop exceeds 256 unique stations"));
        }
        let old_layout = original.layout().map_err(error)?;
        let mut records = Vec::new();
        let mut vertices = Vec::new();
        let mut restriction_work = 4_000_000usize;
        for (point, &vertex) in points.iter().zip(&inserted) {
            if point.existing.is_some_and(|id| id != vertex) || point.existing.is_none() && original.controls.contains_key(&vertex) {
                return Err(error("aligned Insert Loop changed its control birth provenance"));
            }
            if let Some(id) = point.existing {
                restriction_work = restriction_work
                    .checked_sub(old_layout.paths.len())
                    .ok_or_else(|| error("aligned exact restriction scan exceeds its shared work budget"))?;
                // Every incident old exact restriction must agree at a reused
                // T-control; an arbitrary neighboring curve is not a target.
                for ([a, b], _) in old_layout.paths.iter().filter(|(edge, _)| edge.contains(&id)) {
                    if records.len() >= 4096 {
                        return Err(error("aligned Insert Loop exceeds its exact restriction budget"));
                    }
                    records.push(([*a, *b], if *a == id { 0.0 } else { 1.0 }));
                    vertices.push(vertex);
                }
            } else {
                if records.len() >= 4096 {
                    return Err(error("aligned Insert Loop exceeds its exact restriction budget"));
                }
                records.push((point.source_edge, point.fraction));
                vertices.push(vertex);
            }
        }
        if records.len() > 4096 {
            return Err(error("aligned Insert Loop exceeds its exact restriction budget"));
        }
        let curves = crate::doc::forms::edge_curves(&source, tolerance, &records.iter().map(|r| r.0).collect::<Vec<_>>())?;
        let mut targets = BTreeMap::new();
        for ((curve, record), &vertex) in curves.iter().zip(&records).zip(&vertices) {
            let target = curve.at(record.1)?;
            if targets.insert(vertex, target).is_some_and(|previous: Vec3| previous.dist(target) > 1e-7) {
                return Err(error("original CAD restrictions disagree at a reused T-control"));
            }
        }
        candidate.matching = None;
        candidate.match_report = None;
        candidate.adopt_hierarchy(refined, false)?;
        if let Some(matching) = &source.matching {
            let state = state.ok_or_else(|| error("matched insertion requires its source document"))?;
            let (definition, report) = matching.rebind_aligned_refinement(&source, candidate.definition(), state, tolerance, &targets)?;
            candidate.refresh_match_preview(definition, report)?;
        }
        // Rebinding may solve nearby controls, including existing T-controls.
        // Qualify its complete continued evaluator rather than assuming a
        // successful Match report means a source-preserving metadata change.
        candidate.verify_aligned_continuation(&source)?;
        // Endpoint agreement remains a separate fitted-CAD target check. It
        // must not stand in for preservation of the old evaluated source.
        for (&vertex, &target) in &targets {
            let point = candidate.local_vertices.get(vertex).copied().ok_or_else(|| error("missing aligned CAD endpoint"))?;
            if candidate.world_point(point).dist(target) > tolerance.max(1e-8) {
                return Err(error("aligned mixed-depth insertion needs an unsupported constrained CAD correction"));
            }
        }
        if let Some(old) = &source.symmetry {
            let symmetry = candidate.symmetry.as_ref().ok_or_else(|| error("aligned insertion lost symmetry"))?;
            if symmetry.partners.get(..old.partners.len()) != Some(old.partners.as_slice()) {
                return Err(error("aligned insertion changed an original mirror partner"));
            }
            let mut station_lookup = BTreeMap::<[usize; 2], Vec<(f64, usize)>>::new();
            for (record, &vertex) in records.iter().zip(&vertices) {
                station_lookup.entry(record.0).or_default().push((record.1, vertex));
            }
            for (record, &vertex) in records.iter().zip(&vertices) {
                let [a, b] = record.0.map(|id| old.partners.get(id).copied());
                let (Some(a), Some(b)) = (a, b) else {
                    return Err(error("missing original mirrored endpoint"));
                };
                let edge = loop_key([a, b]);
                let fraction = loop_fraction([a, b], record.1);
                let partner = symmetry.partners.get(vertex).copied().ok_or_else(|| error("missing new mirrored control"))?;
                if !station_lookup
                    .get(&edge)
                    .is_some_and(|entries| entries.iter().any(|&(other, id)| (other - fraction).abs() <= 4.0 * f64::EPSILON && id == partner))
                {
                    return Err(error("aligned controls disagree with planned mirrored source stations"));
                }
                let target = *targets.get(&vertex).ok_or_else(|| error("missing aligned mirror target"))?;
                let other = *targets.get(&partner).ok_or_else(|| error("missing paired aligned target"))?;
                if source.placement.point(old.reflect(source.placement.local_point(target))).dist(other) > tolerance.max(1e-8) {
                    return Err(error("aligned CAD targets are incompatible with symmetry"));
                }
            }
            symmetry.validate_reflection(&candidate.cage).map_err(error)?;
            candidate.validate_logical_symmetry(symmetry)?;
        }
        self.checkpoint();
        candidate.undo = std::mem::take(&mut self.undo);
        candidate.redo.clear();
        *self = candidate;
        Ok(())
    }
    /// Preserve the complete subdivision continuation, independently of the
    /// new seam targets and the GUI fit tolerance. Match also retains the
    /// exact earlier tensor chart ancestry. G2 uses the actual exterior-row
    /// continuation, including its noneditable ghost ring.
    pub(crate) fn verify_aligned_continuation(&self, source: &crate::doc::forms::Definition) -> Result<()> {
        let old = source.hierarchy.as_ref().ok_or_else(|| error("missing original aligned hierarchy"))?;
        let next = self.hierarchy.as_ref().ok_or_else(|| error("missing continued aligned hierarchy"))?;
        let levels = next.depth.checked_sub(old.depth).ok_or_else(|| error("aligned insertion discarded source depth"))?;
        if levels > solidcraft_geom::subdivision::hierarchy::MAX_DEPTH || source.cage.points.len().saturating_mul(1_usize << (2 * levels)) > 262_144 {
            return Err(error("aligned source preservation exceeds its evaluator budget"));
        }
        if self.placement != source.placement
            || next.base != old.base
            || next.details.get(..old.details.len()) != Some(old.details.as_slice())
            || old.controls.iter().any(|(id, birth)| next.controls.get(id) != Some(birth))
        {
            return Err(error("aligned Match continuation changed an original authored coefficient or control birth"));
        }
        let order = source.matching.as_ref().map_or(Ok(0), |matching| matching.evaluation_order(&source.cage, Some(old)))?;
        let expected = if order == 2 {
            solidcraft_geom::subdivision::matching::refine_g2(&source.cage, levels).map_err(error)?
        } else {
            source.cage.refined(levels).map_err(error)?
        };
        if self.cage.faces != expected.faces
            || self.cage.points.len() != expected.points.len()
            || self.cage.edge_creases != expected.edge_creases
            || self.cage.vertex_creases != expected.vertex_creases
        {
            return Err(error("aligned Match continuation changed the original evaluator topology or sharpness"));
        }
        for (&actual, &expected) in self.cage.points.iter().zip(&expected.points) {
            if !crate::kernel::form_curves::continued_control_within_roundoff(actual, expected, levels) {
                return Err(error("aligned insertion needs a constrained correction that would move the original continued source"));
            }
        }
        if source.matching.is_some() {
            let matching = self.matching.as_ref().ok_or_else(|| error("aligned insertion lost its Match evaluator"))?;
            let actual_order = matching.evaluation_order(&self.cage, Some(next))?;
            if actual_order != order {
                return Err(error("aligned insertion changed the Match continuation rule"));
            }
            matching.verify_continued_surface(source, &self.definition())?;
        }
        Ok(())
    }
    /// Uniform pointwise bound for two surfaces with the exact same positive
    /// rational basis. The normalized B-spline weights are nonnegative and sum
    /// to one, so every point difference is a convex combination of the control
    /// differences. Outward arithmetic bounds the whole parameter domain.
    #[cfg(test)]
    pub(crate) fn aligned_surface_difference_bound(
        before: &solidcraft_geom::bspline::NurbsSurface,
        after: &solidcraft_geom::bspline::NurbsSurface,
    ) -> Result<f64> {
        if before.degree() != after.degree() || before.knots() != after.knots() || before.count() != after.count() {
            return Err(error("continued tensor certificate requires identical parameter bases"));
        }
        let [u, v] = before.count();
        if u.checked_mul(v).is_none_or(|count| count > 16_384) {
            return Err(error("continued tensor certificate exceeds its coefficient budget"));
        }
        let mut maximum = 0.0_f64;
        for j in 0..v {
            for i in 0..u {
                let a = before.point(i, j);
                let b = after.point(i, j);
                if !a.is_finite() || !b.is_finite() || a.w != b.w || a.w < f64::MIN_POSITIVE {
                    return Err(error("continued tensor certificate requires identical finite positive rational weights"));
                }
                let weight = a.w;
                let coordinate = |a: f64, b: f64| ((a - b).abs().next_up() / weight).next_up();
                let delta = [coordinate(a.x, b.x), coordinate(a.y, b.y), coordinate(a.z, b.z)];
                let squared = delta.into_iter().fold(0.0_f64, |sum, value| (sum + (value * value).next_up()).next_up());
                let bound = squared.sqrt().next_up();
                if !bound.is_finite() {
                    return Err(error("continued tensor coefficient bound overflowed"));
                }
                maximum = maximum.max(bound);
            }
        }
        Ok(maximum)
    }
    /// Preserve recorded mirror identities and check CAD targets in the form's
    /// local frame. No nearest-point rematching decides the mirror source.
    pub(super) fn verify_insertion_symmetry(
        &self,
        source: &crate::doc::forms::Definition,
        records: &[InsertRecord],
        inserted: &[usize],
        targets: &[Vec3],
        tolerance: f64,
    ) -> Result<()> {
        let Some(original) = &source.symmetry else { return Ok(()) };
        let symmetry = self.symmetry.as_ref().ok_or_else(|| error("Insert Loop lost its mirror correspondence"))?;
        if symmetry.partners.get(..original.partners.len()) != Some(original.partners.as_slice()) {
            return Err(error("Insert Loop changed an existing mirror partner identity"));
        }
        let mut endpoints = BTreeMap::<_, Vec<_>>::new();
        for ((&(edge, fraction), &vertex), &target) in records.iter().zip(inserted).zip(targets) {
            endpoints.entry(loop_key(edge)).or_default().push((loop_fraction(edge, fraction), vertex, target));
        }
        for (&edge, stations) in &endpoints {
            let a = *original.partners.get(edge[0]).ok_or_else(|| error("missing original mirror endpoint"))?;
            let b = *original.partners.get(edge[1]).ok_or_else(|| error("missing original mirror endpoint"))?;
            let others = endpoints.get(&loop_key([a, b])).ok_or_else(|| error("missing mirrored insertion target"))?;
            for &(fraction, vertex, target) in stations {
                let mirrored = loop_fraction([a, b], fraction);
                let partner = symmetry.partners.get(vertex).ok_or_else(|| error("missing new mirrored control"))?;
                let matching = others.iter().filter(|&&(other, _, _)| (other - mirrored).abs() <= 4.0 * f64::EPSILON);
                let reflected = source.placement.point(original.reflect(source.placement.local_point(target)));
                let mut found = false;
                for &(_, id, other) in matching {
                    if id != *partner {
                        return Err(error("inserted controls do not retain the planned mirror identities"));
                    }
                    if reflected.dist(other) > tolerance.max(1e-8) {
                        return Err(error("original CAD insertion targets are incompatible with the mirror plane"));
                    }
                    found = true;
                }
                if !found {
                    return Err(error("missing mirrored insertion target"));
                }
            }
        }
        symmetry.validate_reflection(&self.cage).map_err(error)?;
        self.validate_logical_symmetry(symmetry)
    }
}

#[derive(Clone, Copy)]
enum AlignedSeed {
    Edge([usize; 2], f64),
    Leaf(usize, usize),
}

impl AlignedSeed {
    fn trace(self, hierarchy: &Hierarchy) -> Result<AlignedLoop> {
        match self {
            Self::Edge(edge, fraction) => hierarchy.aligned_loop(edge, fraction),
            Self::Leaf(face, axis) => hierarchy.aligned_loop_from_leaf(face, axis),
        }
        .map_err(error)
    }

    fn mirrored(self, form: &Form, hierarchy: &Hierarchy, symmetry: &Symmetry) -> Result<Self> {
        let partner = |id| symmetry.partners.get(id).copied().ok_or_else(|| error("missing mirrored source control"));
        match self {
            Self::Edge([a, b], fraction) => Ok(Self::Edge([partner(a)?, partner(b)?], fraction)),
            Self::Leaf(face, axis) => {
                let mirror = form.mirror_face_selection(&[face])?.into_iter().find(|&id| id != face).unwrap_or(face);
                let corners = hierarchy.leaf_corners().map_err(error)?;
                let from = corners.get(face).copied().flatten().ok_or_else(|| error("missing source leaf corners"))?;
                let [a, b, c, d] = corners.get(mirror).copied().flatten().ok_or_else(|| error("missing mirrored leaf corners"))?;
                let along = match axis {
                    0 => from[1],
                    1 => from[3],
                    _ => return Err(error("insert axis must be U or V")),
                };
                let reflected = loop_key([partner(from[0])?, partner(along)?]);
                let axis = if [loop_key([a, b]), loop_key([c, d])].contains(&reflected) {
                    0
                } else if [loop_key([b, c]), loop_key([d, a])].contains(&reflected) {
                    1
                } else {
                    return Err(error("the reflected loop axis leaves its mirrored parameter side"));
                };
                Ok(Self::Leaf(mirror, axis))
            }
        }
    }
}

fn loop_key([a, b]: [usize; 2]) -> [usize; 2] {
    [a.min(b), a.max(b)]
}

fn loop_fraction(edge: [usize; 2], fraction: f64) -> f64 {
    if edge == loop_key(edge) { fraction } else { 1.0 - fraction }
}

fn walk_loop(
    faces: &[Vec<usize>],
    uses: &BTreeMap<[usize; 2], Vec<usize>>,
    seed: [usize; 2],
    mut face_id: usize,
    fraction: f64,
    crossed: &mut BTreeSet<usize>,
    directions: &mut BTreeMap<[usize; 2], [usize; 2]>,
) -> Result<Option<(Vec<InsertRecord>, bool)>> {
    let mut current = seed;
    let mut records = Vec::new();
    loop {
        if !crossed.insert(face_id) {
            return Err(error("Insert Loop cannot cross the same logical face twice"));
        }
        let face = faces.get(face_id).ok_or_else(|| error("missing logical insertion face"))?;
        if face.len() != 4 {
            return Ok(None);
        }
        let side = face
            .iter()
            .copied()
            .zip(face.iter().copied().cycle().skip(1))
            .take(4)
            .position(|(a, b)| loop_key([a, b]) == loop_key(current))
            .ok_or_else(|| error("logical insertion edge is missing from its face"))?;
        let corner = |offset| face.get((side + offset) % 4).copied().ok_or_else(|| error("missing logical quad corner"));
        let next = if current == [corner(0)?, corner(1)?] { [corner(3)?, corner(2)?] } else { [corner(2)?, corner(3)?] };
        let key = loop_key(next);
        if let Some(previous) = directions.get(&key) {
            if key != loop_key(seed) || *previous != next {
                return Err(error("Insert Loop repeats an edge or reverses its parameter direction"));
            }
            records.push((key, loop_fraction(next, fraction)));
            return Ok(Some((records, true)));
        }
        if directions.len() >= 256 {
            return Err(error("Insert Loop exceeds 256 unique source edge positions"));
        }
        directions.insert(key, next);
        records.push((key, loop_fraction(next, fraction)));
        let incident = uses.get(&key).ok_or_else(|| error("opposite logical edge has no adjacent face"))?;
        let mut continuation = incident.iter().copied().filter(|&id| id != face_id);
        let Some(next_face) = continuation.next() else {
            return Ok(Some((records, false)));
        };
        if continuation.next().is_some() {
            return Err(error("Insert Loop has branching logical continuation"));
        }
        current = next;
        face_id = next_face;
    }
}
