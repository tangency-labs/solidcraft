//! Trim-aware region queries over a native edited face's raw parameterization.
//! Preparation belongs on the CAD worker; queries share a bounded work budget.
use super::{curve_pieces, projected_curve};
use crate::{selection::BoxSel, viewport::Proj};
use solidcore_record::{
    Vec3,
    brep::{Curve2, Loop, Surface},
};
use solidcraft_engine::{
    doc::kernel::Body,
    geom::{
        brep::eval::SurfaceEval,
        bspline::{CurveN, Dir, HPoint, NurbsSurface, insert_knot},
        tess::{EvalSurface, from_kernel},
    },
};

type Result<T> = std::result::Result<T, String>;
const MAX_CONTROLS: usize = 16_384;
const MAX_DEGREE: usize = 32;
const MAX_DEPTH: usize = 28;
const SCREEN_UNCERTAINTY: f64 = 0.25;

#[derive(Clone)]
struct Tile {
    degree: [usize; 2],
    uv: [[f64; 2]; 2],
    controls: Vec<HPoint>,
}
#[derive(Clone)]
struct JoinBand {
    uv: [[f64; 2]; 2],
    world: [[f64; 2]; 3],
}
pub(super) struct PreparedFace {
    pub(super) native_face: usize,
    tiles: Vec<Tile>,
    /// Stored UV restrictions and certified pole or native-join connectors.
    trims: Vec<Vec<HPoint>>,
    // Native topology closes these independently rounded parameter endpoints.
    // The connector is never an exact boundary witness inside this band.
    joins: Vec<JoinBand>,
}
fn spend(remaining: &mut usize, work: usize) -> Result<()> {
    *remaining = remaining.checked_sub(work).ok_or("Form face selection exceeded its work budget")?;
    Ok(())
}
fn valid_controls(controls: &[HPoint]) -> bool {
    !controls.is_empty() && controls.len() <= MAX_CONTROLS && controls.iter().all(|point| point.is_finite() && point.w >= f64::MIN_POSITIVE)
}
impl PreparedFace {
    /// Read canonical native IDs and stored pcurves. FaceGeometry's display
    /// parameterization is deliberately not mixed with these raw record trims.
    pub(super) fn prepare(body: &Body, native_face: usize, remaining: &mut usize) -> Result<Self> {
        let face = body.record().faces.get(native_face).ok_or("Form face identity is outside its exact body")?;
        if face.domain.iter().any(|range| !range[0].is_finite() || !range[1].is_finite() || range[0] >= range[1]) {
            return Err("Form face has no finite raw parameter domain".into());
        }
        if face.loops.is_empty() || face.loops.len() > 4096 {
            return Err("Form face exceeds its trim-loop budget".into());
        }
        spend(remaining, face.loops.len())?;
        preflight_surface(face.parameters(), remaining)?;
        let surface = SurfaceEval::new(face.parameters()).map_err(|error| error.to_string())?;
        let (tiles, spline) = surface_tiles(&surface, face.domain, remaining)?;
        let mut trims = Vec::new();
        let mut joins = Vec::new();
        let mut trim_controls = 0_usize;
        for wire in &face.loops {
            let Loop::Edges { edges } = wire else { continue };
            if edges.is_empty() || edges.len() > 4096 {
                return Err("Form face trim loop exceeds its edge budget".into());
            }
            let mut first = None;
            let mut previous = None;
            for use_ in edges {
                let edge = body.record().edges.get(use_.edge as usize).ok_or("Form trim lost its native edge")?;
                let mut pieces = pcurve_pieces(&use_.pcurve, edge.interval, remaining)?;
                if !use_.forward {
                    pieces.reverse();
                    for piece in &mut pieces {
                        piece.reverse();
                    }
                }
                let a = uv_endpoint(&pieces, true)?;
                let b = uv_endpoint(&pieces, false)?;
                let [start, end] = if use_.forward { edge.vertices } else { [edge.vertices[1], edge.vertices[0]] };
                if let Some((point, vertex, previous_tolerance)) = previous {
                    if vertex != start {
                        return Err("Form trim loop does not retain one native vertex at its join".into());
                    }
                    let native = body.record().vertices.get(vertex as usize).ok_or("Form trim lost its shared native vertex")?;
                    close_native_uv(
                        point,
                        a,
                        &surface,
                        spline.as_ref(),
                        NativeJoin {
                            point: Vec3::from(native.point),
                            vertex_tolerance: native.tolerance,
                            edge_tolerances: [previous_tolerance, edge.tolerance],
                            tiles: &tiles,
                        },
                        JoinOutput { curves: &mut trims, joins: &mut joins },
                        remaining,
                    )?;
                } else {
                    first = Some((a, start, edge.tolerance));
                }
                previous = Some((b, end, edge.tolerance));
                trim_controls = trim_controls.checked_add(pieces.iter().map(Vec::len).sum::<usize>()).ok_or("Form trim size overflowed")?;
                if trim_controls > MAX_CONTROLS || trims.len() > MAX_CONTROLS {
                    return Err("Form face trim extraction exceeds its coefficient budget".into());
                }
                trims.extend(pieces);
            }
            if let (Some((a, first_vertex, first_tolerance)), Some((b, last_vertex, last_tolerance))) = (first, previous) {
                if first_vertex != last_vertex {
                    return Err("Form trim loop is not closed in native topology".into());
                }
                let native = body.record().vertices.get(first_vertex as usize).ok_or("Form trim lost its closing native vertex")?;
                close_native_uv(
                    b,
                    a,
                    &surface,
                    spline.as_ref(),
                    NativeJoin {
                        point: Vec3::from(native.point),
                        vertex_tolerance: native.tolerance,
                        edge_tolerances: [last_tolerance, first_tolerance],
                        tiles: &tiles,
                    },
                    JoinOutput { curves: &mut trims, joins: &mut joins },
                    remaining,
                )?;
            }
        }
        if trims.is_empty() || trims.len() > MAX_CONTROLS {
            return Err("Form face has no bounded exact trim region".into());
        }
        Ok(Self { native_face, tiles, trims, joins })
    }
    /// Crossing uses any material-point witness; window/lasso requires the
    /// complete trimmed image. UV subdivision covers interior silhouettes and
    /// folds as well as the visible boundary. Occlusion is a separate UI query.
    pub(super) fn takes(&self, proj: &Proj, region: &BoxSel, remaining: &mut usize) -> Result<bool> {
        if !region.rect.is_finite()
            || region.rect.width() < 0.0
            || region.rect.height() < 0.0
            || (region.is_lasso() && (region.lasso.len() < 3 || region.lasso.len() > 4096 || region.lasso.iter().any(|point| !point.is_finite())))
        {
            return Err("invalid Form face selection region".into());
        }
        self.qualify_join_bands(proj, region, remaining)?;
        let mut front = 0;
        for tile in &self.tiles {
            spend(remaining, tile.controls.len())?;
            front += usize::from(project_tile(proj, tile)?.is_some());
        }
        if front == 0 {
            return Ok(false);
        }
        if front != self.tiles.len() {
            return Err("the Form face crosses the camera near plane; rotate or zoom out".into());
        }
        let crossing = region.crossing && !region.is_lasso();
        let mut uncertain = None;
        for tile in &self.tiles {
            match self.classify(proj, region, tile, remaining, 0) {
                Ok(value) if value == crossing => return Ok(value),
                Ok(_) => {}
                Err(error) => uncertain = Some(error),
            }
        }
        uncertain.map_or(Ok(!crossing), Err)
    }

    /// Is there at least one exact material point inside this screen region
    /// that the display visibility query accepts? Drag direction does not affect
    /// this existential query. Hidden witnesses alone never prove full occlusion.
    #[cfg(test)]
    pub(super) fn has_visible<V, O>(
        &self,
        proj: &Proj,
        region: &BoxSel,
        remaining: &mut usize,
        visible: &mut V,
        occluded_hull: &mut O,
    ) -> Result<bool>
    where
        V: FnMut(Vec3, &mut usize) -> Result<bool>,
        O: FnMut(&[HPoint], &mut usize) -> Result<bool>,
    {
        Self::group_has_visible(std::slice::from_ref(self), proj, region, remaining, visible, occluded_hull)
    }
    /// Iterative deepening gives every native piece and root the same depth
    /// before descending farther. Roots remain borrowed. A pass retains only
    /// one depth-first branch and its siblings: at most 2 * (MAX_DEPTH + 1)
    /// owned nets, each bounded by (MAX_DEGREE + 1)^2 coefficients. Thus a wide
    /// unresolved silhouette cannot evict a later exact visible witness.
    /// A bounded, query-local proof cache avoids reclassifying known subtrees.
    /// Eviction causes recomputation only; it never discards an unresolved path.
    /// Every pass, cache access and reconstruction debits the shared budget.
    pub(super) fn group_has_visible<V, O>(
        faces: &[Self],
        proj: &Proj,
        region: &BoxSel,
        remaining: &mut usize,
        visible: &mut V,
        occluded_hull: &mut O,
    ) -> Result<bool>
    where
        V: FnMut(Vec3, &mut usize) -> Result<bool>,
        O: FnMut(&[HPoint], &mut usize) -> Result<bool>,
    {
        projected_curve::validate_visibility_region(region, remaining)?;
        if faces.is_empty() {
            return Err("no prepared native Form pieces for visibility".into());
        }
        let mut search = FaceVisibilitySearch { remaining, pages: Vec::new() };
        for limit in 0..=MAX_DEPTH {
            let mut cutoff = false;
            let mut uncertain = None;
            let mut root = 0usize;
            for face in faces {
                let first_root = root;
                root = root.checked_add(face.tiles.len()).ok_or("native Form root ordinal overflowed")?;
                spend(search.remaining, 1)?;
                if face.tiles.is_empty() || face.tiles.len() > MAX_CONTROLS {
                    uncertain = Some("invalid prepared Form visibility tile count".into());
                    continue;
                }
                if let Err(error) = face.qualify_join_bands(proj, region, search.remaining) {
                    uncertain = Some(error);
                    continue;
                }
                for (index, tile) in face.tiles.iter().enumerate() {
                    let visit = FaceVisibilityVisit { root: first_root + index, path: 1, depth: 0, limit };
                    match face.visible_through(proj, region, tile, &mut search, visible, occluded_hull, visit) {
                        Ok(FaceVisibilityPass::Visible) => return Ok(true),
                        Ok(FaceVisibilityPass::Hidden) => {}
                        Ok(FaceVisibilityPass::Cutoff(error)) => {
                            cutoff = true;
                            if error.is_some() {
                                uncertain = error;
                            }
                        }
                        Err(error) if error == super::CANCELLED => return Err(error),
                        Err(error) => uncertain = Some(error),
                    }
                }
            }
            if !cutoff {
                return uncertain.map_or(Ok(false), Err);
            }
        }
        Err("Form face visibility remains unresolved; move the selection boundary or view slightly".into())
    }
    fn visible_through<V, O>(
        &self,
        proj: &Proj,
        region: &BoxSel,
        tile: &Tile,
        search: &mut FaceVisibilitySearch<'_>,
        visible: &mut V,
        occluded_hull: &mut O,
        visit: FaceVisibilityVisit,
    ) -> Result<FaceVisibilityPass>
    where
        V: FnMut(Vec3, &mut usize) -> Result<bool>,
        O: FnMut(&[HPoint], &mut usize) -> Result<bool>,
    {
        let node = match search.get(visit)? {
            Some(FaceVisibilityProof::Hidden) => return Ok(FaceVisibilityPass::Hidden),
            Some(FaceVisibilityProof::Split) => {
                let work =
                    tile.controls.len().checked_mul(tile.degree[0] + tile.degree[1] + 2).ok_or("native Form reconstruction work overflowed")?;
                spend(search.remaining, work)?;
                FaceVisibilityNode::Split(split_tile(tile, visit.depth % 2)?)
            }
            None => self.visible_tile(proj, region, tile, search.remaining, visible, occluded_hull, visit)?,
        };
        match node {
            FaceVisibilityNode::Visible => Ok(FaceVisibilityPass::Visible),
            FaceVisibilityNode::Hidden => {
                search.remember(visit, FaceVisibilityProof::Hidden)?;
                Ok(FaceVisibilityPass::Hidden)
            }
            FaceVisibilityNode::Split(children) => {
                search.remember(visit, FaceVisibilityProof::Split)?;
                if visit.depth == visit.limit {
                    return Ok(FaceVisibilityPass::Cutoff(None));
                }
                spend(search.remaining, 2)?;
                let mut cutoff = false;
                let mut uncertain = None;
                for (index, child) in children.iter().enumerate() {
                    let path =
                        visit.path.checked_mul(2).and_then(|path| path.checked_add(index as u32)).ok_or("native Form subdivision path overflowed")?;
                    let child_visit = FaceVisibilityVisit { path, depth: visit.depth + 1, ..visit };
                    match self.visible_through(proj, region, child, search, visible, occluded_hull, child_visit) {
                        Ok(FaceVisibilityPass::Visible) => return Ok(FaceVisibilityPass::Visible),
                        Ok(FaceVisibilityPass::Hidden) => {}
                        Ok(FaceVisibilityPass::Cutoff(error)) => {
                            cutoff = true;
                            if error.is_some() {
                                uncertain = error;
                            }
                        }
                        Err(error) if error == super::CANCELLED => return Err(error),
                        Err(error) => uncertain = Some(error),
                    }
                }
                if cutoff {
                    Ok(FaceVisibilityPass::Cutoff(uncertain))
                } else if let Some(error) = uncertain {
                    Err(error)
                } else {
                    search.remember(visit, FaceVisibilityProof::Hidden)?;
                    Ok(FaceVisibilityPass::Hidden)
                }
            }
        }
    }
    fn visible_tile<V, O>(
        &self,
        proj: &Proj,
        region: &BoxSel,
        tile: &Tile,
        remaining: &mut usize,
        visible: &mut V,
        occluded_hull: &mut O,
        visit: FaceVisibilityVisit,
    ) -> Result<FaceVisibilityNode>
    where
        V: FnMut(Vec3, &mut usize) -> Result<bool>,
        O: FnMut(&[HPoint], &mut usize) -> Result<bool>,
    {
        let [p, q] = tile.degree;
        if p == 0
            || q == 0
            || p > MAX_DEGREE
            || q > MAX_DEGREE
            || tile.controls.len() != (p + 1) * (q + 1)
            || tile.uv.iter().any(|range| !range[0].is_finite() || !range[1].is_finite() || range[0] >= range[1])
        {
            return Err("invalid prepared Form visibility tile shape".into());
        }
        // A split copies the two outer corners from its parent. That parent
        // checked their visibility before publishing its split proof.
        let inherited_edge = (visit.depth > 0).then(|| ((visit.depth - 1) % 2, f64::from(visit.path & 1)));
        let witnesses = if inherited_edge.is_some() { 3 } else { 5 };
        // One tensor midpoint evaluation, one split, projection of the
        // controls and up to five exact witness checks. Trims and rays charge
        // their own work.
        let work = tile
            .controls
            .len()
            .checked_mul(2 * (p + q + 2) + 16)
            .and_then(|work| work.checked_add(region.lasso.len() * (2 + witnesses) + 4 * witnesses + 12))
            .ok_or("Form visibility work size overflowed")?;
        spend(remaining, work)?;
        if !valid_controls(&tile.controls) {
            return Err("invalid prepared Form visibility coefficients".into());
        }
        let Some(projected) = project_tile(proj, tile)? else { return Ok(FaceVisibilityNode::Hidden) };
        let bbox = bounds(&projected)?;
        if projected_curve::visibility_outside(region, bbox) {
            return Ok(FaceVisibilityNode::Hidden);
        }
        let mut material = false;
        for (local, corner) in
            [([0.0, 0.0], Some(0)), ([1.0, 0.0], Some(p)), ([0.0, 1.0], Some(q * (p + 1))), ([1.0, 1.0], Some(q * (p + 1) + p)), ([0.5, 0.5], None)]
        {
            if inherited_edge.is_some_and(|(axis, side)| local[axis] == side) {
                continue;
            }
            let uv = [tile.uv[0][0] * (1.0 - local[0]) + tile.uv[0][1] * local[0], tile.uv[1][0] * (1.0 - local[1]) + tile.uv[1][1] * local[1]];
            if self.inside(uv, remaining)? != Some(true) {
                continue;
            }
            material = true;
            let world = match corner {
                Some(index) => *tile.controls.get(index).ok_or("missing Form material corner")?,
                None => eval_tile(tile, local)?,
            };
            if witness_visible(proj, region, world, remaining, visible)? {
                return Ok(FaceVisibilityNode::Visible);
            }
        }
        if (!material && self.empty_tile(tile, remaining)?) || occluded_hull(&tile.controls, remaining)? {
            return Ok(FaceVisibilityNode::Hidden);
        }
        if visit.limit == 0 && self.tiles.len() == 1 && self.root_corner_visible(proj, region, tile, remaining, visible)? {
            return Ok(FaceVisibilityNode::Visible);
        }
        if visit.depth == MAX_DEPTH {
            return Err("Form face visibility remains unresolved; move the selection boundary or view slightly".into());
        }
        let children = split_tile(tile, visit.depth % 2)?;
        Ok(FaceVisibilityNode::Split(children))
    }

