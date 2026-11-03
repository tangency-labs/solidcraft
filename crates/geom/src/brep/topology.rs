//! The structural rules of the topology records: references, loops, regions and shells, each
//! edge's list of uses and the canonical order (docs/native-brep.md). Indices are hostile:
//! every lookup is checked, and work is linear in the records.
use std::collections::HashSet;

use super::{Brep, EdgeUseRef, Invalid, Loop, Result, Shell, require};

/// How far a pcurve's interval may differ from its edge's: this fraction of the interval's
/// magnitude (at least 1), room for a linear remap's rounding and nothing more.
pub(super) const PCURVE_INTERVAL: f64 = 1e-9;

fn get<'a, T>(items: &'a [T], i: u32, what: &'static str) -> Result<&'a T> {
    items.get(i as usize).ok_or(Invalid(what))
}

/// The next index in a first-appearance order: `seen[i]` is set the first time `i` turns up,
/// and that must be in index order (the first new one is `next`).
struct Order {
    seen: Vec<bool>,
    next: usize,
    what: &'static str,
}
impl Order {
    fn new(n: usize, what: &'static str) -> Order {
        Order { seen: vec![false; n], next: 0, what }
    }
    fn visit(&mut self, i: u32) -> Result<bool> {
        let slot = self.seen.get_mut(i as usize).ok_or(Invalid(self.what))?;
        if *slot {
            return Ok(false);
        }
        require(i as usize == self.next, self.what)?;
        *slot = true;
        self.next += 1;
        Ok(true)
    }
    fn complete(&self) -> Result<()> {
        require(self.next == self.seen.len(), self.what)
    }
}

