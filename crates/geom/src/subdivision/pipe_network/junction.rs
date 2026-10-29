//! A perforated spherical quad grid whose disjoint collars share branch rims.
use super::*;

pub(super) struct Port {
    pub direction: Vec3,
    pub ring: Vec<usize>,
    pub radius: f64,
    pub reverse: bool,
}
const AXES: [Vec3; 6] = [Vec3::X, Vec3::new(-1.0, 0.0, 0.0), Vec3::Y, Vec3::new(0.0, -1.0, 0.0), Vec3::Z, Vec3::new(0.0, 0.0, -1.0)];
fn assignments(ports: &[Port]) -> Result<Vec<usize>> {
    fn search(ports: &[Port], used: u8, chosen: &mut Vec<usize>, score: f64, best: &mut (f64, Vec<usize>)) {
        if chosen.len() == ports.len() {
            if score > best.0 {
                *best = (score, chosen.clone());
            }
            return;
        }
        for (axis, direction) in AXES.iter().enumerate() {
            if used & (1 << axis) == 0 {
                let next = score + ports[chosen.len()].direction.dot(*direction);
                chosen.push(axis);
                search(ports, used | (1 << axis), chosen, next, best);
                chosen.pop();
            }
        }
    }
    let mut best = (f64::NEG_INFINITY, Vec::new());
    search(ports, 0, &mut Vec::new(), 0.0, &mut best);
    if best.1.len() != ports.len() {
        return Err(bad("pipe junction has no distinct port assignment"));
    }
    Ok(best.1)
}
pub(super) fn attach(cage: &mut Cage, center: Vec3, ports: &[Port], density: usize) -> Result<()> {
    if !(3..=6).contains(&ports.len()) || !(1..=12).contains(&density) || ports.iter().any(|p| p.ring.len() != 8 * density) {
        return Err(bad("a pipe junction needs 3..6 compatible branch rings"));
    }
    let assigned = assignments(ports)?;
    let n = (4 * density) as i32;
    let frames = [
        ([n, 0, 0], [0, 1, 0], [0, 0, 1]),
        ([0, 0, 0], [0, 0, 1], [0, 1, 0]),
        ([0, n, n], [1, 0, 0], [0, 0, -1]),
        ([0, 0, 0], [1, 0, 0], [0, 0, 1]),
        ([0, 0, n], [1, 0, 0], [0, 1, 0]),
        ([0, 0, 0], [0, 1, 0], [1, 0, 0]),
    ];
    let key = |axis: usize, i: i32, j: i32| {
        let (o, u, v) = frames[axis];
        [0, 1, 2].map(|d| o[d] + u[d] * i + v[d] * j)
    };
    let radius = ports.iter().map(|p| p.radius).fold(0.0f64, f64::max) * 1.35;
    let mut vertices = BTreeMap::<[i32; 3], usize>::new();
    let mut points = Vec::new();
    let mut faces = Vec::new();
    for axis in 0..6 {
        for j in 0..n {
            for i in 0..n {
                if assigned.contains(&axis) && (density as i32..3 * density as i32).contains(&i) && (density as i32..3 * density as i32).contains(&j)
                {
                    continue;
                }
                let mut face = Vec::with_capacity(4);
                for (s, t) in [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)] {
                    let k = key(axis, s, t);
                    let id = if let Some(&id) = vertices.get(&k) {
                        id
                    } else {
                        let direction = Vec3::new(k[0] as f64, k[1] as f64, k[2] as f64) * (2.0 / n as f64) - Vec3::new(1.0, 1.0, 1.0);
                        let p = center + direction.normalized().ok_or_else(|| bad("pipe junction has no radial direction"))? * radius;
                        let id = points.len();
                        points.push(p);
                        vertices.insert(k, id);
                        id
                    };
                    face.push(id);
                }
                faces.push(face);
            }
        }
    }
    let mut mapped = BTreeMap::new();
    let low = density as i32;
    let high = 3 * low;
    for (port, &axis) in ports.iter().zip(&assigned) {
        let wire: Vec<_> = (low..high)
            .map(|i| (i, low))
            .chain((low..high).map(|j| (high, j)))
            .chain((low + 1..=high).rev().map(|i| (i, high)))
            .chain((low + 1..=high).rev().map(|j| (low, j)))
            .map(|(i, j)| vertices.get(&key(axis, i, j)).copied().ok_or_else(|| bad("pipe junction lost a collar vertex")))
            .collect::<Result<_>>()?;
        let at = |offset: usize, i: usize| if port.reverse { (offset + wire.len() - i) % wire.len() } else { (offset + i) % wire.len() };
        let offset = (0..wire.len())
            .map(|offset| {
                let distance = wire
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| cage.points.get(port.ring[at(offset, i)]).map_or(f64::INFINITY, |p| (points[v] - *p).len2()))
                    .sum::<f64>();
                (distance, offset)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .filter(|v| v.0.is_finite())
            .map(|v| v.1)
            .ok_or_else(|| bad("pipe junction cannot align its branch rim"))?;
        for (i, &v) in wire.iter().enumerate() {
            if mapped.insert(v, port.ring[at(offset, i)]).is_some() {
                return Err(bad("pipe junction collars overlap"));
            }
        }
    }
    let ids: Vec<_> = points
        .into_iter()
        .enumerate()
        .map(|(id, p)| {
            mapped.get(&id).copied().unwrap_or_else(|| {
                let index = cage.points.len();
                cage.points.push(p);
                index
            })
        })
        .collect();
    cage.faces.extend(faces.into_iter().map(|face| face.into_iter().map(|v| ids[v]).collect()));
    budget(cage)?;
    Ok(())
}
