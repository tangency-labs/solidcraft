//! The BSP of the faceted boolean on trees deeper than a small stack holds (#2470): a convex
//! faceted face makes a chain one node per facet, whatever order its facets come in.

use super::*;

/// The side facets of a convex `n`-sided prism about Z, as polygons of operand `operand`:
/// every facet has all the others behind it, so the tree is a chain `n` deep.
fn prism(n: usize, r: f64, h: f64, operand: usize) -> Vec<Poly> {
    (0..n)
        .map(|i| {
            let (a0, a1) = (i as f64 * std::f64::consts::TAU / n as f64, (i + 1) as f64 * std::f64::consts::TAU / n as f64);
            let p = |a: f64, z: f64| Vec3::new(r * a.cos(), r * a.sin(), z);
            let v = vec![p(a0, 0.0), p(a1, 0.0), p(a1, h), p(a0, h)];
            let n = (v[1] - v[0]).cross(v[3] - v[0]).normalized().unwrap();
            Poly { v, plane: PlaneEq { n, w: n.dot(p(a0, 0.0)) }, src: FaceOrigin::Face { operand, face: i } }
        })
        .collect()
}

/// `f` on a thread with a 512 KiB stack, half the main thread's on Windows. A stack overflow
/// aborts the whole test run rather than failing this test.
fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new().stack_size(512 << 10).spawn(f).unwrap().join().unwrap()
}

fn depth(tree: &Tree) -> usize {
    let mut deepest = 0;
    let mut work = vec![(0usize, 1usize)];
    while let Some((at, d)) = work.pop() {
        deepest = deepest.max(d);
        let node = &tree.nodes[at];
        work.extend(node.front.into_iter().chain(node.back).map(|c| (c, d + 1)));
    }
    deepest
}

/// A 3000-facet prism cut by a small one beside it: a tree 3000 deep, built, clipped, inverted,
/// collected and dropped on a small stack. Before #2470 2000 levels overflowed 1 MiB.
#[test]
fn a_chain_deeper_than_the_stack_holds_is_cut() {
    let pa = shuffled(prism(3000, 5.0, 10.0, 0));
    // Clear of the big prism and of every one of its facets' planes' far sides: nothing splits.
    let pb: Vec<Poly> = prism(4, 1.0, 10.0, 1)
        .into_iter()
        .map(|mut p| {
            p.v = p.v.into_iter().map(|v| v + Vec3::new(100.0, 0.0, 0.0)).collect();
            p.plane.w = p.plane.n.dot(p.v[0]);
            p
        })
        .collect();
    let (deep, out) = on_small_stack(move || {
        let mut tree = Tree::default();
        tree.build(pa.clone(), 1e-9, &mut Pieces(MAX_PIECES)).unwrap();
        (depth(&tree), csg_within(pa, pb, BoolOp::Cut, 1e-9, 0))
    });
    assert_eq!(deep, 3000, "a convex face is a chain one node per facet");
    // With no splits (a budget of none is enough), the big prism comes through whole.
    let out = out.unwrap();
    let kept = out.iter().filter(|p| matches!(p.src, FaceOrigin::Face { operand: 0, .. })).count();
    assert_eq!(kept, 3000);
}

/// Past the depth cap the tree is refused with an error, on any stack.
#[test]
fn a_chain_past_the_depth_cap_is_refused() {
    let pa = prism(MAX_DEPTH + 100, 5.0, 10.0, 0);
    let r = on_small_stack(move || Tree::default().build(pa, 1e-9, &mut Pieces(MAX_PIECES)));
    assert_eq!(r, Err(KernelError::Failed("planar boolean: tree too deep".into())));
}

/// Splits past the budget stop the boolean with an error instead of growing without end.
#[test]
fn splits_past_the_budget_are_refused() {
    // Two boxes' sides crossing each other: some facets of each span the other's planes.
    let (pa, pb) = (prism(4, 5.0, 10.0, 0), prism(4, 5.0, 10.0, 1));
    let turned: Vec<Poly> = pb
        .into_iter()
        .map(|mut p| {
            let turn = |v: Vec3| Vec3::new(v.x * 0.6 - v.y * 0.8, v.x * 0.8 + v.y * 0.6, v.z + 3.0);
            p.v = p.v.into_iter().map(turn).collect();
            let n = (p.v[1] - p.v[0]).cross(p.v[3] - p.v[0]).normalized().unwrap();
            p.plane = PlaneEq { n, w: n.dot(p.v[0]) };
            p
        })
        .collect();
    assert!(csg_within(pa.clone(), turned.clone(), BoolOp::Cut, 1e-9, MAX_PIECES).is_ok());
    assert_eq!(csg_within(pa, turned, BoolOp::Cut, 1e-9, 0).err(), Some(KernelError::Failed("the bodies are too detailed to facet".into())));
}