    fn root_corner_visible<V>(&self, proj: &Proj, region: &BoxSel, tile: &Tile, remaining: &mut usize, visible: &mut V) -> Result<bool>
    where
        V: FnMut(Vec3, &mut usize) -> Result<bool>,
    {
        // A narrow strip at a trim corner can contain a visible point while
        // the trim endpoints and the tile midpoint are hidden. Probe inward
        // at several scales before spending work on complete depth passes.
        // These probes can only supply a verified positive witness.
        let allowance = (*remaining / 16).min(16_384);
        let mut work = allowance;
        let found = (|| {
            let tensor = tile.controls.len() * (tile.degree[0] + tile.degree[1] + 2);
            for exponent in (3..=19).step_by(2) {
                let t = 0.5_f64.powi(exponent);
                for local in [[t, t], [1.0 - t, t], [t, 1.0 - t], [1.0 - t, 1.0 - t]] {
                    spend(&mut work, 16)?;
                    let uv = std::array::from_fn(|axis| tile.uv[axis][0] * (1.0 - local[axis]) + tile.uv[axis][1] * local[axis]);
                    if self.inside(uv, &mut work)? != Some(true) {
                        continue;
                    }
                    spend(&mut work, tensor + region.lasso.len() + 16)?;
                    if witness_visible(proj, region, eval_tile(tile, local)?, &mut work, visible)? {
                        return Ok(true);
                    }
                }
            }
            Ok::<_, String>(false)
        })();
        spend(remaining, allowance - work)?;
        match found {
            Err(error) if error == super::CANCELLED => Err(error),
            Ok(found) => Ok(found),
            Err(_) => Ok(false),
        }
    }
    /// Failure diagnostics use a fresh, independent work account and cannot
    /// affect the real query's answer. Report actual tensor/trim witnesses,
    /// not rendered triangle stations or authored cage coordinates.
    #[cfg(test)]
    pub(super) fn visibility_diagnostic<V, O>(&self, proj: &Proj, region: &BoxSel, budget: usize, visible: &mut V, occluded_hull: &mut O) -> String
    where
        V: FnMut(Vec3, &mut usize) -> Result<bool>,
        O: FnMut(&[HPoint], &mut usize) -> Result<bool>,
    {
        let mut remaining = budget;
        let mut details = Vec::new();
        let (mut roots, mut material, mut in_region, mut hidden, mut certified) = (0, 0, 0, 0, 0);
        let mut witness = None;
        let mut failure = None;
        'roots: for (index, tile) in self.tiles.iter().take(MAX_CONTROLS).enumerate() {
            let mut root_material = 0;
            let mut root_region = 0;
            for j in 0..=4 {
                for i in 0..=4 {
                    let local = [f64::from(i) / 4.0, f64::from(j) / 4.0];
                    let uv = std::array::from_fn(|axis| tile.uv[axis][0] * (1.0 - local[axis]) + tile.uv[axis][1] * local[axis]);
                    let probe = (|| -> Result<Option<bool>> {
                        spend(&mut remaining, tile.controls.len() * (tile.degree[0] + tile.degree[1] + 2) + region.lasso.len() + 4)?;
                        if self.inside(uv, &mut remaining)? != Some(true) {
                            return Ok(None);
                        }
                        root_material += 1;
                        material += 1;
                        let world = eval_tile(tile, local)?;
                        let projected = projected_curve::project_controls(proj, &[world, world]).map_err(str::to_owned)?;
                        let Some(screen) = projected.as_ref().and_then(|points| points.first()) else { return Ok(None) };
                        if !contains(region, [screen[0] / screen[2], screen[1] / screen[2]]) {
                            return Ok(None);
                        }
                        root_region += 1;
                        in_region += 1;
                        visible(world.project().ok_or("invalid diagnostic tensor witness")?, &mut remaining).map(Some)
                    })();
                    match probe {
                        Ok(Some(true)) => {
                            witness = Some(format!("root={index}, local={local:?}, raw_uv={uv:?}"));
                            break 'roots;
                        }
                        Ok(Some(false)) => hidden += 1,
                        Ok(None) => {}
                        Err(error) => {
                            failure = Some(error);
                            break 'roots;
                        }
                    }
                }
            }
            let certificate = occluded_hull(&tile.controls, &mut remaining);
            certified += usize::from(certificate == Ok(true));
            roots += 1;
            if index < 8 {
                details.push(format!("root={index}, raw_uv={:?}, material={root_material}, in_region={root_region}, hull={certificate:?}", tile.uv));
            }
            if let Err(error) = certificate {
                failure = Some(error);
                break;
            }
        }
        format!(
            "native={}, roots={roots}/{}, material={material}, in_region={in_region}, hidden_witnesses={hidden}, certified_roots={certified}, visible={witness:?}, failure={failure:?}, used={}: {}",
            self.native_face,
            self.tiles.len(),
            budget - remaining,
            details.join("; ")
        )
    }

    fn classify(&self, proj: &Proj, region: &BoxSel, tile: &Tile, remaining: &mut usize, depth: usize) -> Result<bool> {
        let [p, q] = tile.degree;
        let work = tile
            .controls
            .len()
            .checked_mul(p + q + 2)
            .and_then(|work| work.checked_add(region.lasso.len() * 6))
            .ok_or("Form face work size overflowed")?;
        spend(remaining, work)?;
        let projected = project_tile(proj, tile)?.ok_or("Form face projection changed its near-plane coverage")?;
        let bounds = bounds(&projected)?;
        let rect = [f64::from(region.rect.min.x), f64::from(region.rect.min.y), f64::from(region.rect.max.x), f64::from(region.rect.max.y)];
        let crossing = region.crossing && !region.is_lasso();
        if disjoint(bounds, rect) && (crossing || self.empty_tile(tile, remaining)?) {
            return Ok(!crossing);
        }
        // Containment of the full untrimmed patch also contains its material subset.
        if !crossing && region_contains_box(region, bounds) {
            return Ok(true);
        }
        let midpoint = tile.uv.map(|range| range[0] * 0.5 + range[1] * 0.5);
        for local in [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0], [0.5, 0.5]] {
            let uv = [tile.uv[0][0] * (1.0 - local[0]) + tile.uv[0][1] * local[0], tile.uv[1][0] * (1.0 - local[1]) + tile.uv[1][1] * local[1]];
            if self.inside(uv, remaining)? == Some(true) {
                let world = eval_tile(tile, local)?;
                let screen = projected_curve::project_controls(proj, &[world, world])
                    .map_err(str::to_owned)?
                    .ok_or("Form material witness is behind the camera")?;
                let point = screen.first().ok_or("missing projected Form witness")?;
                let point = [point[0] / point[2], point[1] / point[2]];
                if point.iter().any(|value| !value.is_finite()) {
                    return Err("Form material witness projection overflowed".into());
                }
                let inside = contains(region, point);
                if inside == crossing {
                    return Ok(inside);
                }
            }
        }
        // With no trim boundary in this UV rectangle, midpoint membership is a
        // certificate for the entire connected rectangle, including a hole.
        if self.trim_clear(tile.uv, remaining)? && self.inside(midpoint, remaining)? == Some(false) {
            return Ok(!crossing);
        }
        if depth == MAX_DEPTH || (bounds[2] - bounds[0]).max(bounds[3] - bounds[1]) <= SCREEN_UNCERTAINTY {
            return Err("a Form face lies too close to the selection boundary; move the boundary slightly".into());
        }
        let children = split_tile(tile, depth % 2)?;
        let left = self.classify(proj, region, &children[0], remaining, depth + 1);
        if left == Ok(crossing) {
            return left;
        }
        let right = self.classify(proj, region, &children[1], remaining, depth + 1);
        if right == Ok(crossing) {
            return right;
        }
        left.and(right)
    }
    fn qualify_join_bands(&self, proj: &Proj, region: &BoxSel, remaining: &mut usize) -> Result<()> {
        for join in &self.joins {
            spend(remaining, 8 + region.lasso.len() * 4)?;
            let corners: Vec<_> = join.world[0]
                .into_iter()
                .flat_map(|x| {
                    join.world[1].into_iter().flat_map(move |y| join.world[2].into_iter().map(move |z| HPoint::weighted(Vec3::new(x, y, z), 1.0)))
                })
                .collect();
            let Some(projected) = projected_curve::project_controls(proj, &corners).map_err(str::to_owned)? else { continue };
            let box_ = bounds(&projected)?;
            if !region_contains_box(region, box_) && !projected_curve::visibility_outside(region, box_) {
                return Err("selection boundary crosses a native Form vertex's recorded trim uncertainty".into());
            }
        }
        Ok(())
    }
    fn inside(&self, point: [f64; 2], remaining: &mut usize) -> Result<Option<bool>> {
        if point.iter().any(|value| !value.is_finite()) {
            return Err("invalid Form trim query point".into());
        }
        spend(remaining, self.joins.len())?;
        if self.joins.iter().any(|join| (0..2).all(|axis| point[axis] >= join.uv[axis][0] && point[axis] <= join.uv[axis][1])) {
            return Ok(None);
        }
        let mut parity = false;
        for curve in &self.trims {
            match ray_crosses(curve, point, remaining, 0)? {
                Some(crosses) => parity ^= crosses,
                None => return Ok(None),
            }
        }
        Ok(Some(parity))
    }
    fn trim_clear(&self, uv: [[f64; 2]; 2], remaining: &mut usize) -> Result<bool> {
        spend(remaining, self.joins.len())?;
        if self.joins.iter().any(|join| (0..2).all(|axis| uv[axis][0] <= join.uv[axis][1] && join.uv[axis][0] <= uv[axis][1])) {
            return Ok(false);
        }
        let rect = [uv[0][0], uv[1][0], uv[0][1], uv[1][1]];
        for curve in &self.trims {
            if !curve_clear(curve, rect, remaining, 0)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn empty_tile(&self, tile: &Tile, remaining: &mut usize) -> Result<bool> {
        Ok(self.trim_clear(tile.uv, remaining)? && self.inside(tile.uv.map(|range| range[0] * 0.5 + range[1] * 0.5), remaining)? == Some(false))
    }
}

enum FaceVisibilityNode {
    Visible,
    Hidden,
    Split([Tile; 2]),
}
enum FaceVisibilityPass {
    Visible,
    Hidden,
    Cutoff(Option<String>),
}

#[derive(Clone, Copy)]
struct FaceVisibilityVisit {
    root: usize,
    /// Binary heap address: 1 is the root and children are 2*n and 2*n+1.
    /// The address includes depth; depth is also checked explicitly on lookup.
    path: u32,
    depth: usize,
    limit: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FaceVisibilityProof {
    Hidden,
    Split,
}
#[derive(Clone, Copy)]
struct FaceVisibilityMemo {
    root: usize,
    path: u32,
    depth: usize,
    proof: FaceVisibilityProof,
}
struct FaceVisibilitySearch<'a> {
    remaining: &'a mut usize,
    pages: Vec<Option<Vec<FaceVisibilityMemo>>>,
}
impl FaceVisibilitySearch<'_> {
    // At most 4096 compact entries, allocated in small pages as needed. The
    // full table fits below the former 4096-HPoint tensor frontier. A collision
    // evicts a memo only, never geometry or a pending traversal obligation.
    const SLOTS: usize = 4096;
    const PAGE: usize = 64;
    fn slot(visit: FaceVisibilityVisit) -> usize {
        let mut hash = (visit.path as usize).wrapping_mul(0x9e37_79b1) ^ visit.root.wrapping_mul(0x85eb_ca6b);
        hash ^= hash >> 16;
        hash = hash.wrapping_mul(0xc2b2_ae35);
        (hash ^ (hash >> 13) ^ visit.depth.rotate_left(5)) & (Self::SLOTS - 1)
    }
    fn get(&mut self, visit: FaceVisibilityVisit) -> Result<Option<FaceVisibilityProof>> {
        spend(self.remaining, 1)?;
        let slot = Self::slot(visit);
        let entry = self.pages.get(slot / Self::PAGE).and_then(Option::as_ref).and_then(|page| page.get(slot % Self::PAGE));
        Ok(entry.filter(|entry| entry.root == visit.root && entry.path == visit.path && entry.depth == visit.depth).map(|entry| entry.proof))
    }
    fn remember(&mut self, visit: FaceVisibilityVisit, proof: FaceVisibilityProof) -> Result<()> {
        spend(self.remaining, 1)?;
        let slot = Self::slot(visit);
        if self.pages.is_empty() {
            let count = Self::SLOTS / Self::PAGE;
            // Memoization is optional. A small remaining work account can
            // continue the exact traversal without initializing a cache.
            if *self.remaining < count {
                return Ok(());
            }
            spend(self.remaining, count)?;
            self.pages.try_reserve_exact(count).map_err(|_| "native Form visibility memo allocation failed")?;
            self.pages.resize_with(count, || None);
        }
        let page = self.pages.get_mut(slot / Self::PAGE).ok_or("native Form visibility memo page is missing")?;
        if page.is_none() {
            if *self.remaining < Self::PAGE {
                return Ok(());
            }
            spend(self.remaining, Self::PAGE)?;
            let mut entries = Vec::new();
            entries.try_reserve_exact(Self::PAGE).map_err(|_| "native Form visibility memo page allocation failed")?;
            entries.resize(Self::PAGE, FaceVisibilityMemo { root: 0, path: 0, depth: 0, proof: FaceVisibilityProof::Hidden });
            *page = Some(entries);
        }
        let entry = page.as_mut().and_then(|page| page.get_mut(slot % Self::PAGE)).ok_or("native Form visibility memo entry is missing")?;
        *entry = FaceVisibilityMemo { root: visit.root, path: visit.path, depth: visit.depth, proof };
        Ok(())
    }
}