pub(super) fn check(b: &Brep) -> Result<()> {
    // Regions and shells: which regions list each shell, and as whose outer shell.
    let mut listed = vec![0u32; b.shells.len()];
    let mut outer_of = vec![0u32; b.shells.len()];
    let infinite = b.regions.first().ok_or(Invalid("region count"))?;
    require(!infinite.void, "the infinite region is a void")?;
    // The last region (numbered from 1) that listed each shell: a repeat within one region shows
    // in one lookup, so the check stays linear however many shells and regions there are.
    let mut last_region = vec![0usize; b.shells.len()];
    for (r, region) in b.regions.iter().enumerate() {
        require(region.shells.len() <= b.shells.len(), "region shells")?;
        for (k, &s) in region.shells.iter().enumerate() {
            let last = last_region.get_mut(s as usize).ok_or(Invalid("region shell reference"))?;
            require(*last != r + 1, "shell listed twice in a region")?;
            *last = r + 1;
            let n = listed.get_mut(s as usize).ok_or(Invalid("region shell reference"))?;
            *n += 1;
            if r > 0 && k == 0 {
                require(matches!(get(&b.shells, s, "region shell reference")?, Shell::Faces { .. }), "outer shell is not a face shell")?;
                if let Some(n) = outer_of.get_mut(s as usize) {
                    *n += 1;
                }
            }
        }
        require(r == 0 || !region.shells.is_empty(), "bounded region without an outer shell")?;
    }

    // Faces: who uses each one. Loops: references, shape and connectivity.
    let mut face_uses: Vec<Vec<(usize, bool)>> = vec![Vec::new(); b.faces.len()];
    for (s, shell) in b.shells.iter().enumerate() {
        match shell {
            Shell::Faces { faces, wires } => {
                require(!faces.is_empty(), "empty shell")?;
                for f in faces {
                    face_uses.get_mut(f.face as usize).ok_or(Invalid("oriented face reference"))?.push((s, f.forward));
                }
                for &e in wires {
                    require(get(&b.edges, e, "wire edge reference")?.uses.is_empty(), "wire edge with face uses")?;
                }
            }
            Shell::Wire { edges } => {
                require(!edges.is_empty(), "empty wire shell")?;
                for &e in edges {
                    require(get(&b.edges, e, "wire edge reference")?.uses.is_empty(), "wire edge with face uses")?;
                }
            }
            Shell::Vertex { vertex } => {
                get(&b.vertices, *vertex, "vertex shell reference")?;
            }
        }
    }
    for uses in &face_uses {
        match uses.as_slice() {
            [_] => {}
            [(a, fa), (b, fb)] => require(a != b && fa != fb, "a face's two sides")?,
            _ => return Err(Invalid("face used by neither one nor two oriented faces")),
        }
    }
    for f in &b.faces {
        require(matches!(f.loops.first(), Some(Loop::Edges { .. })), "outer loop is not an edge loop")?;
        for l in &f.loops {
            match l {
                Loop::Vertex { vertex } => {
                    get(&b.vertices, *vertex, "vertex loop reference")?;
                }
                Loop::Edges { edges } => {
                    require(!edges.is_empty(), "empty loop")?;
                    let (mut first, mut last) = (None, None);
                    for u in edges {
                        let edge = get(&b.edges, u.edge, "edge reference")?;
                        let [a, z] = edge.vertices;
                        let (start, end) = if u.forward { (a, z) } else { (z, a) };
                        require(last.is_none_or(|v| v == start), "disconnected loop")?;
                        first.get_or_insert(start);
                        last = Some(end);
                        let p = u.pcurve.interval().ok_or(Invalid("pcurve interval"))?;
                        let scale = PCURVE_INTERVAL * (1f64).max(edge.interval[0].abs()).max(edge.interval[1].abs());
                        require(
                            (p[0] - edge.interval[0]).abs() <= scale && (p[1] - edge.interval[1]).abs() <= scale,
                            "pcurve interval differs from its edge's",
                        )?;
                    }
                    require(first == last, "open loop")?;
                }
            }
        }
    }

    // Shells: a closed face shell (every edge on its faces used at least twice among them)
    // separates two regions and encloses one; any other shell lies in one region.
    let mut count = vec![0u32; b.edges.len()];
    for (s, shell) in b.shells.iter().enumerate() {
        let (listed, outer) = (listed.get(s).copied().unwrap_or(0), outer_of.get(s).copied().unwrap_or(0));
        let closed = match shell {
            Shell::Faces { faces, .. } => {
                let edges = || {
                    faces.iter().filter_map(|f| b.faces.get(f.face as usize)).flat_map(|f| &f.loops).flat_map(|l| match l {
                        Loop::Edges { edges } => edges.as_slice(),
                        Loop::Vertex { .. } => &[],
                    })
                };
                for u in edges() {
                    if let Some(n) = count.get_mut(u.edge as usize) {
                        *n = n.saturating_add(1);
                    }
                }
                let closed = edges().all(|u| count.get(u.edge as usize).is_some_and(|n| *n >= 2));
                for u in edges() {
                    if let Some(n) = count.get_mut(u.edge as usize) {
                        *n = 0;
                    }
                }
                closed
            }
            Shell::Wire { .. } | Shell::Vertex { .. } => false,
        };
        if closed {
            require(listed == 2 && outer == 1, "a closed shell bounds two regions and encloses one")?;
        } else {
            require(listed == 1 && outer == 0, "an open, wire or vertex shell lies in one region")?;
        }
    }

    // Canonical order. Shells and faces in first appearance walking regions → shells →
    // oriented faces; each face is walked in the direction its first oriented face gives.
    let mut shells = Order::new(b.shells.len(), "shells out of canonical order");
    let mut faces = Order::new(b.faces.len(), "faces out of canonical order");
    let mut direction = vec![true; b.faces.len()];
    for region in &b.regions {
        for &s in &region.shells {
            if !shells.visit(s)? {
                continue;
            }
            if let Some(Shell::Faces { faces: uses, .. }) = b.shells.get(s as usize) {
                for f in uses {
                    if faces.visit(f.face)?
                        && let Some(d) = direction.get_mut(f.face as usize)
                    {
                        *d = f.forward;
                    }
                }
            }
        }
    }
    shells.complete()?;
    faces.complete()?;

    // Edges in first appearance walking faces → loops → oriented edges (backwards in a face
    // whose first side is reversed), then wire edges walking shells; each edge's uses in that
    // walk's order, the first one primary.
    let mut edges = Order::new(b.edges.len(), "edges out of canonical order");
    let mut walked: Vec<Vec<EdgeUseRef>> = vec![Vec::new(); b.edges.len()];
    for (fi, f) in b.faces.iter().enumerate() {
        let forward = direction.get(fi).copied().unwrap_or(true);
        for (li, l) in f.loops.iter().enumerate() {
            let Loop::Edges { edges: uses } = l else { continue };
            let n = uses.len();
            for k in 0..n {
                let p = if forward { k } else { n - 1 - k };
                let u = uses.get(p).ok_or(Invalid("edge reference"))?;
                edges.visit(u.edge)?;
                let at = EdgeUseRef { face: index(fi)?, r#loop: index(li)?, position: index(p)?, upper: !u.forward };
                walked.get_mut(u.edge as usize).ok_or(Invalid("edge reference"))?.push(at);
            }
        }
    }
    for shell in &b.shells {
        let wires = match shell {
            Shell::Faces { wires, .. } => wires.as_slice(),
            Shell::Wire { edges } => edges.as_slice(),
            Shell::Vertex { .. } => &[],
        };
        for &e in wires {
            edges.visit(e)?;
        }
    }
    edges.complete()?;
    for (edge, walked) in b.edges.iter().zip(&walked) {
        require(edge.uses.len() == walked.len(), "edge uses do not list the edge's uses")?;
        require(edge.uses.first() == walked.first(), "edge's primary use is not its first")?;
        // As many entries as uses, and each one a use not listed before: each use once.
        let mut left: HashSet<EdgeUseRef> = walked.iter().copied().collect();
        for u in &edge.uses {
            require(left.remove(u), "edge use listed twice or not a use of the edge")?;
        }
    }

    // Vertices in first appearance walking edges (start, end), then vertex loops walking
    // faces → loops, then vertex shells walking shells.
    let mut vertices = Order::new(b.vertices.len(), "vertices out of canonical order");
    for e in &b.edges {
        for &v in &e.vertices {
            vertices.visit(v)?;
        }
    }
    for f in &b.faces {
        for l in &f.loops {
            if let Loop::Vertex { vertex } = l {
                vertices.visit(*vertex)?;
            }
        }
    }
    for shell in &b.shells {
        if let Shell::Vertex { vertex } = shell {
            vertices.visit(*vertex)?;
        }
    }
    vertices.complete()
}

fn index(i: usize) -> Result<u32> {
    u32::try_from(i).map_err(|_| Invalid("index"))
}
