use super::*;
use array_macro::array;

/// Find collisions between two polygon meshes and extract interference lines.
///
/// # Details
///
/// The algorithm for mesh extraction is as follows.
/// All the triangles are projected toward the axis to form intervals.
/// By looking at the overlap of these intervals, we can narrow down the interfering triangles.
/// The overlap of the intervals is obtained by sorting the endpoints.
///
/// # Remarks
///
/// We see that the surfaces that are in contact are not interfering with each other.
/// Therefore, polygon meshes included in one plane are not interfering.
pub trait Collision {
    /// If `self` and `other` collide, then returns only one interference line.
    /// Otherwise, returns `None`.
    fn collide_with(&self, other: &PolygonMesh) -> Option<(Point3, Point3)>;
    /// Extract all interference lines between `self` and `other`.
    /// # Remarks
    /// The results is not arranged so that included lines make continuous maximal polyline curve.
    fn extract_interference(&self, other: &PolygonMesh) -> Vec<(Point3, Point3)>;
}

impl Collision for PolygonMesh {
    #[inline(always)]
    fn collide_with(&self, other: &PolygonMesh) -> Option<(Point3, Point3)> {
        are_colliding(self, other)
    }
    #[inline(always)]
    fn extract_interference(&self, other: &PolygonMesh) -> Vec<(Point3, Point3)> {
        collision(self, other)
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum EndPointType {
    Front,
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
struct EndPoint {
    entity: f64,
    r#type: EndPointType,
    segnum: usize,
    index: usize,
}

impl EndPoint {
    #[inline(always)]
    fn new(entity: f64, r#type: EndPointType, segnum: usize, index: usize) -> EndPoint {
        EndPoint {
            entity,
            r#type,
            segnum,
            index,
        }
    }
    #[inline(always)]
    fn from_seg(seg: (f64, f64), segnum: usize, index: usize) -> Vec<EndPoint> {
        vec![
            EndPoint::new(seg.0, EndPointType::Front, segnum, index),
            EndPoint::new(seg.1, EndPointType::Back, segnum, index),
        ]
    }
}

fn tri_to_seg(tri: [Point3; 3], unit: Vector3) -> (f64, f64) {
    let a = tri[0].to_vec().dot(unit);
    let b = tri[1].to_vec().dot(unit);
    let c = tri[2].to_vec().dot(unit);
    (f64::min(f64::min(a, b), c), f64::max(f64::max(a, b), c))
}

fn sorted_endpoints<I, J>(iter0: I, iter1: J, unit: Vector3) -> Vec<EndPoint>
where
    I: IntoIterator<Item = [Point3; 3]>,
    J: IntoIterator<Item = [Point3; 3]>, {
    let mut res: Vec<EndPoint> = iter0
        .into_iter()
        .enumerate()
        .filter(|(_, tri)| !(tri[1] - tri[0]).cross(tri[2] - tri[0]).so_small())
        .flat_map(|(i, tri)| EndPoint::from_seg(tri_to_seg(tri, unit), 0, i))
        .chain(
            iter1
                .into_iter()
                .enumerate()
                .filter(|(_, tri)| !(tri[1] - tri[0]).cross(tri[2] - tri[0]).so_small())
                .flat_map(|(i, tri)| EndPoint::from_seg(tri_to_seg(tri, unit), 1, i)),
        )
        .collect();
    res.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Greater));
    res
}

fn collide_seg_triangle(seg: [Point3; 2], tri: [Point3; 3]) -> Option<Point3> {
    let ab = tri[1] - tri[0];
    let bc = tri[2] - tri[1];
    let ca = tri[0] - tri[2];
    let nor = ab.cross(ca);
    if nor.so_small() {
        return None;
    }
    let ap = seg[0] - tri[0];
    let aq = seg[1] - tri[0];
    let dotapnor = ap.dot(nor);
    let dotaqnor = aq.dot(nor);
    if dotapnor * dotaqnor > 0.0 {
        return None;
    }
    let h = seg[0] + dotapnor / (dotapnor - dotaqnor) * (seg[1] - seg[0]);
    if f64::signum(ab.cross(nor).dot(h - tri[0]) + TOLERANCE2)
        + f64::signum(bc.cross(nor).dot(h - tri[1]) + TOLERANCE2)
        + f64::signum(ca.cross(nor).dot(h - tri[2]) + TOLERANCE2)
        >= 2.0
    {
        Some(h)
    } else {
        None
    }
}

fn collide_triangles(tri0: [Point3; 3], tri1: [Point3; 3]) -> Option<(Point3, Point3)> {
    let mut tuple = (None, None);
    [
        collide_seg_triangle([tri0[0], tri0[1]], tri1),
        collide_seg_triangle([tri0[1], tri0[2]], tri1),
        collide_seg_triangle([tri0[2], tri0[0]], tri1),
        collide_seg_triangle([tri1[0], tri1[1]], tri0),
        collide_seg_triangle([tri1[1], tri1[2]], tri0),
        collide_seg_triangle([tri1[2], tri1[0]], tri0),
    ]
    .into_iter()
    .for_each(|pt| match tuple {
        (None, _) => tuple.0 = pt,
        (Some(_), None) => tuple.1 = pt,
        (Some(ref mut p), Some(ref mut q)) => {
            if let Some(pt) = pt {
                let dist0 = pt.distance2(*p);
                let dist1 = pt.distance2(*q);
                let dist2 = p.distance2(*q);
                if dist2 < dist0 {
                    *q = pt;
                } else if dist2 < dist1 {
                    *p = pt;
                }
            }
        }
    });
    match tuple {
        (Some(a), Some(b)) => Some((a, b)),
        _ => None,
    }
}

fn make_pos_tri(poly: &PolygonMesh, face: [StandardVertex; 3]) -> [Point3; 3] {
    array![i => poly.positions()[face[i].pos]; 3]
}

/// Triangles of a mesh with their bounding boxes, each computed once.
fn boxed_triangles(poly: &PolygonMesh) -> Vec<([Point3; 3], BoundingBox<Point3>)> {
    poly.faces()
        .triangle_iter()
        .map(|face| {
            let tri = make_pos_tri(poly, face);
            (tri, tri.iter().collect())
        })
        .collect()
}

/// Whether two triangles' bounding boxes miss each other (touching boxes do not miss).
#[inline(always)]
fn disjoint_boxes(bdb0: &BoundingBox<Point3>, bdb1: &BoundingBox<Point3>) -> bool {
    bdb0.max()[0] < bdb1.min()[0]
        || bdb1.max()[0] < bdb0.min()[0]
        || bdb0.max()[1] < bdb1.min()[1]
        || bdb1.max()[1] < bdb0.min()[1]
        || bdb0.max()[2] < bdb1.min()[2]
        || bdb1.max()[2] < bdb0.min()[2]
}

/// SolidCraft: whether some triangle of each mesh meets the box of the other mesh. When not, no two
/// triangles' boxes overlap and [`candidate_pairs`] finds none; the sweep it saves dominated
/// booleans whose finely divided faces mostly lie apart (every face of one solid is met with
/// every face of the other).
fn reaches(
    tris0: &[([Point3; 3], BoundingBox<Point3>)],
    tris1: &[([Point3; 3], BoundingBox<Point3>)],
) -> bool {
    let whole = |tris: &[([Point3; 3], BoundingBox<Point3>)]| {
        tris.iter().fold(BoundingBox::new(), |mut b, (_, t)| {
            b += t;
            b
        })
    };
    let (box0, box1) = (whole(tris0), whole(tris1));
    let meets = |tris: &[([Point3; 3], BoundingBox<Point3>)], other: &BoundingBox<Point3>| {
        !other.is_empty() && tris.iter().any(|(_, t)| !disjoint_boxes(t, other))
    };
    meets(tris0, &box1) && meets(tris1, &box0)
}

/// Pairs of triangles (one from each mesh) whose bounding boxes overlap, in the order the
/// sweep along `unit` meets them.
///
/// The sweep keeps the triangles whose interval along `unit` is open. When a triangle opens,
/// it pairs with every open triangle of the other mesh whose box it overlaps. The pairs and
/// their order are the same as the earlier version, which cloned the open list for every
/// triangle and rejected disjoint boxes afterwards (rebuilding both boxes for each pair).
fn candidate_pairs(
    tris0: &[([Point3; 3], BoundingBox<Point3>)],
    tris1: &[([Point3; 3], BoundingBox<Point3>)],
    unit: Vector3,
) -> Vec<(usize, usize)> {
    let endpoints = sorted_endpoints(
        tris0.iter().map(|(t, _)| *t),
        tris1.iter().map(|(t, _)| *t),
        unit,
    );
    let boxes = [tris0, tris1];
    let mut open = [Vec::<usize>::new(), Vec::<usize>::new()];
    let mut pairs = Vec::new();
    for EndPoint {
        r#type,
        segnum,
        index,
        ..
    } in endpoints
    {
        match r#type {
            EndPointType::Front => {
                let mine = &boxes[segnum][index].1;
                for &other in &open[1 - segnum] {
                    if !disjoint_boxes(mine, &boxes[1 - segnum][other].1) {
                        pairs.push(if segnum == 0 {
                            (index, other)
                        } else {
                            (other, index)
                        });
                    }
                }
                open[segnum].push(index);
            }
            EndPointType::Back => {
                // Every interval opens before it closes unless a NaN broke the sort. Then the
                // interval stays open: more box tests, never a panic (the web build aborts on any).
                if let Some(i) = open[segnum].iter().position(|idx| *idx == index) {
                    open[segnum].swap_remove(i);
                }
            }
        }
    }
    pairs
}