fn tensor_work<T>(u: &[f64], v: &[f64], rows: &[Vec<T>]) -> Result<usize> {
    if rows.is_empty() || rows.len() > MAX_CONTROLS || u.len() > MAX_CONTROLS || v.len() > MAX_CONTROLS {
        return Err("raw Form tensor exceeds its borrowed-size budget".into());
    }
    let columns = rows.first().map_or(0, Vec::len);
    let count = rows.len().checked_mul(columns).ok_or("raw Form tensor size overflowed")?;
    if columns == 0 || count > MAX_CONTROLS || rows.iter().any(|row| row.len() != columns) {
        return Err("raw Form tensor has an invalid or excessive coefficient shape".into());
    }
    count
        .checked_mul(4)
        .and_then(|work| work.checked_add(u.len()))
        .and_then(|work| work.checked_add(v.len()))
        .ok_or_else(|| "raw Form tensor work size overflowed".into())
}
fn preflight_surface(surface: &Surface, remaining: &mut usize) -> Result<()> {
    let work = match surface {
        Surface::Plane { .. } | Surface::PlaneThrough { .. } => 12,
        Surface::Spline { u_knots, v_knots, points } => tensor_work(u_knots, v_knots, points)?,
        Surface::RationalSpline { u_knots, v_knots, homogeneous } => tensor_work(u_knots, v_knots, homogeneous)?,
        _ => return Err("exact Form region selection requires the raw planar or NURBS skin form".into()),
    };
    spend(remaining, work)
}
fn surface_tiles(surface: &SurfaceEval, domain: [[f64; 2]; 2], remaining: &mut usize) -> Result<(Vec<Tile>, Option<NurbsSurface>)> {
    match surface {
        SurfaceEval::Eval(EvalSurface::Plane { .. }, _) => {
            let mut controls = Vec::with_capacity(4);
            for v in domain[1] {
                for u in domain[0] {
                    controls.push(HPoint::weighted(surface.point([u, v]).ok_or("raw Form plane does not evaluate")?, 1.0));
                }
            }
            Ok((vec![Tile { degree: [1, 1], uv: domain, controls }], None))
        }
        SurfaceEval::Eval(EvalSurface::Nurbs { degree, knots, ctrl, .. }, _) => {
            if degree.iter().any(|degree| *degree == 0 || *degree > MAX_DEGREE) || ctrl.len() > MAX_CONTROLS {
                return Err("Form face surface exceeds its exact query budget".into());
            }
            spend(remaining, ctrl.len() * (degree[0] + degree[1] + 2))?;
            let raw = NurbsSurface::from_homogeneous(
                *degree,
                knots.clone(),
                ctrl.iter().map(|point| HPoint { x: point[0], y: point[1], z: point[2], w: point[3] }).collect(),
            )
            .map_err(|error| error.to_string())?;
            let natural = raw.domain();
            if (0..2).any(|axis| domain[axis][0] < natural[axis][0] || domain[axis][1] > natural[axis][1]) {
                return Err("Form face trims leave the raw NURBS parameter domain".into());
            }
            let mut surface = raw.clamped().map_err(|error| error.to_string())?;
            for (axis, direction) in [Dir::S, Dir::T].into_iter().enumerate() {
                let knots = surface.knots().get(axis).ok_or("missing Form surface knot direction")?.clone();
                for run in knots.chunk_by(|a, b| a == b) {
                    if run.len() > degree[axis] + 1 {
                        return Err("Form surface has an invalid knot multiplicity".into());
                    }
                    let value = *run.first().ok_or("empty Form surface knot run")?;
                    if value <= natural[axis][0] || value >= natural[axis][1] {
                        continue;
                    }
                    for _ in run.len()..degree[axis] {
                        spend(remaining, surface.homogeneous().len() * (degree[axis] + 1))?;
                        surface = surface.insert_knot(direction, value).map_err(|error| error.to_string())?;
                        if surface.homogeneous().len() > MAX_CONTROLS {
                            return Err("Form surface Bezier extraction exceeds its control budget".into());
                        }
                    }
                }
            }
            let mut tiles = Vec::new();
            let count = surface.count();
            for (v, ys) in surface.knots()[1].windows(2).enumerate().skip(degree[1]).take(count[1] - degree[1]) {
                if ys[0] == ys[1] || ys[1] <= domain[1][0] || ys[0] >= domain[1][1] {
                    continue;
                }
                for (u, xs) in surface.knots()[0].windows(2).enumerate().skip(degree[0]).take(count[0] - degree[0]) {
                    if xs[0] == xs[1] || xs[1] <= domain[0][0] || xs[0] >= domain[0][1] {
                        continue;
                    }
                    spend(remaining, (degree[0] + 1) * (degree[1] + 1))?;
                    let mut controls = Vec::new();
                    for j in v - degree[1]..=v {
                        for i in u - degree[0]..=u {
                            controls.push(*surface.homogeneous().get(j * count[0] + i).ok_or("missing Form surface Bezier coefficient")?);
                        }
                    }
                    if !valid_controls(&controls) {
                        return Err("Form surface requires finite positive Bezier weights".into());
                    }
                    tiles.push(Tile { degree: *degree, uv: [[xs[0], xs[1]], [ys[0], ys[1]]], controls });
                }
            }
            if tiles.is_empty() {
                return Err("Form face has no exact surface tiles".into());
            }
            Ok((tiles, Some(raw)))
        }
        _ => Err("exact Form region selection requires the raw planar or NURBS skin form".into()),
    }
}
fn pcurve_pieces(curve: &Curve2, range: [f64; 2], remaining: &mut usize) -> Result<Vec<Vec<HPoint>>> {
    let (borrowed_knots, borrowed_count) = match curve {
        Curve2::Spline { knots, points } => (knots.as_slice(), points.len()),
        Curve2::RationalSpline { knots, homogeneous } => (knots.as_slice(), homogeneous.len()),
    };
    if borrowed_count > MAX_CONTROLS || borrowed_knots.len() > MAX_CONTROLS {
        return Err("raw Form pcurve exceeds its borrowed-size budget".into());
    }
    let degree = borrowed_count.checked_add(1).and_then(|count| borrowed_knots.len().checked_sub(count)).ok_or("invalid raw Form pcurve degree")?;
    if degree == 0 || degree > MAX_DEGREE || borrowed_count <= degree {
        return Err("invalid raw Form pcurve coefficient shape".into());
    }
    let work = borrowed_count.checked_mul(4).and_then(|work| work.checked_add(borrowed_knots.len())).ok_or("raw Form pcurve work size overflowed")?;
    spend(remaining, work)?;
    let (mut knots, mut controls) = match curve {
        Curve2::Spline { knots, points } => {
            (knots.clone(), points.iter().map(|point| HPoint::weighted(Vec3::new(point[0], point[1], 0.0), 1.0)).collect::<Vec<_>>())
        }
        Curve2::RationalSpline { knots, homogeneous } => {
            (knots.clone(), homogeneous.iter().map(|point| HPoint { x: point[0], y: point[1], z: 0.0, w: point[2] }).collect())
        }
    };
    let degree = knots.len().checked_sub(controls.len() + 1).ok_or("invalid raw Form pcurve degree")?;
    if degree == 0
        || degree > MAX_DEGREE
        || controls.len() <= degree
        || !valid_controls(&controls)
        || knots.iter().any(|value| !value.is_finite())
        || knots.windows(2).any(|pair| pair[0] > pair[1])
        || !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] >= range[1]
    {
        return Err("invalid bounded raw Form pcurve".into());
    }
    let low = *knots.get(degree).ok_or("missing raw Form pcurve domain start")?;
    let high = *knots.get(controls.len()).ok_or("missing raw Form pcurve domain end")?;
    if low >= high {
        return Err("empty raw Form pcurve domain".into());
    }
    // Canonical B-rep permits endpoint interval rounding. As geometry::on_pcurve,
    // read an intrinsic edge parameter outside the saved pcurve at its endpoint.
    // Constant clamped tails add no curve image or winding; no UV/3D point is moved.
    let range = [range[0].clamp(low, high), range[1].clamp(low, high)];
    if range[0] >= range[1] {
        return Err("raw Form pcurve has no nonconstant edge interval".into());
    }
    for endpoint in range {
        let multiplicity = knots.iter().filter(|value| **value == endpoint).count();
        for _ in multiplicity..=degree {
            spend(remaining, controls.len() * (degree + 1))?;
            (knots, controls) = insert_knot(degree, &knots, &controls, endpoint).map_err(|error| error.to_string())?;
        }
    }
    let first = knots.iter().position(|value| *value == range[0]).ok_or("missing Form trim start knot")?;
    let last = knots.iter().rposition(|value| *value == range[1]).ok_or("missing Form trim end knot")? + 1;
    let count = (last - first).checked_sub(degree + 1).ok_or("invalid Form trim restriction")?;
    controls = controls.get(first..first + count).ok_or("missing restricted Form trim coefficients")?.to_vec();
    knots = knots.get(first..last).ok_or("missing restricted Form trim knots")?.to_vec();
    let interior = knots.clone();
    for run in interior.chunk_by(|a, b| a == b) {
        let knot = *run.first().ok_or("empty Form trim knot run")?;
        if knot <= range[0] || knot >= range[1] {
            continue;
        }
        for _ in run.len()..degree {
            spend(remaining, controls.len() * (degree + 1))?;
            (knots, controls) = insert_knot(degree, &knots, &controls, knot).map_err(|error| error.to_string())?;
            if controls.len() > MAX_CONTROLS {
                return Err("Form trim Bezier extraction exceeds its control budget".into());
            }
        }
    }
    let curve = CurveN { degree, knots, ctrl: controls.iter().map(|point| [point.x, point.y, point.z, point.w]).collect() };
    Ok(curve_pieces::bezier_pieces(&curve, remaining).map_err(str::to_owned)?.into_iter().map(|piece| piece.controls).collect())
}