fn collision(poly0: &PolygonMesh, poly1: &PolygonMesh) -> Vec<(Point3, Point3)> {
    let unit = match poly0.positions().is_empty() {
        true => return Vec::new(),
        false => hash::take_one_unit(poly0.positions()[0]),
    };
    let (tris0, tris1) = (boxed_triangles(poly0), boxed_triangles(poly1));
    // SolidCraft: no sweep for meshes that cannot meet.
    if !reaches(&tris0, &tris1) {
        return Vec::new();
    }
    let pairs = candidate_pairs(&tris0, &tris1, unit);
    let test = |&(i, j): &(usize, usize)| collide_triangles(tris0[i].0, tris1[j].0);
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        // Indexed parallel collect: results in the same order as the sequential loop.
        pairs.par_iter().filter_map(test).collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        pairs.iter().filter_map(test).collect()
    }
}

fn are_colliding(poly0: &PolygonMesh, poly1: &PolygonMesh) -> Option<(Point3, Point3)> {
    let unit = match poly0.positions().is_empty() {
        true => return None,
        false => hash::take_one_unit(poly0.positions()[0]),
    };
    let (tris0, tris1) = (boxed_triangles(poly0), boxed_triangles(poly1));
    let pairs = candidate_pairs(&tris0, &tris1, unit);
    let test = |&(i, j): &(usize, usize)| collide_triangles(tris0[i].0, tris1[j].0);
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        // The first collision in sweep order, as the sequential search returns.
        pairs.par_iter().find_map_first(test)
    }
    #[cfg(target_arch = "wasm32")]
    {
        pairs.iter().find_map(test)
    }
}