fn uv_endpoint(pieces: &[Vec<HPoint>], first: bool) -> Result<[f64; 2]> {
    let piece = if first { pieces.first() } else { pieces.last() }.ok_or("empty Form trim restriction")?;
    let point = if first { piece.first() } else { piece.last() }.and_then(|point| point.project()).ok_or("invalid Form trim endpoint")?;
    Ok([point.x, point.y])
}
// Outward intervals certify the inverse plane metric even when raw axes
// are skew or differently scaled. An unresolved determinant is a refusal,
// rather than a fixed epsilon pretending to bound an ill-conditioned solve.
#[derive(Clone, Copy)]
struct Bound {
    lo: f64,
    hi: f64,
}
impl Bound {
    fn at(value: f64) -> Self {
        Self { lo: value, hi: value }
    }
    fn whole() -> Self {
        Self { lo: f64::NEG_INFINITY, hi: f64::INFINITY }
    }
    fn finite(self) -> bool {
        self.lo.is_finite() && self.hi.is_finite() && self.lo <= self.hi
    }
    fn add(self, other: Self) -> Self {
        Self { lo: (self.lo + other.lo).next_down(), hi: (self.hi + other.hi).next_up() }
    }
    fn sub(self, other: Self) -> Self {
        self.add(Self { lo: -other.hi, hi: -other.lo })
    }
    fn mul(self, other: Self) -> Self {
        if !self.finite() || !other.finite() {
            return Self::whole();
        }
        let products = [self.lo * other.lo, self.lo * other.hi, self.hi * other.lo, self.hi * other.hi];
        if products.iter().any(|value| !value.is_finite()) {
            return Self::whole();
        }
        Self {
            lo: products.iter().copied().fold(f64::INFINITY, f64::min).next_down(),
            hi: products.iter().copied().fold(f64::NEG_INFINITY, f64::max).next_up(),
        }
    }
    fn square(self) -> Self {
        if !self.finite() {
            return Self::whole();
        }
        let low = if self.lo <= 0.0 && self.hi >= 0.0 { 0.0 } else { self.lo.abs().min(self.hi.abs()) };
        let high = self.lo.abs().max(self.hi.abs());
        Self { lo: (low * low).next_down().max(0.0), hi: (high * high).next_up() }
    }
    fn div(self, other: Self) -> Self {
        if !other.finite() || other.lo <= 0.0 && other.hi >= 0.0 {
            return Self::whole();
        }
        self.mul(Self { lo: (1.0 / other.hi).next_down(), hi: (1.0 / other.lo).next_up() })
    }
    fn sqrt(self) -> Self {
        if !self.finite() || self.lo < 0.0 { Self::whole() } else { Self { lo: self.lo.sqrt().next_down().max(0.0), hi: self.hi.sqrt().next_up() } }
    }
}
fn vector_bounds(point: Vec3) -> [Bound; 3] {
    [point.x, point.y, point.z].map(Bound::at)
}
fn dot_bounds(a: [Bound; 3], b: [Bound; 3]) -> Bound {
    a.into_iter().zip(b).fold(Bound::at(0.0), |sum, (a, b)| sum.add(a.mul(b)))
}
struct JoinOutput<'a> {
    curves: &'a mut Vec<Vec<HPoint>>,
    joins: &'a mut Vec<JoinBand>,
}
struct NativeJoin<'a> {
    point: Vec3,
    vertex_tolerance: f64,
    edge_tolerances: [f64; 2],
    tiles: &'a [Tile],
}
fn close_native_uv(
    a: [f64; 2],
    b: [f64; 2],
    surface: &SurfaceEval,
    spline: Option<&NurbsSurface>,
    native: NativeJoin<'_>,
    output: JoinOutput<'_>,
    remaining: &mut usize,
) -> Result<()> {
    let JoinOutput { curves: trims, joins } = output;
    if a.iter().chain(b.iter()).any(|value| !value.is_finite()) {
        return Err("native Form join endpoints must be finite".into());
    }
    if a == b {
        return Ok(());
    }
    if !matches!(surface, SurfaceEval::Eval(EvalSurface::Plane { .. }, _)) && close_uv(a, b, spline, trims, remaining).is_ok() {
        return Ok(());
    }
    spend(remaining, 128)?;
    if !native.point.is_finite()
        || !native.vertex_tolerance.is_finite()
        || native.vertex_tolerance < 0.0
        || native.edge_tolerances.iter().any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err("invalid native Form join tolerance".into());
    }
    let radius = Bound::at(native.vertex_tolerance).add(Bound::at(2.0).mul(Bound::at(native.edge_tolerances[0].max(native.edge_tolerances[1]))));
    if !radius.finite() || radius.lo < 0.0 {
        return Err("invalid native Form join radius".into());
    }
    let SurfaceEval::Eval(EvalSurface::Plane { origin, du, dv }, _) = surface else {
        let spline = spline.ok_or("raw nonplanar Form join has no retained exact tensor chart")?;
        return close_nurbs_native_uv(a, b, spline, &native, radius, JoinOutput { curves: trims, joins }, remaining);
    };
    let origin = vector_bounds(from_kernel(*origin));
    let du = vector_bounds(from_kernel(*du));
    let dv = vector_bounds(from_kernel(*dv));
    let point = vector_bounds(native.point);
    let lifted = |uv: [f64; 2]| std::array::from_fn(|axis| origin[axis].add(du[axis].mul(Bound::at(uv[0]))).add(dv[axis].mul(Bound::at(uv[1]))));
    let first: [Bound; 3] = lifted(a);
    let last: [Bound; 3] = lifted(b);
    for end in [first, last] {
        let delta: [Bound; 3] = std::array::from_fn(|axis| end[axis].sub(point[axis]));
        let squared = delta.into_iter().fold(Bound::at(0.0), |sum, value| sum.add(value.square()));
        let distance = Bound { lo: squared.lo.max(0.0), hi: squared.hi }.sqrt();
        if !distance.finite() || distance.hi > radius.lo {
            return Err("raw planar Form pcurve endpoints exceed their certified shared native vertex tolerance".into());
        }
    }
    // Endpoints and the full affine connector are in the native tolerance
    // ball. Bound its entire inverse UV ellipse with the certified dual metric.
    let uu = dot_bounds(du, du);
    let vv = dot_bounds(dv, dv);
    let uv_dot = dot_bounds(du, dv);
    let det = uu.mul(vv).sub(uv_dot.square());
    if !det.finite() || det.lo <= 0.0 {
        return Err("raw Form join plane metric is singular or cannot be certified".into());
    }
    let delta = std::array::from_fn(|axis| point[axis].sub(origin[axis]));
    let a_dot = dot_bounds(delta, du);
    let b_dot = dot_bounds(delta, dv);
    let centers = [a_dot.mul(vv).sub(b_dot.mul(uv_dot)).div(det), b_dot.mul(uu).sub(a_dot.mul(uv_dot)).div(det)];
    let widths = [radius.mul(vv.div(det).sqrt()), radius.mul(uu.div(det).sqrt())];
    if centers.iter().chain(&widths).any(|bound| !bound.finite()) {
        return Err("native Form join dual bounds overflowed".into());
    }
    let uv: [[f64; 2]; 2] = std::array::from_fn(|axis| {
        [
            centers[axis].sub(Bound::at(widths[axis].hi)).lo.min(a[axis]).min(b[axis]),
            centers[axis].add(Bound::at(widths[axis].hi)).hi.max(a[axis]).max(b[axis]),
        ]
    });
    let world: [[f64; 2]; 3] = std::array::from_fn(|axis| {
        [point[axis].sub(radius).lo.min(first[axis].lo).min(last[axis].lo), point[axis].add(radius).hi.max(first[axis].hi).max(last[axis].hi)]
    });
    if uv.iter().flatten().chain(world.iter().flatten()).any(|value| !value.is_finite()) {
        return Err("native Form join uncertainty overflowed".into());
    }
    joins.push(JoinBand { uv, world });
    trims.push(vec![HPoint::weighted(Vec3::new(a[0], a[1], 0.0), 1.0), HPoint::weighted(Vec3::new(b[0], b[1], 0.0), 1.0)]);
    Ok(())
}
/// The topology-authorized connector is only a parity closure. Its complete
/// world image must lie in the native tolerance ball; ALL possible chart
/// preimages of that ball are withheld from exact material/boundary decisions.
fn close_nurbs_native_uv(
    a: [f64; 2],
    b: [f64; 2],
    surface: &NurbsSurface,
    native: &NativeJoin<'_>,
    radius: Bound,
    output: JoinOutput<'_>,
    remaining: &mut usize,
) -> Result<()> {
    if native.tiles.is_empty() {
        return Err("native Form join has no retained tensor tiles".into());
    }
    let natural = surface.domain();
    let mut rectangle = std::array::from_fn(|axis| [a[axis].min(b[axis]), a[axis].max(b[axis])]);
    for axis in 0..2 {
        let slack = solidcraft_engine::geom::bspline::DOMAIN_ROUNDING * f64::EPSILON * natural[axis][0].abs().max(natural[axis][1].abs()).max(1.0);
        if rectangle[axis][0] < natural[axis][0] - slack || rectangle[axis][1] > natural[axis][1] + slack {
            return Err("native Form join leaves its raw tensor domain".into());
        }
        // Match the canonical raw evaluator's endpoint read. The saved trim
        // endpoints are retained; only its constant clamped surface tails are
        // used for the physical image certificate.
        rectangle[axis] = rectangle[axis].map(|value| value.clamp(natural[axis][0], natural[axis][1]));
    }
    let center = vector_bounds(native.point);
    let radius_squared = radius.square();
    let mut chart = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    for tile in native.tiles {
        for (axis, range) in chart.iter_mut().enumerate() {
            range[0] = range[0].min(tile.uv[axis][0]);
            range[1] = range[1].max(tile.uv[axis][1]);
        }
    }
    if (0..2).any(|axis| rectangle[axis][0] < chart[axis][0] || rectangle[axis][1] > chart[axis][1]) {
        return Err("native Form join escaped its retained tensor chart cover".into());
    }
    let mut restrictions = 0usize;
    for tile in native.tiles {
        spend(remaining, 1)?;
        if (0..2).any(|axis| rectangle[axis][1] < tile.uv[axis][0] || rectangle[axis][0] > tile.uv[axis][1]) {
            continue;
        }
        let connector = tile_rectangle_controls(tile, rectangle, remaining)?;
        restrictions += 1;
        for control in &connector {
            spend(remaining, 12)?;
            let point = [control.x, control.y, control.z].map(|x| Bound::at(x).div(Bound::at(control.w)));
            let squared = (0..3).fold(Bound::at(0.0), |sum, axis| sum.add(point[axis].sub(center[axis]).square()));
            if !squared.finite() || squared.hi > radius_squared.lo {
                return Err("raw nonplanar Form join exceeds its recorded shared native vertex tolerance".into());
            }
        }
    }
    if restrictions == 0 {
        return Err("native Form join has no exact covered connector restriction".into());
    }
    let mut uv: [[f64; 2]; 2] = std::array::from_fn(|axis| [a[axis].min(b[axis]), a[axis].max(b[axis])]);
    let mut retained = 0usize;
    for root in native.tiles {
        spend(remaining, root.controls.len())?;
        let mut pending = vec![(root.clone(), 0usize)];
        while let Some((tile, depth)) = pending.pop() {
            spend(remaining, tile.controls.len().saturating_mul(8))?;
            let bounds = tile_world_bounds(&tile)?;
            if ball_misses_box(bounds, center, radius_squared) {
                continue;
            }
            let diameter = (0..3).fold(Bound::at(0.0), |sum, axis| sum.add(Bound::at(bounds[axis].hi).sub(Bound::at(bounds[axis].lo)).square()));
            if diameter.hi <= radius_squared.lo || depth == MAX_DEPTH {
                retained += 1;
                if retained > MAX_CONTROLS {
                    return Err("native Form join preimage cover exceeds its tile budget".into());
                }
                for (axis, range) in uv.iter_mut().enumerate() {
                    range[0] = range[0].min(tile.uv[axis][0]);
                    range[1] = range[1].max(tile.uv[axis][1]);
                }
                continue;
            }
            // Pick the split whose still-possible children have the tighter
            // world cover. In particular a collapsed side refines across its
            // width instead of exponentially subdividing the whole pole.
            spend(remaining, tile.controls.len().saturating_mul(2 * (tile.degree[0] + tile.degree[1] + 2) + 8))?;
            let choices = [split_tile(&tile, 0)?, split_tile(&tile, 1)?];
            let mut best = None;
            for children in choices {
                let mut score = 0.0_f64;
                let mut possible = Vec::new();
                for child in children {
                    let bounds = tile_world_bounds(&child)?;
                    if ball_misses_box(bounds, center, radius_squared) {
                        continue;
                    }
                    score += bounds.iter().map(|range| range.hi - range.lo).sum::<f64>();
                    possible.push(child);
                }
                if !score.is_finite() {
                    return Err("native Form join preimage bounds overflowed".into());
                }
                if best.as_ref().is_none_or(|(old, _)| score < *old) {
                    best = Some((score, possible));
                }
            }
            let (_, children) = best.ok_or("native Form join preimage lost its subdivision choice")?;
            pending.extend(children.into_iter().map(|child| (child, depth + 1)));
        }
    }
    let world = std::array::from_fn(|axis| [center[axis].sub(radius).lo, center[axis].add(radius).hi]);
    if uv.iter().flatten().chain(world.iter().flatten()).any(|x| !x.is_finite()) {
        return Err("native Form join uncertainty bounds overflowed".into());
    }
    output.joins.push(JoinBand { uv, world });
    output.curves.push(vec![HPoint::weighted(Vec3::new(a[0], a[1], 0.0), 1.0), HPoint::weighted(Vec3::new(b[0], b[1], 0.0), 1.0)]);
    Ok(())
}
/// Tensor de Casteljau restriction of a UV rectangle, including zero-width
/// sides. Bounds round the normalized parameters outward; no knot snapping,
/// chord substitution or inverse-nearest-point repair is involved.
fn tile_rectangle_controls(tile: &Tile, rectangle: [[f64; 2]; 2], remaining: &mut usize) -> Result<Vec<HPoint>> {
    use solidcraft_engine::geom::bspline::{bezier_point, bezier_segment};
    let [p, q] = tile.degree;
    if p == 0 || q == 0 || p > MAX_DEGREE || q > MAX_DEGREE || tile.controls.len() != (p + 1) * (q + 1) || !valid_controls(&tile.controls) {
        return Err("invalid native Form connector tensor".into());
    }
    spend(remaining, tile.controls.len().saturating_mul(2 * (p + q + 2)))?;
    let ranges: [[f64; 2]; 2] = std::array::from_fn(|axis| {
        let width = Bound::at(tile.uv[axis][1]).sub(Bound::at(tile.uv[axis][0]));
        let low = Bound::at(rectangle[axis][0].max(tile.uv[axis][0])).sub(Bound::at(tile.uv[axis][0])).div(width);
        let high = Bound::at(rectangle[axis][1].min(tile.uv[axis][1])).sub(Bound::at(tile.uv[axis][0])).div(width);
        [low.lo.max(0.0), high.hi.min(1.0)]
    });
    if ranges.iter().any(|r| r.iter().any(|x| !x.is_finite()) || r[0] > r[1]) {
        return Err("native Form connector parameter bound is unresolved".into());
    }
    let restrict = |row: &[HPoint], range: [f64; 2]| -> Result<Vec<HPoint>> {
        let result = if range[0] == range[1] {
            vec![bezier_point(row, range[0])]
        } else {
            bezier_segment(row, range[0], range[1]).map_err(|error| error.to_string())?
        };
        if !valid_controls(&result) {
            return Err("native Form connector restriction has invalid positive weights".into());
        }
        Ok(result)
    };
    let rows = tile.controls.chunks_exact(p + 1).map(|row| restrict(row, ranges[0])).collect::<Result<Vec<_>>>()?;
    let width = rows.first().ok_or("native Form connector has no tensor rows")?.len();
    let mut result = Vec::new();
    for column in 0..width {
        let points = rows
            .iter()
            .map(|row| row.get(column).copied().ok_or("native Form connector column is incomplete"))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        result.extend(restrict(&points, ranges[1])?);
    }
    Ok(result)
}
fn tile_world_bounds(tile: &Tile) -> Result<[Bound; 3]> {
    let [p, q] = tile.degree;
    if p == 0 || q == 0 || p > MAX_DEGREE || q > MAX_DEGREE || tile.controls.len() != (p + 1) * (q + 1) || !valid_controls(&tile.controls) {
        return Err("invalid native Form join tile coefficients".into());
    }
    let mut bounds = [Bound { lo: f64::INFINITY, hi: f64::NEG_INFINITY }; 3];
    for point in &tile.controls {
        for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            let coordinate = Bound::at(value).div(Bound::at(point.w));
            if !coordinate.finite() {
                return Err("native Form join tile bound overflowed".into());
            }
            bounds[axis].lo = bounds[axis].lo.min(coordinate.lo);
            bounds[axis].hi = bounds[axis].hi.max(coordinate.hi);
        }
    }
    Ok(bounds)
}
fn ball_misses_box(bounds: [Bound; 3], center: [Bound; 3], radius_squared: Bound) -> bool {
    let squared = (0..3).fold(Bound::at(0.0), |sum, axis| sum.add(bounds[axis].sub(center[axis]).square()));
    squared.finite() && squared.lo > radius_squared.hi
}

fn close_uv(a: [f64; 2], b: [f64; 2], surface: Option<&NurbsSurface>, trims: &mut Vec<Vec<HPoint>>, remaining: &mut usize) -> Result<()> {
    if a == b {
        return Ok(());
    }
    let surface = surface.ok_or("raw planar Form pcurves do not close")?;
    let domain = surface.domain();
    let side = (0..2).any(|axis| a[axis] == b[axis] && (a[axis] == domain[axis][0] || a[axis] == domain[axis][1]));
    if !side {
        return Err("raw Form pcurves do not close in one nonperiodic chart".into());
    }
    let work =
        surface.homogeneous().len().checked_mul(surface.degree().iter().copied().max().unwrap_or(0) + 1).ok_or("Form pole work size overflowed")?;
    spend(remaining, work)?;
    let varying = usize::from(a[0] == b[0]);
    let direction = if varying == 0 { Dir::S } else { Dir::T };
    let (_, controls) = surface
        .iso_curve_segment(direction, a[1 - varying], [a[varying].min(b[varying]), a[varying].max(b[varying])])
        .map_err(|error| error.to_string())?;
    let first = controls.first().and_then(|point| point.project()).ok_or("invalid collapsed Form pole")?;
    if controls.iter().any(|point| point.project() != Some(first)) {
        return Err("raw Form trim gap is not an exact collapsed surface side".into());
    }
    trims.push(vec![HPoint::weighted(Vec3::new(a[0], a[1], 0.0), 1.0), HPoint::weighted(Vec3::new(b[0], b[1], 0.0), 1.0)]);
    Ok(())
}
fn projected_bounds(controls: &[HPoint]) -> Result<[f64; 4]> {
    let mut result = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for point in controls {
        let point = point.project().ok_or("Form rational bound is nonfinite")?;
        result[0] = result[0].min(point.x);
        result[1] = result[1].min(point.y);
        result[2] = result[2].max(point.x);
        result[3] = result[3].max(point.y);
    }
    if result.iter().any(|value| !value.is_finite()) {
        return Err("empty Form rational bound".into());
    }
    Ok(result)
}
/// Ordinary fitted faces have exact unit-weight isoparametric trim segments.
/// Their boundary and winding can be decided without subdividing near a side.
fn axis_aligned_trim(controls: &[HPoint]) -> Option<[[f64; 2]; 2]> {
    let [a, b] = controls else { return None };
    (a.w == 1.0 && b.w == 1.0 && a.is_finite() && b.is_finite() && (a.x == b.x || a.y == b.y)).then_some([[a.x, a.y], [b.x, b.y]])
}
fn ray_crosses(controls: &[HPoint], query: [f64; 2], remaining: &mut usize, depth: usize) -> Result<Option<bool>> {
    spend(remaining, controls.len() * (controls.len() + 1) / 2)?;
    if let Some([a, b]) = axis_aligned_trim(controls) {
        if (0..2).all(|axis| a[axis].min(b[axis]) <= query[axis] && query[axis] <= a[axis].max(b[axis])) {
            return Ok(None);
        }
        return Ok(Some(a[0] > query[0] && (a[1] > query[1]) != (b[1] > query[1])));
    }
    for endpoint in [controls.first(), controls.last()].into_iter().flatten() {
        let point = endpoint.project().ok_or("invalid exact Form trim endpoint")?;
        if [point.x, point.y] == query {
            return Ok(None);
        }
    }
    let bbox = projected_bounds(controls)?;
    if bbox[2] < query[0] || bbox[3] < query[1] || bbox[1] > query[1] {
        return Ok(Some(false));
    }
    if bbox[0] > query[0] {
        let a = controls.first().ok_or("empty Form trim curve")?;
        let b = controls.last().ok_or("empty Form trim curve")?;
        return Ok(Some((a.y / a.w > query[1]) != (b.y / b.w > query[1])));
    }
    if depth == MAX_DEPTH {
        return Ok(None);
    }
    let (left, right) = split_curve(controls)?;
    Ok(match (ray_crosses(&left, query, remaining, depth + 1)?, ray_crosses(&right, query, remaining, depth + 1)?) {
        (Some(a), Some(b)) => Some(a ^ b),
        _ => None,
    })
}
fn curve_clear(controls: &[HPoint], rect: [f64; 4], remaining: &mut usize, depth: usize) -> Result<bool> {
    spend(remaining, controls.len() * (controls.len() + 1) / 2)?;
    let bbox = projected_bounds(controls)?;
    if disjoint(bbox, rect) {
        return Ok(true);
    }
    if axis_aligned_trim(controls).is_some() || depth == 12 || (bbox[0] >= rect[0] && bbox[1] >= rect[1] && bbox[2] <= rect[2] && bbox[3] <= rect[3])
    {
        return Ok(false);
    }
    let (left, right) = split_curve(controls)?;
    Ok(curve_clear(&left, rect, remaining, depth + 1)? && curve_clear(&right, rect, remaining, depth + 1)?)
}
fn witness_visible<V>(proj: &Proj, region: &BoxSel, world: HPoint, remaining: &mut usize, visible: &mut V) -> Result<bool>
where
    V: FnMut(Vec3, &mut usize) -> Result<bool>,
{
    let point = world.project().ok_or("invalid exact Form material visibility witness")?;
    let screen = projected_curve::project_controls(proj, &[world, world])
        .map_err(str::to_owned)?
        .ok_or("Form material visibility witness changed camera coverage")?;
    let screen = screen.first().ok_or("missing Form material visibility projection")?;
    if screen[2] < f64::MIN_POSITIVE {
        return Err("Form material visibility witness has a vanishing projected weight".into());
    }
    let screen = [screen[0] / screen[2], screen[1] / screen[2]];
    if screen.iter().any(|v| !v.is_finite()) {
        return Err("Form material visibility projection overflowed".into());
    }
    if contains(region, screen) { visible(point, remaining) } else { Ok(false) }
}
fn split_curve(controls: &[HPoint]) -> Result<(Vec<HPoint>, Vec<HPoint>)> {
    if !valid_controls(controls) {
        return Err("invalid Form Bezier coefficients".into());
    }
    let mut row = controls.to_vec();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    while let (Some(a), Some(b)) = (row.first().copied(), row.last().copied()) {
        left.push(a);
        right.push(b);
        row = row.windows(2).map(|pair| pair[0] * 0.5 + pair[1] * 0.5).collect();
    }
    right.reverse();
    Ok((left, right))
}
fn split_tile(tile: &Tile, axis: usize) -> Result<[Tile; 2]> {
    let [p, q] = tile.degree;
    let mut nets = [Vec::new(), Vec::new()];
    if axis == 0 {
        for row in tile.controls.chunks_exact(p + 1) {
            let (left, right) = split_curve(row)?;
            nets[0].extend(left);
            nets[1].extend(right);
        }
    } else {
        nets = [vec![HPoint::ZERO; tile.controls.len()], vec![HPoint::ZERO; tile.controls.len()]];
        for i in 0..=p {
            let column = (0..=q)
                .map(|j| tile.controls.get(j * (p + 1) + i).copied().ok_or("missing Form surface column"))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let (left, right) = split_curve(&column)?;
            for (half, points) in [left, right].into_iter().enumerate() {
                for (j, point) in points.into_iter().enumerate() {
                    *nets[half].get_mut(j * (p + 1) + i).ok_or("missing split Form surface coefficient")? = point;
                }
            }
        }
    }
    let middle = tile.uv[axis][0] * 0.5 + tile.uv[axis][1] * 0.5;
    let mut low = tile.uv;
    let mut high = tile.uv;
    low[axis][1] = middle;
    high[axis][0] = middle;
    let [left, right] = nets;
    Ok([Tile { degree: tile.degree, uv: low, controls: left }, Tile { degree: tile.degree, uv: high, controls: right }])
}
fn eval_tile(tile: &Tile, local: [f64; 2]) -> Result<HPoint> {
    let eval = |controls: &[HPoint], fraction: f64| -> Result<HPoint> {
        let mut row = controls.to_vec();
        while row.len() > 1 {
            row = row.windows(2).map(|pair| pair[0] * (1.0 - fraction) + pair[1] * fraction).collect();
        }
        row.first().copied().filter(|point| point.is_finite() && point.w >= f64::MIN_POSITIVE).ok_or_else(|| "invalid Form surface witness".into())
    };
    let rows = tile.controls.chunks_exact(tile.degree[0] + 1).map(|row| eval(row, local[0])).collect::<Result<Vec<_>>>()?;
    eval(&rows, local[1])
}
fn project_tile(proj: &Proj, tile: &Tile) -> Result<Option<Vec<[f64; 3]>>> {
    let mut projected = Vec::new();
    let mut front = 0;
    let mut behind = 0;
    for row in tile.controls.chunks_exact(tile.degree[0] + 1) {
        match projected_curve::project_controls(proj, row).map_err(str::to_owned)? {
            Some(row) => {
                front += 1;
                projected.extend(row);
            }
            None => behind += 1,
        }
    }
    if front != 0 && behind != 0 {
        return Err("the Form face crosses the camera near plane; rotate or zoom out".into());
    }
    if front == 0 {
        return Ok(None);
    }
    if projected.iter().any(|point| point[2] < f64::MIN_POSITIVE) {
        return Err("Form surface projection has vanishing rational weights".into());
    }
    Ok(Some(projected))
}
fn bounds(controls: &[[f64; 3]]) -> Result<[f64; 4]> {
    projected_bounds(&controls.iter().map(|point| HPoint { x: point[0], y: point[1], z: 0.0, w: point[2] }).collect::<Vec<_>>())
}
fn disjoint(a: [f64; 4], b: [f64; 4]) -> bool {
    a[2] < b[0] || a[3] < b[1] || a[0] > b[2] || a[1] > b[3]
}
fn contains(region: &BoxSel, point: [f64; 2]) -> bool {
    let rect = [f64::from(region.rect.min.x), f64::from(region.rect.min.y), f64::from(region.rect.max.x), f64::from(region.rect.max.y)];
    if point.iter().any(|value| !value.is_finite()) || point[0] < rect[0] || point[1] < rect[1] || point[0] > rect[2] || point[1] > rect[3] {
        return false;
    }
    if !region.is_lasso() {
        return true;
    }
    let mut inside = false;
    for (a, b) in region.lasso.iter().zip(region.lasso.iter().cycle().skip(1)).take(region.lasso.len()) {
        let (a, b) = ([f64::from(a.x), f64::from(a.y)], [f64::from(b.x), f64::from(b.y)]);
        if (a[1] > point[1]) != (b[1] > point[1]) && point[0] < (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
    }
    inside
}
fn region_contains_box(region: &BoxSel, bounds: [f64; 4]) -> bool {
    if !contains(region, [bounds[0], bounds[1]]) || !contains(region, [bounds[2], bounds[3]]) {
        return false;
    }
    if !region.is_lasso() {
        return true;
    }
    region
        .lasso
        .iter()
        .zip(region.lasso.iter().cycle().skip(1))
        .take(region.lasso.len())
        .all(|(a, b)| !segment_touches_box([f64::from(a.x), f64::from(a.y)], [f64::from(b.x), f64::from(b.y)], bounds))
}
fn segment_touches_box(a: [f64; 2], b: [f64; 2], rect: [f64; 4]) -> bool {
    let (mut low, mut high) = (0.0_f64, 1.0_f64);
    for axis in 0..2 {
        let delta = b[axis] - a[axis];
        if delta == 0.0 {
            if a[axis] < rect[axis] || a[axis] > rect[axis + 2] {
                return false;
            }
        } else {
            let x = (rect[axis] - a[axis]) / delta;
            let y = (rect[axis + 2] - a[axis]) / delta;
            low = low.max(x.min(y));
            high = high.min(x.max(y));
            if low > high {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_visibility_searches_interior_beyond_the_initial_hidden_witnesses() {
        let face = square_with_hole();
        for area in [
            region(false, [-1.0, -1.0], [21.0, 21.0]),
            region(true, [-1.0, -1.0], [21.0, 21.0]),
            BoxSel::lasso(&[pos2(-1.0, -1.0), pos2(21.0, -1.0), pos2(21.0, 21.0), pos2(-1.0, 21.0)]),
        ] {
            let mut visible = |point: Vec3, budget: &mut usize| {
                spend(budget, 1)?;
                Ok(point.x > 4.9 && point.x < 5.1 && point.y > 9.9 && point.y < 10.1)
            };
            let mut uncertified = |_: &[HPoint], budget: &mut usize| {
                spend(budget, 1)?;
                Ok(false)
            };
            assert_eq!(face.has_visible(&projection(), &area, &mut 1_000_000, &mut visible, &mut uncertified), Ok(true));
        }
    }
    #[test]
    fn root_corner_visibility_uses_exact_trimmed_material_before_depth_passes() {
        let mut face = square_with_hole();
        let area = region(false, [-1.0, -1.0], [21.0, 21.0]);
        let near_corner = |point: Vec3| (0.00015..0.00016).contains(&point.x) && (0.00015..0.00016).contains(&point.y);
        let mut visible = |point: Vec3, _: &mut usize| Ok(near_corner(point));
        let mut hull_calls = 0;
        let mut uncertified = |_: &[HPoint], _: &mut usize| {
            hull_calls += 1;
            Ok(false)
        };
        assert_eq!(face.has_visible(&projection(), &area, &mut 64_000, &mut visible, &mut uncertified), Ok(true));
        assert_eq!(hull_calls, 1, "the root witness avoids subdivision");

        let mut cancelled = |point: Vec3, _: &mut usize| {
            if near_corner(point) { Err(super::super::CANCELLED.into()) } else { Ok(false) }
        };
        assert_eq!(face.has_visible(&projection(), &area, &mut 64_000, &mut cancelled, &mut |_, _| Ok(false)), Err(super::super::CANCELLED.into()));

        face.trims.extend(wire(&[[0.00014, 0.00014], [0.00017, 0.00014], [0.00017, 0.00017], [0.00014, 0.00017]]));
        assert_eq!(face.root_corner_visible(&projection(), &area, &face.tiles[0], &mut 64_000, &mut visible), Ok(false));
    }
    #[test]
    fn face_visibility_holes_and_certificates_are_distinct_from_hidden_samples() {
        let face = square_with_hole();
        let mut visible = |_: Vec3, _: &mut usize| Ok(true);
        let mut uncertified = |_: &[HPoint], _: &mut usize| Ok(false);
        assert_eq!(
            face.has_visible(&projection(), &region(true, [9.0, 9.0], [11.0, 11.0]), &mut 1_000_000, &mut visible, &mut uncertified),
            Ok(false)
        );
        let mut hidden = |_: Vec3, _: &mut usize| Ok(false);
        let area = region(false, [-1.0, -1.0], [21.0, 21.0]);
        assert!(face.has_visible(&projection(), &area, &mut 1_000_000, &mut hidden, &mut uncertified).is_err());
        let mut certified = |_: &[HPoint], _: &mut usize| Ok(true);
        assert_eq!(face.has_visible(&projection(), &area, &mut 1_000_000, &mut hidden, &mut certified), Ok(false));
    }
    #[test]
    fn face_visibility_lasso_exclusion_does_not_query_outside_material() {
        let mut face = square_with_hole();
        for tile in &mut face.tiles {
            for point in &mut tile.controls {
                point.x = 9.0 * point.w + point.x * 0.1;
                point.y = 15.0 * point.w + point.y * 0.1;
            }
        }
        let lasso = BoxSel::lasso(&[
            pos2(0.0, 0.0),
            pos2(20.0, 0.0),
            pos2(20.0, 20.0),
            pos2(12.0, 20.0),
            pos2(12.0, 8.0),
            pos2(8.0, 8.0),
            pos2(8.0, 20.0),
            pos2(0.0, 20.0),
        ]);
        let mut never = |_: Vec3, _: &mut usize| -> Result<bool> { panic!("lasso-excluded face must not query visibility") };
        let mut uncertified = |_: &[HPoint], _: &mut usize| Ok(false);
        assert_eq!(face.has_visible(&projection(), &lasso, &mut 1_000_000, &mut never, &mut uncertified), Ok(false));
    }
    #[test]
    fn face_visibility_finds_folded_interior_without_a_visible_trim_in_the_region() {
        let controls = (0..3)
            .flat_map(|j| {
                (0..3).map(move |i| HPoint::weighted(Vec3::new(if i == 1 && j == 1 { 40.0 } else { 0.0 }, f64::from(j) * 10.0, f64::from(i)), 1.0))
            })
            .collect();
        let face = PreparedFace {
            native_face: 0,
            tiles: vec![Tile { degree: [2, 2], uv: [[0.0, 1.0], [0.0, 1.0]], controls }],
            joins: Vec::new(),
            trims: wire(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
        };
        let mut visible = |point: Vec3, _: &mut usize| Ok(point.x > 9.0);
        let mut uncertified = |_: &[HPoint], _: &mut usize| Ok(false);
        assert_eq!(
            face.has_visible(&projection(), &region(false, [9.0, 9.0], [11.0, 11.0]), &mut 1_000_000, &mut visible, &mut uncertified),
            Ok(true)
        );
    }
    #[test]
    fn face_visibility_propagates_callback_and_shared_budget_errors() {
        let face = square_with_hole();
        let area = region(true, [3.0, 3.0], [5.0, 5.0]);
        let mut visible = |_: Vec3, _: &mut usize| Ok(true);
        let mut uncertified = |_: &[HPoint], _: &mut usize| Ok(false);
        assert!(face.has_visible(&projection(), &area, &mut 1, &mut visible, &mut uncertified).is_err());
        let mut failed = |_: Vec3, _: &mut usize| Err("visibility callback exhausted its work".to_owned());
        assert!(face.has_visible(&projection(), &area, &mut 1_000_000, &mut failed, &mut uncertified).is_err());
        let mut hidden = |_: Vec3, _: &mut usize| Ok(false);
        let mut failed_hull = |_: &[HPoint], _: &mut usize| Err("occlusion proof exhausted its work".to_owned());
        assert!(face.has_visible(&projection(), &area, &mut 1_000_000, &mut hidden, &mut failed_hull).is_err());
    }
    use egui::{Rect, pos2};
    use solidcraft_engine::doc::kernel;
    use solidcraft_viewport::{Camera, Mat4};

    fn projection() -> Proj {
        Proj {
            cam: Camera::default(),
            vp: Mat4([[0.02, 0.0, 0.0, 0.0], [0.0, -0.02, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [-1.0, 1.0, 0.0, 1.0]]),
            rect: Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0)),
        }
    }
    fn region(crossing: bool, a: [f32; 2], b: [f32; 2]) -> BoxSel {
        BoxSel { rect: Rect::from_min_max(pos2(a[0], a[1]), pos2(b[0], b[1])), crossing, lasso: Vec::new() }
    }
    fn wire(points: &[[f64; 2]]) -> Vec<Vec<HPoint>> {
        points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .take(points.len())
            .map(|(a, b)| vec![HPoint::weighted(Vec3::new(a[0], a[1], 0.0), 1.0), HPoint::weighted(Vec3::new(b[0], b[1], 0.0), 1.0)])
            .collect()
    }
    fn square_with_hole() -> PreparedFace {
        let mut trims = wire(&[[0.0, 0.0], [20.0, 0.0], [20.0, 20.0], [0.0, 20.0]]);
        trims.extend(wire(&[[8.0, 8.0], [8.0, 12.0], [12.0, 12.0], [12.0, 8.0]]));
        PreparedFace {
            native_face: 7,
            tiles: vec![Tile {
                degree: [1, 1],
                uv: [[0.0, 20.0], [0.0, 20.0]],
                controls: vec![
                    HPoint::weighted(Vec3::new(0.0, 0.0, 0.0), 1.0),
                    HPoint::weighted(Vec3::new(20.0, 0.0, 0.0), 1.0),
                    HPoint::weighted(Vec3::new(0.0, 20.0, 0.0), 1.0),
                    HPoint::weighted(Vec3::new(20.0, 20.0, 0.0), 1.0),
                ],
            }],
            trims,
            joins: Vec::new(),
        }
    }
    #[test]
    fn face_regions_find_material_without_touching_an_outline_and_exclude_holes() {
        let face = square_with_hole();
        assert_eq!(face.native_face, 7);
        assert_eq!(face.inside([4.0, 4.0], &mut 1_000_000).unwrap(), Some(true));
        assert_eq!(face.inside([10.0, 10.0], &mut 1_000_000).unwrap(), Some(false));
        assert_eq!(face.inside([8.0, 10.0], &mut 1_000_000).unwrap(), None);
        assert!(face.takes(&projection(), &region(true, [3.0, 3.0], [5.0, 5.0]), &mut 1_000_000).unwrap());
        assert!(!face.takes(&projection(), &region(true, [9.0, 9.0], [11.0, 11.0]), &mut 1_000_000).unwrap());
        assert!(face.takes(&projection(), &region(false, [-1.0, -1.0], [21.0, 21.0]), &mut 1_000_000).unwrap());
        assert!(!face.takes(&projection(), &region(false, [-1.0, -1.0], [19.0, 21.0]), &mut 1_000_000).unwrap());
    }
    #[test]
    fn face_regions_account_for_silhouettes_inside_the_surface_parameter_domain() {
        let controls = (0..3)
            .flat_map(|j| {
                (0..3).map(move |i| HPoint::weighted(Vec3::new(if i == 1 && j == 1 { 40.0 } else { 0.0 }, f64::from(j) * 10.0, f64::from(i)), 1.0))
            })
            .collect();
        let face = PreparedFace {
            native_face: 0,
            tiles: vec![Tile { degree: [2, 2], uv: [[0.0, 1.0], [0.0, 1.0]], controls }],
            joins: Vec::new(),
            trims: wire(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
        };
        // Every projected trim is on x=0. The material's interior reaches x=10.
        assert!(face.takes(&projection(), &region(true, [9.0, 9.0], [11.0, 11.0]), &mut 1_000_000).unwrap());
        assert!(!face.takes(&projection(), &region(false, [-1.0, -1.0], [1.0, 21.0]), &mut 1_000_000).unwrap());
    }
    #[test]
    fn face_window_lasso_contains_the_material_and_rejects_a_concave_cutout() {
        let face = square_with_hole();
        let enclosing = BoxSel::lasso(&[pos2(-1.0, -1.0), pos2(21.0, -1.0), pos2(21.0, 21.0), pos2(-1.0, 21.0)]);
        assert!(face.takes(&projection(), &enclosing, &mut 1_000_000).unwrap());
        let notched = BoxSel::lasso(&[
            pos2(-1.0, -1.0),
            pos2(21.0, -1.0),
            pos2(21.0, 21.0),
            pos2(12.0, 21.0),
            pos2(12.0, 8.0),
            pos2(8.0, 8.0),
            pos2(8.0, 21.0),
            pos2(-1.0, 21.0),
        ]);
        assert!(!face.takes(&projection(), &notched, &mut 1_000_000).unwrap());
    }
    #[test]
    fn raw_native_trim_preparation_retains_face_identity_and_hole_geometry() {
        let surface = NurbsSurface::from_homogeneous(
            [1, 1],
            [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]],
            vec![
                HPoint::weighted(Vec3::new(0.0, 0.0, 0.0), 1.0),
                HPoint::weighted(Vec3::new(20.0, 0.0, 0.0), 1.0),
                HPoint::weighted(Vec3::new(0.0, 20.0, 0.0), 1.0),
                HPoint::weighted(Vec3::new(20.0, 20.0, 0.0), 1.0),
            ],
        )
        .unwrap();
        let body = kernel::nurbs_region_sheet(
            &surface,
            &[vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], vec![[0.4, 0.4], [0.4, 0.6], [0.6, 0.6], [0.6, 0.4]]],
        )
        .unwrap();
        let face = PreparedFace::prepare(&body, 0, &mut 1_000_000).unwrap();
        assert_eq!(face.native_face, 0);
        assert!(face.takes(&projection(), &region(true, [3.0, 3.0], [5.0, 5.0]), &mut 1_000_000).unwrap());
        assert!(!face.takes(&projection(), &region(true, [9.0, 9.0], [11.0, 11.0]), &mut 1_000_000).unwrap());
        assert!(PreparedFace::prepare(&body, 100, &mut 1_000_000).is_err());
    }
    #[test]
    fn straight_native_trim_stays_exact_on_and_beside_the_boundary() {
        let vertical = [[0.5, 0.0], [0.5, 1.0]].map(|[x, y]| HPoint::weighted(Vec3::new(x, y, 0.0), 1.0));
        let horizontal = [[0.0, 0.5], [1.0, 0.5]].map(|[x, y]| HPoint::weighted(Vec3::new(x, y, 0.0), 1.0));
        let left = 0.5_f64.next_down();
        let right = 0.5_f64.next_up();
        for controls in [vertical, [vertical[1], vertical[0]]] {
            assert_eq!(ray_crosses(&controls, [0.5, 0.5], &mut 3, 0).unwrap(), None);
            assert_eq!(ray_crosses(&controls, [left, 0.5], &mut 3, 0).unwrap(), Some(true));
            assert_eq!(ray_crosses(&controls, [right, 0.5], &mut 3, 0).unwrap(), Some(false));
            assert_eq!(ray_crosses(&controls, [left, 0.0], &mut 3, 0).unwrap(), Some(true));
            assert_eq!(ray_crosses(&controls, [left, 1.0], &mut 3, 0).unwrap(), Some(false));
            assert!(!curve_clear(&controls, [left, 0.25, right, 0.75], &mut 3, 0).unwrap());
            assert!(curve_clear(&controls, [right, 0.25, 0.9, 0.75], &mut 3, 0).unwrap());
        }
        assert_eq!(ray_crosses(&horizontal, [0.5, 0.5], &mut 3, 0).unwrap(), None);
        assert_eq!(ray_crosses(&horizontal, [0.5, right], &mut 3, 0).unwrap(), Some(false));
        assert!(!curve_clear(&horizontal, [0.25, left, 0.75, right], &mut 3, 0).unwrap());
    }
    #[test]
    fn rational_pcurves_are_restricted_exactly_and_closed_poles_are_certified() {
        let curve = Curve2::RationalSpline {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            homogeneous: vec![[1.0, 0.0, 1.0], [1.0, 1.0, 0.5_f64.sqrt()], [0.0, 1.0, 1.0]],
        };
        let pieces = pcurve_pieces(&curve, [0.2, 0.8], &mut 1_000_000).unwrap();
        let a = solidcraft_engine::geom::brep::pcurve::eval(&curve, 0.2).unwrap();
        let b = solidcraft_engine::geom::brep::pcurve::eval(&curve, 0.8).unwrap();
        let start = uv_endpoint(&pieces, true).unwrap();
        let end = uv_endpoint(&pieces, false).unwrap();
        assert!((start[0] - a[0]).abs() < 1e-12 && (start[1] - a[1]).abs() < 1e-12);
        assert!((end[0] - b[0]).abs() < 1e-12 && (end[1] - b[1]).abs() < 1e-12);
        let pole = NurbsSurface::from_homogeneous(
            [1, 1],
            [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]],
            vec![
                HPoint::weighted(Vec3::ZERO, 1.0),
                HPoint::weighted(Vec3::ZERO, 1.0),
                HPoint::weighted(Vec3::new(0.0, 1.0, 0.0), 1.0),
                HPoint::weighted(Vec3::new(1.0, 1.0, 0.0), 1.0),
            ],
        )
        .unwrap();
        let mut trims = Vec::new();
        close_uv([0.0, 0.0], [1.0, 0.0], Some(&pole), &mut trims, &mut 1_000_000).unwrap();
        assert_eq!(trims.len(), 1);
        assert!(close_uv([0.0, 1.0], [1.0, 1.0], Some(&pole), &mut trims, &mut 1_000_000).is_err());
    }
    #[test]
    fn raw_pcurve_reads_canonical_edge_endpoint_interval_rounding_without_moving_the_curve() {
        let drift = 1e-10;
        let curve = Curve2::Spline { knots: vec![drift, drift, 1.0 + drift, 1.0 + drift], points: vec![[0.0, 0.0], [1.0, 0.0]] };
        let pieces = pcurve_pieces(&curve, [0.0, 1.0], &mut 1_000_000).unwrap();
        let start = uv_endpoint(&pieces, true).unwrap();
        let end = uv_endpoint(&pieces, false).unwrap();
        assert_eq!(start, solidcraft_engine::geom::brep::pcurve::eval(&curve, drift).unwrap());
        assert_eq!(end, solidcraft_engine::geom::brep::pcurve::eval(&curve, 1.0).unwrap());
        assert!(end[0] < 1.0, "do not extend a canonical clamped read to a different saved endpoint");
    }
    #[test]
    fn raw_source_size_and_work_are_checked_before_homogeneous_conversion() {
        let curve = Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![[0.0, 0.0], [1.0, 0.0]] };
        assert!(pcurve_pieces(&curve, [0.0, 1.0], &mut 0).is_err());
        let oversized = Curve2::Spline { knots: vec![0.0; MAX_CONTROLS + 1], points: Vec::new() };
        assert!(pcurve_pieces(&oversized, [0.0, 1.0], &mut 0).is_err());
        let plane = Surface::Plane { origin: [0.0; 3], u_axis: [1.0, 0.0, 0.0], v_axis: [0.0, 1.0, 0.0] };
        assert!(preflight_surface(&plane, &mut 0).is_err());
        let oversized = Surface::Spline { u_knots: vec![0.0; MAX_CONTROLS + 1], v_knots: Vec::new(), points: Vec::new() };
        assert!(preflight_surface(&oversized, &mut 0).is_err());
        let ragged =
            Surface::Spline { u_knots: vec![0.0, 0.0, 1.0, 1.0], v_knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![vec![[0.0; 3]], vec![[0.0; 3]; 2]] };
        assert!(preflight_surface(&ragged, &mut 1_000_000).is_err());
    }
    #[test]
    fn rounded_nonplanar_native_joins_keep_full_preimage_and_recorded_world_uncertainty() {
        let raw = Surface::RationalSpline {
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            // Raw rows advance U, unlike the evaluator's U-fast flat array.
            // S(u,v) = (u * (1e-10 * (1-v) + v), v, 0.2*u*v).
            homogeneous: vec![vec![[0.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]], vec![[1e-10, 0.0, 0.0, 1.0], [1.0, 1.0, 0.2, 1.0]]],
        };
        let eval = SurfaceEval::new(&raw).unwrap();
        assert_eq!(eval.point([0.0, 0.0]), Some(Vec3::ZERO));
        assert_eq!(eval.point([1.0, 0.0]), Some(Vec3::new(1e-10, 0.0, 0.0)));
        assert_eq!(eval.point([0.0, 1.0]), Some(Vec3::new(0.0, 1.0, 0.0)));
        let (tiles, surface) = surface_tiles(&eval, [[0.0, 1.0], [0.0, 1.0]], &mut 1_000_000).unwrap();
        let pole_controls = tile_rectangle_controls(&tiles[0], [[0.0, 1.0], [0.0, 0.0]], &mut 1_000_000).unwrap();
        assert!(pole_controls.iter().all(|control| {
            let point = control.project().unwrap();
            // Normalizing the zero-width V side rounds its UV bound outward
            // by subnormal ulps; the true endpoint assertions above stay exact.
            point.x >= 0.0 && point.x <= 1e-10 && point.y >= 0.0 && point.y < f64::MIN_POSITIVE && point.z >= 0.0 && point.z < f64::MIN_POSITIVE
        }));
        assert!(
            close_uv([0.0, 0.0], [1.0, 0.0], surface.as_ref(), &mut Vec::new(), &mut 1_000_000).is_err(),
            "rounded pole must not be called bit-exact"
        );
        let mut trims = vec![
            vec![HPoint::weighted(Vec3::new(1.0, 0.0, 0.0), 1.0), HPoint::weighted(Vec3::new(1.0, 1.0, 0.0), 1.0)],
            vec![HPoint::weighted(Vec3::new(1.0, 1.0, 0.0), 1.0), HPoint::weighted(Vec3::new(0.0, 1.0, 0.0), 1.0)],
            vec![HPoint::weighted(Vec3::new(0.0, 1.0, 0.0), 1.0), HPoint::weighted(Vec3::new(0.0, 0.0, 0.0), 1.0)],
        ];
        let mut joins = Vec::new();
        close_native_uv(
            [0.0, 0.0],
            [1.0, 0.0],
            &eval,
            surface.as_ref(),
            NativeJoin { point: Vec3::ZERO, vertex_tolerance: 1e-7, edge_tolerances: [0.0; 2], tiles: &tiles },
            JoinOutput { curves: &mut trims, joins: &mut joins },
            &mut 4_000_000,
        )
        .unwrap();
        assert_eq!(joins.len(), 1);
        assert!(joins[0].uv[0][0] <= 0.0 && joins[0].uv[0][1] >= 1.0, "cover the complete collapsed-side preimage, not just one inverse point");
        assert!(joins[0].uv[1][1] > 0.0 && joins[0].uv[1][1] < 0.01);
        assert!(joins[0].world[0][0] <= -1e-7 && joins[0].world[0][1] >= 1e-7);
        let face = PreparedFace { native_face: 0, tiles: tiles.clone(), trims: trims.clone(), joins };
        assert_eq!(face.inside([0.5, 0.0], &mut 1_000_000).unwrap(), None);
        assert_eq!(face.inside([0.5, 0.5], &mut 1_000_000).unwrap(), Some(true));
        assert!(face.takes(&projection(), &region(false, [-1.0, -1.0], [2.0, 2.0]), &mut 1_000_000).unwrap());
        assert!(
            face.takes(&projection(), &region(true, [0.0, 0.0], [1.0, 1.0]), &mut 1_000_000).is_err(),
            "screen boundary crossing the native uncertainty is unresolved"
        );
        let before = trims.clone();
        let mut output = Vec::new();
        assert!(
            close_native_uv(
                [0.0, 1.0],
                [1.0, 1.0],
                &eval,
                surface.as_ref(),
                NativeJoin { point: Vec3::ZERO, vertex_tolerance: 1e-7, edge_tolerances: [0.0; 2], tiles: &tiles },
                JoinOutput { curves: &mut trims, joins: &mut output },
                &mut 4_000_000
            )
            .is_err()
        );
        assert_eq!(trims, before);
        assert!(output.is_empty());
    }
    #[test]
    fn grouped_face_visibility_finds_right_subtrees_later_tiles_and_later_native_pieces() {
        let mut face = square_with_hole();
        face.trims.truncate(4);
        let area = region(false, [-1.0, -1.0], [51.0, 21.0]);
        let mut visible = |point: Vec3, _: &mut usize| Ok(point.x == 15.0);
        let mut unknown = |_: &[HPoint], _: &mut usize| Ok(false);
        assert_eq!(face.has_visible(&projection(), &area, &mut 20_000, &mut visible, &mut unknown), Ok(true));
        let [left, right] = split_tile(&face.tiles[0], 0).unwrap();
        face.tiles = vec![left, right];
        assert_eq!(face.has_visible(&projection(), &area, &mut 20_000, &mut visible, &mut unknown), Ok(true));
        let mut later = square_with_hole();
        later.trims.truncate(4);
        later.native_face = 8;
        for point in &mut later.tiles[0].controls {
            point.x += 30.0 * point.w;
        }
        let mut later_visible = |point: Vec3, _: &mut usize| Ok(point.x == 40.0);
        assert_eq!(PreparedFace::group_has_visible(&[face, later], &projection(), &area, &mut 20_000, &mut later_visible, &mut unknown), Ok(true));
        let face = square_with_hole();
        let mut hidden = |_: Vec3, _: &mut usize| Ok(false);
        let mut certified = |_: &[HPoint], _: &mut usize| Ok(true);
        assert_eq!(
            PreparedFace::group_has_visible(std::slice::from_ref(&face), &projection(), &area, &mut 20_000, &mut hidden, &mut certified),
            Ok(false)
        );
        assert!(PreparedFace::group_has_visible(std::slice::from_ref(&face), &projection(), &area, &mut 20_000, &mut hidden, &mut unknown).is_err());
        assert!(PreparedFace::group_has_visible(&[], &projection(), &area, &mut 20_000, &mut hidden, &mut certified).is_err());
    }

    #[test]
    fn face_visibility_deepens_without_dropping_wide_tensor_frontiers() {
        let mut face = square_with_hole();
        face.trims.truncate(4);
        let degree = MAX_DEGREE;
        face.tiles[0] = Tile {
            degree: [degree; 2],
            uv: [[0.0, 20.0]; 2],
            controls: (0..=degree)
                .flat_map(|j| {
                    (0..=degree).map(move |i| HPoint::weighted(Vec3::new(20.0 * i as f64 / degree as f64, 20.0 * j as f64 / degree as f64, 0.0), 1.0))
                })
                .collect(),
        };
        // Root/corner/center witnesses and both depth-one centers are hidden.
        // The depth-two upper-right center is exactly visible. A 4096-control
        // breadth-first frontier discards its parent's two 1089-control nets.
        let area = region(false, [-1.0, -1.0], [21.0, 21.0]);
        let mut witness = |point: Vec3, _: &mut usize| Ok(point.x == 15.0 && point.y == 15.0);
        let mut unknown = |_: &[HPoint], _: &mut usize| Ok(false);
        assert_eq!(face.has_visible(&projection(), &area, &mut 8_000_000, &mut witness, &mut unknown), Ok(true));

        let mut hidden = |_: Vec3, _: &mut usize| Ok(false);
        let mut small_hull = |controls: &[HPoint], _: &mut usize| {
            let bbox = projected_bounds(controls)?;
            Ok(bbox[2] - bbox[0] <= 10.0 && bbox[3] - bbox[1] <= 10.0)
        };
        assert_eq!(face.has_visible(&projection(), &area, &mut 8_000_000, &mut hidden, &mut small_hull), Ok(false));
        assert!(face.has_visible(&projection(), &area, &mut 1_000_000, &mut hidden, &mut unknown).is_err());
    }

    #[test]
    fn face_visibility_keeps_failed_branches_uncertain_while_searching_later_depths() {
        let mut face = square_with_hole();
        face.trims.truncate(4);
        let area = region(false, [-1.0, -1.0], [21.0, 21.0]);
        let mut later = |point: Vec3, _: &mut usize| {
            if point.x == 5.0 { Err("left native witness is unresolved".to_owned()) } else { Ok(point.x == 15.0 && point.y == 15.0) }
        };
        let mut unknown = |_: &[HPoint], _: &mut usize| Ok(false);
        assert_eq!(face.has_visible(&projection(), &area, &mut 100_000, &mut later, &mut unknown), Ok(true));
        let mut failed = |_: Vec3, _: &mut usize| Err("native witness is unresolved".to_owned());
        let mut certified = |_: &[HPoint], _: &mut usize| Ok(true);
        assert!(face.has_visible(&projection(), &area, &mut 100_000, &mut failed, &mut certified).is_err());
    }

    #[test]
    fn face_visibility_reuses_exact_hidden_subtrees_and_split_proofs_across_depth_passes() {
        let mut face = square_with_hole();
        face.trims.truncate(4);
        let area = region(false, [-1.0, -1.0], [21.0, 21.0]);
        let mut visible = |point: Vec3, _: &mut usize| Ok(point.x == 12.5 && point.y == 17.5);
        let (mut root_calls, mut left_calls) = (0, 0);
        let mut certificate = |controls: &[HPoint], work: &mut usize| {
            // An expensive real occlusion query must not be repeated merely
            // because another branch needs one more subdivision level.
            spend(work, 1_000)?;
            let bbox = projected_bounds(controls)?;
            root_calls += usize::from(bbox == [0.0, 0.0, 20.0, 20.0]);
            left_calls += usize::from(bbox == [0.0, 0.0, 10.0, 20.0]);
            Ok(bbox[2] <= 10.0 || bbox[3] <= 10.0)
        };
        assert_eq!(face.has_visible(&projection(), &area, &mut 50_000, &mut visible, &mut certificate), Ok(true));
        assert_eq!(root_calls, 1, "structural split must be reused, not reclassified on every complete pass");
        assert_eq!(left_calls, 1, "a fully certified hidden subtree must be pruned on later passes");
    }

    #[test]
    fn face_visibility_memo_collisions_and_eviction_never_alias_root_path_or_depth() {
        let mut work = 100_000;
        let mut search = FaceVisibilitySearch { remaining: &mut work, pages: Vec::new() };
        let first = FaceVisibilityVisit { root: 3, path: 1, depth: 0, limit: 0 };
        search.remember(first, FaceVisibilityProof::Hidden).unwrap();
        assert_eq!(search.get(first).unwrap(), Some(FaceVisibilityProof::Hidden));
        let other = (4..100_000)
            .map(|root| FaceVisibilityVisit { root, ..first })
            .find(|&visit| FaceVisibilitySearch::slot(visit) == FaceVisibilitySearch::slot(first))
            .unwrap();
        assert_eq!(search.get(other).unwrap(), None, "a slot hit is not an identity match");
        search.remember(other, FaceVisibilityProof::Split).unwrap();
        assert_eq!(search.get(first).unwrap(), None, "eviction permits only recomputation");
        assert_eq!(search.get(other).unwrap(), Some(FaceVisibilityProof::Split));
        assert_eq!(search.get(FaceVisibilityVisit { path: 2, ..other }).unwrap(), None);
        assert_eq!(search.get(FaceVisibilityVisit { depth: 1, ..other }).unwrap(), None);
        // Tensor frontier storage was 4096 HPoints. The paged memo stays below
        // that same bound even when every compact slot has been allocated.
        let memo_bytes = FaceVisibilitySearch::SLOTS * std::mem::size_of::<FaceVisibilityMemo>()
            + (FaceVisibilitySearch::SLOTS / FaceVisibilitySearch::PAGE) * std::mem::size_of::<Option<Vec<FaceVisibilityMemo>>>();
        assert!(memo_bytes <= 4096 * std::mem::size_of::<HPoint>());
        let mut tiny_work = 2;
        let mut tiny = FaceVisibilitySearch { remaining: &mut tiny_work, pages: Vec::new() };
        tiny.remember(first, FaceVisibilityProof::Hidden).unwrap();
        assert!(tiny.pages.is_empty(), "memo initialization is optional for small exact queries");
        assert_eq!(tiny.get(first).unwrap(), None);
        assert!(tiny.get(first).is_err(), "memo lookups share the original work account");
    }

    #[test]
    fn rounded_planar_native_joins_use_recorded_tolerance_and_keep_boundary_uncertainty() {
        let raw = Surface::Plane { origin: [0.0; 3], u_axis: [1.0, 0.0, 0.0], v_axis: [0.0, 1.0, 0.0] };
        let surface = SurfaceEval::new(&raw).unwrap();
        let mut trims = Vec::new();
        let mut joins = Vec::new();
        close_native_uv(
            [0.0, 0.0],
            [1e-10, 0.0],
            &surface,
            None,
            NativeJoin { point: Vec3::ZERO, vertex_tolerance: 1e-9, edge_tolerances: [0.0; 2], tiles: &[] },
            JoinOutput { curves: &mut trims, joins: &mut joins },
            &mut 1_000_000,
        )
        .unwrap();
        assert_eq!(trims.len(), 1);
        assert_eq!(joins.len(), 1);
        let mut face = square_with_hole();
        face.joins = joins;
        assert_eq!(face.inside([5e-11, 0.0], &mut 1_000_000).unwrap(), None);
        assert!(face.takes(&projection(), &region(false, [-1.0, -1.0], [21.0, 21.0]), &mut 1_000_000).unwrap());
        assert!(face.takes(&projection(), &region(true, [3.0, 3.0], [5.0, 5.0]), &mut 1_000_000).unwrap());
        assert!(face.takes(&projection(), &region(true, [0.0, 0.0], [1.0, 1.0]), &mut 1_000_000).is_err());
        assert!(
            close_native_uv(
                [0.0, 0.0],
                [1e-4, 0.0],
                &surface,
                None,
                NativeJoin { point: Vec3::ZERO, vertex_tolerance: 1e-9, edge_tolerances: [0.0; 2], tiles: &[] },
                JoinOutput { curves: &mut trims, joins: &mut Vec::new() },
                &mut 1_000_000
            )
            .is_err()
        );
    }
    #[test]
    fn skew_native_join_bounds_enclose_the_dual_ellipse_and_near_singular_axes_refuse() {
        let raw = Surface::Plane { origin: [0.0; 3], u_axis: [2.0, 0.0, 0.0], v_axis: [1.0, 3.0, 0.0] };
        let surface = SurfaceEval::new(&raw).unwrap();
        let mut trims = Vec::new();
        let mut joins = Vec::new();
        close_native_uv(
            [0.0, 0.0],
            [1e-10, 0.0],
            &surface,
            None,
            NativeJoin { point: Vec3::ZERO, vertex_tolerance: 1e-9, edge_tolerances: [1e-10; 2], tiles: &[] },
            JoinOutput { curves: &mut trims, joins: &mut joins },
            &mut 1_000_000,
        )
        .unwrap();
        let radius = 1e-9 + 2.0 * 1e-10;
        let u = radius * 10.0_f64.sqrt() / 6.0;
        let v = radius / 3.0;
        assert!(joins[0].uv[0][0] <= -u && joins[0].uv[0][1] >= u);
        assert!(joins[0].uv[1][0] <= -v && joins[0].uv[1][1] >= v);
        assert!(joins[0].world[0][0] <= -radius && joins[0].world[0][1] >= radius);
        let near = Surface::Plane { origin: [0.0; 3], u_axis: [1.0, 0.0, 0.0], v_axis: [1.0, 1e-12, 0.0] };
        let surface = SurfaceEval::new(&near).unwrap();
        let before = trims.clone();
        let count = joins.len();
        assert!(
            close_native_uv(
                [0.0, 0.0],
                [1e-10, 0.0],
                &surface,
                None,
                NativeJoin { point: Vec3::ZERO, vertex_tolerance: 1e-9, edge_tolerances: [0.0; 2], tiles: &[] },
                JoinOutput { curves: &mut trims, joins: &mut joins },
                &mut 1_000_000
            )
            .is_err()
        );
        assert_eq!(trims, before);
        assert_eq!(joins.len(), count);
    }
    #[test]
    fn hostile_input_and_near_plane_cases_refuse_without_substituting_samples() {
        let face = square_with_hole();
        assert!(face.takes(&projection(), &region(true, [3.0, 3.0], [5.0, 5.0]), &mut 1).is_err());
        let mut proj = projection();
        proj.vp.0[3][3] = -1.0;
        assert!(!face.takes(&proj, &region(false, [-1.0, -1.0], [21.0, 21.0]), &mut 1_000_000).unwrap());
        let mut proj = projection();
        proj.vp.0[0][3] = 1.0;
        proj.vp.0[3][3] = -10.0;
        assert!(face.takes(&proj, &region(true, [3.0, 3.0], [5.0, 5.0]), &mut 1_000_000).is_err());
        let mut lasso = region(false, [-1.0, -1.0], [21.0, 21.0]);
        lasso.lasso = vec![pos2(f32::NAN, 0.0)];
        assert!(face.takes(&projection(), &lasso, &mut 1_000_000).is_err());
    }
}