#[test]
fn collide_triangles_test() {
    let tri0 = [
        Point3::origin(),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ];
    let tri1 = [
        Point3::new(0.0, 0.0, -1.0),
        Point3::new(-1.0, -1.0, 1.0),
        Point3::new(1.0, 1.0, 1.0),
    ];
    assert!(collide_triangles(tri0, tri1).is_some());
    let tri0 = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ];
    let tri1 = [
        Point3::new(0.0, 0.0, 0.5),
        Point3::new(1.0, 0.0, 1.0),
        Point3::new(1.0, 1.0, 1.0),
    ];
    assert!(collide_triangles(tri0, tri1).is_none());
}

/// The candidate search before boxes were computed once and the open list stopped being
/// cloned: kept to check the new search finds the same collisions in the same order.
///
/// Not part of the API. Built only for tests, or with the `test-hooks` feature, so the
/// SolidCraft kernel tests can run these checks in CI (this crate's own tests do not run there).
#[cfg(any(test, feature = "test-hooks"))]
#[doc(hidden)]
pub mod test_hooks {
    use super::*;

    fn old_pairs(sort_endpoints: Vec<EndPoint>) -> Vec<(usize, usize)> {
        let mut current = [Vec::<usize>::new(), Vec::<usize>::new()];
        let mut out = Vec::new();
        for EndPoint { r#type, segnum, index, .. } in sort_endpoints {
            match r#type {
                EndPointType::Front => {
                    current[segnum].push(index);
                    for i in current[1 - segnum].clone() {
                        out.push(if segnum == 0 { (index, i) } else { (i, index) });
                    }
                }
                EndPointType::Back => {
                    if let Some(i) = current[segnum].iter().position(|idx| *idx == index) {
                        current[segnum].swap_remove(i);
                    }
                }
            }
        }
        out
    }

    fn old_disjoint(tri0: [Point3; 3], tri1: [Point3; 3]) -> bool {
        let b0: BoundingBox<Point3> = tri0.iter().collect();
        let b1: BoundingBox<Point3> = tri1.iter().collect();
        disjoint_boxes(&b0, &b1)
    }

    /// Small seeded generator (no extra dependency).
    struct Lcg(u64);
    impl Lcg {
        fn unit(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
        /// On a coarse grid half the time, so shared vertices, touching boxes and
        /// degenerate triangles come up.
        fn coord(&mut self) -> f64 {
            if self.unit() < 0.5 { (self.unit() * 8.0).floor() * 0.5 } else { self.unit() * 4.0 }
        }
        fn tri(&mut self) -> [Point3; 3] {
            array![_ => Point3::new(self.coord(), self.coord(), self.coord()); 3]
        }
    }

    /// A NaN coordinate breaks the endpoint sort: the search must go on without a panic.
    pub fn nan_triangles_do_not_panic() -> Result<(), String> {
        let good = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)];
        let nan = [Point3::new(f64::NAN, 0.0, 0.0), Point3::new(1.0, f64::NAN, 0.0), Point3::new(0.0, 1.0, 0.5)];
        let crossing = [Point3::new(0.2, 0.2, -1.0), Point3::new(0.2, 0.2, 1.0), Point3::new(0.6, 0.2, 0.0)];
        for (a, b) in [(vec![good, nan], vec![crossing, nan]), (vec![nan, nan], vec![nan]), (vec![nan], vec![good])] {
            let ba: Vec<_> = a.iter().map(|t| (*t, t.iter().collect())).collect();
            let bb: Vec<_> = b.iter().map(|t| (*t, t.iter().collect())).collect();
            let r = std::panic::catch_unwind(|| candidate_pairs(&ba, &bb, Vector3::new(1.0, 0.0, 0.0)));
            if r.is_err() {
                return Err(format!("panicked on {a:?} / {b:?}"));
            }
        }
        Ok(())
    }

    /// The new candidate search finds the same collisions, in the same order, as the old one
    /// on random triangle sets. Returns how many collisions were compared.
    pub fn same_collisions_in_the_same_order(seeds: std::ops::Range<u64>) -> Result<usize, String> {
        let mut compared = 0;
        for seed in seeds {
            let mut r = Lcg(seed * 7919 + 1);
            let n0 = 1 + (r.unit() * 60.0) as usize;
            let n1 = 1 + (r.unit() * 60.0) as usize;
            let t0: Vec<[Point3; 3]> = (0..n0).map(|_| r.tri()).collect();
            let t1: Vec<[Point3; 3]> = (0..n1).map(|_| r.tri()).collect();
            let unit = hash::take_one_unit(t0[0][0]);

            let old: Vec<(Point3, Point3)> = old_pairs(sorted_endpoints(t0.iter().copied(), t1.iter().copied(), unit))
                .into_iter()
                .filter(|&(i, j)| !old_disjoint(t0[i], t1[j]))
                .filter_map(|(i, j)| collide_triangles(t0[i], t1[j]))
                .collect();

            let b0: Vec<_> = t0.iter().map(|t| (*t, t.iter().collect())).collect();
            let b1: Vec<_> = t1.iter().map(|t| (*t, t.iter().collect())).collect();
            let new: Vec<(Point3, Point3)> = candidate_pairs(&b0, &b1, unit)
                .into_iter()
                .filter_map(|(i, j)| collide_triangles(t0[i], t1[j]))
                .collect();

            if old.len() != new.len() {
                return Err(format!("seed {seed}: {} collisions before, {} now", old.len(), new.len()));
            }
            for (k, (a, b)) in old.iter().zip(&new).enumerate() {
                if !(a.0 == b.0 && a.1 == b.1) {
                    return Err(format!("seed {seed}: collision {k} differs: {a:?} vs {b:?}"));
                }
            }
            compared += old.len();
        }
        Ok(compared)
    }

    /// Meshes the early exit of `collision` (`reaches`) calls apart have no candidate pairs, on
    /// random triangle sets moved apart by up to twice their size. Returns how many were apart.
    pub fn apart_meshes_have_no_pairs(seeds: std::ops::Range<u64>) -> Result<usize, String> {
        let mut apart = 0;
        for seed in seeds {
            let mut r = Lcg(seed * 7919 + 3);
            let n0 = 1 + (r.unit() * 30.0) as usize;
            let n1 = 1 + (r.unit() * 30.0) as usize;
            let shift = Vector3::new(r.unit() * 8.0, r.unit() * 8.0, r.unit() * 8.0);
            let t0: Vec<[Point3; 3]> = (0..n0).map(|_| r.tri()).collect();
            let t1: Vec<[Point3; 3]> = (0..n1).map(|_| r.tri().map(|p| p + shift)).collect();
            let b0: Vec<_> = t0.iter().map(|t| (*t, t.iter().collect())).collect();
            let b1: Vec<_> = t1.iter().map(|t| (*t, t.iter().collect())).collect();
            if reaches(&b0, &b1) {
                continue;
            }
            apart += 1;
            let pairs = candidate_pairs(&b0, &b1, hash::take_one_unit(t0[0][0]));
            if !pairs.is_empty() {
                return Err(format!("seed {seed}: called apart, but {} pairs overlap", pairs.len()));
            }
        }
        Ok(apart)
    }

    #[cfg(test)]
    #[test]
    fn hooks_pass() {
        assert_eq!(nan_triangles_do_not_panic(), Ok(()));
        assert!(matches!(apart_meshes_have_no_pairs(0..400), Ok(n) if n > 0));
        let n = same_collisions_in_the_same_order(0..200);
        assert!(matches!(n, Ok(n) if n > 0), "{n:?}");
    }
}
