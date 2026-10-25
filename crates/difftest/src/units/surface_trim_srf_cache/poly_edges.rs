//! Polygon-edge implantation followed by edge-use-driven subdivision.
//! Both kernels receive the original face surface separately from the cache surface.

use super::{
    bss_of, build_tree, cache_of, class_code, cpp_input, curve_of, dom_of, domain_of_srf, ffi,
    flag, gen_cache_dom, gen_srf, harness, link, push_crv, push_dom, push_srf, push_tree, status,
    to_output, BssIn, CallIn, Case, CaseContext, Crv2In, Deserialize, Extent2d, HarnessError,
    InlineSrf, Inputs, Kernel, NodeData, NodeId, NodeIn, Op, OpIn, Output, Rng, Run, Serialize,
    SolidError, SplitDir, SrfIn, Surface, Value, DEFAULT_OPTS, NOW,
};
use crate::case::CaseSource;
use solidcore::geom::vector3d::Point3;
use solidcore::surface::trim_srf_cache::PolyEdgeInput;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Edge {
    start: [f64; 3],
    end: [f64; 3],
    tolerance: f64,
    node: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Input {
    srf: BssIn,
    original_srf: BssIn,
    dom: [f64; 4],
    factor: f64,
    tree: Vec<NodeIn>,
    edges: Vec<Edge>,
    calls: Vec<CallIn>,
    #[serde(default)]
    remap: bool,
}

fn encoded(i: &Input) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    push_srf(&mut o, &i.srf)?;
    o.extend(i.dom);
    o.push(i.factor);
    push_tree(&mut o, &i.tree);
    push_srf(&mut o, &i.original_srf)?;
    o.push(i.edges.len() as f64);
    for edge in &i.edges {
        o.extend(edge.start);
        o.extend(edge.end);
        o.extend([edge.tolerance, edge.node as f64]);
    }
    o.push(flag(i.remap));
    o.push(i.calls.len() as f64);
    for call in &i.calls {
        push_crv(&mut o, &call.crv);
        o.extend([flag(call.subdiv), call.node as f64]);
    }
    Ok(o)
}

pub(super) fn run_cpp(value: &Value) -> Result<Output, HarnessError> {
    let i: Input = serde_json::from_value(value.clone()).map_err(|e| harness(e.to_string()))?;
    ffi::eval(Op::ImplantPoly, &encoded(&i)?)
        .map(|o| to_output(&o))
        .map_err(|e| harness(e.to_string()))
}

pub(super) fn run_rust(value: &Value) -> Result<Output, HarnessError> {
    let i: Input = serde_json::from_value(value.clone()).map_err(|e| harness(e.to_string()))?;
    let k = Kernel::default();
    let mut c = cache_of(&bss_of(&i.srf)?, DEFAULT_OPTS);
    c.base_mut().set_uv_domain(dom_of(i.dom));
    c.base_mut().trim_subdivision_factor = i.factor;
    c.base_mut().point_test_enabled = false;
    build_tree(&mut c, &i.tree)?;
    let original = Surface::from(bss_of(&i.original_srf)?);
    let curves = i
        .calls
        .iter()
        .map(|call| curve_of(&call.crv))
        .collect::<Result<Vec<_>, _>>()?;
    let result = (|| -> Run {
        let mut o = vec![0.0];
        for (id, edge) in i.edges.iter().enumerate() {
            c.implant_poly_edge(
                &k,
                PolyEdgeInput {
                    poly_edge: id as i64,
                    start: Point3::new(edge.start[0], edge.start[1], edge.start[2]),
                    end: Point3::new(edge.end[0], edge.end[1], edge.end[2]),
                    // The native fixture constructs SmZoneTol3d before GetTolerance.
                    tolerance: edge.tolerance.abs(),
                    surface: &original,
                },
                NodeId(edge.node),
            )?;
            o.push(0.0);
        }
        if i.remap {
            c = c
                .map_handles(|h| Ok(h + 100))?
                .map_handles(|h| Ok(h - 100))?;
            // Base mapping clears the fixture's callback, as documented by map_handles.
            c.base_mut()
                .set_subdivision_handles(Some(|n| 1000 + n as i64));
        }
        let mut extent = Extent2d::new();
        for (id, (call, curve)) in i.calls.iter().zip(&curves).enumerate() {
            match c.implant_edgeuse(
                &k,
                id as i64,
                Some(curve),
                call.subdiv,
                &mut extent,
                link(call.node),
            ) {
                Ok(()) => o.push(0.0),
                Err(e) if e.is_unported() => return Err(e),
                Err(e) => {
                    o.push(status(&e));
                    break;
                }
            }
        }
        push_dom(&mut o, &extent);
        let tree = c.base().tree().ok_or(SolidError::InvalidInput {
            operation: "difftest::trim_poly",
            reason: "lost tree",
        })?;
        o.push(tree.nodes().len() as f64);
        for (id, node) in tree.nodes().iter().enumerate() {
            o.extend(
                [node.parent, node.child1, node.child2].map(|id| id.map_or(-1.0, |id| id.0 as f64)),
            );
            let aux = c.base().aux(NodeId(id))?;
            push_dom(&mut o, &aux.uv_domain);
            let NodeData::Surface(data) = node.data else {
                unreachable!("fixture surface tree")
            };
            o.push(match data.split_dir {
                SplitDir::U => 0.0,
                SplitDir::V => 1.0,
                SplitDir::Unknown => 2.0,
            });
            o.extend([flag(data.is_marked), class_code(data.node_class)]);
            o.push(aux.edgeuse_list.len() as f64);
            o.extend(aux.edgeuse_list.iter().map(|&id| id as f64));
            o.push(aux.poly_edge_list.len() as f64);
            o.extend(aux.poly_edge_list.iter().map(|&id| id as f64));
        }
        Ok(o)
    })();
    Ok(match result {
        Ok(o) => to_output(&o),
        Err(e) => Output::unported(&e).unwrap_or_else(|| to_output(&[status(&e)])),
    })
}

/// Append a separate fixed-seed stream without changing any existing generated input.
pub(super) fn cases() -> Result<Vec<Case>, HarnessError> {
    let mut cases = Vec::new();
    for index in 0..128 {
        let seed = CaseContext::seed_for("surface.trim_srf_cache.poly_edges", index);
        let mut rng = Rng::new(seed);
        let cache_srf = valid_surface(&mut rng)?;
        let natural = domain_of_srf(&cache_srf);
        let dom = gen_cache_dom(&mut rng, natural, false);
        let mut original_srf = cache_srf.clone();
        let scale = match index % 4 {
            0 => 1e-6,
            1 => 1e-3,
            2 => 1.0,
            _ => 1e3,
        };
        for row in &mut original_srf.net {
            for p in row {
                for c in &mut p[..3] {
                    *c *= scale;
                }
            }
        }
        let point = |u: f64, v: f64| {
            [
                dom[0] + u * (dom[2] - dom[0]),
                dom[1] + v * (dom[3] - dom[1]),
                0.0,
            ]
        };
        let tolerance = scale * 1e-4;
        // A miss of the cache domain that still evaluates on the original surface.
        let outside = [
            (natural[0] + dom[0]) / 2.0,
            (natural[1] + dom[1]) / 2.0,
            0.0,
        ];
        let segments = [
            (point(0.1, 0.1), point(0.4, 0.3)),
            (point(0.6, 0.7), point(0.9, 0.9)),
            (point(-0.1, 0.4), point(1.1, 0.6)),
            (point(0.5, 0.1), point(0.5, 0.9)),
            (point(0.25, 0.25), point(0.25, 0.25)),
            (outside, outside),
        ];
        let edges = segments
            .into_iter()
            .map(|(start, end)| Edge {
                start,
                end,
                tolerance,
                node: 0,
            })
            .collect();
        let curve = |a: [f64; 3], b: [f64; 3]| Crv2In {
            p: 1,
            u: vec![0.0, 0.0, 1.0, 1.0],
            cpts: vec![[a[0], a[1], NOW], [b[0], b[1], NOW]],
        };
        let calls = vec![
            CallIn {
                crv: curve(point(0.1, 0.1), point(0.9, 0.9)),
                subdiv: false,
                node: 0,
            },
            CallIn {
                crv: curve(point(0.1, 0.9), point(0.9, 0.1)),
                subdiv: true,
                node: 0,
            },
            CallIn {
                crv: curve(point(0.1, 0.5), point(0.9, 0.5)),
                subdiv: true,
                node: 0,
            },
        ];
        let input = Input {
            srf: BssIn {
                oob: false,
                form: 11,
                srf: SrfIn::Inline(cache_srf),
            },
            original_srf: BssIn {
                oob: false,
                form: 11,
                srf: SrfIn::Inline(original_srf),
            },
            dom,
            factor: if index % 2 == 0 { 4.0 } else { 8.0 },
            tree: vec![NodeIn {
                parent: -1,
                c1: -1,
                c2: -1,
                aux: 3,
                split: 2,
                marked: false,
                class: 0,
                dom,
            }],
            edges,
            calls,
            remap: index % 3 == 0,
        };
        // Invalid generated data is a generator bug, never silently dropped.
        ffi::validate(Op::ImplantPoly, &encoded(&input)?).map_err(|e| harness(e.to_string()))?;
        cases.push(Case {
            unit: "surface.trim_srf_cache".into(),
            name: format!("poly_edges_gen{index:04}"),
            source: CaseSource::Generated { seed },
            inputs: serde_json::json!({"poly_edges": input}),
        });
    }
    Ok(cases)
}

/// The shared surface generator can repeat an interior knot even for degree one;
/// redraw those invalid NURBs just as the existing runner's generator does.
fn valid_surface(rng: &mut Rng) -> Result<InlineSrf, HarnessError> {
    for _ in 0..64 {
        let s = gen_srf(rng, 2);
        let i = Inputs {
            srf: BssIn {
                oob: false,
                form: 11,
                srf: SrfIn::Inline(s.clone()),
            },
            op: OpIn::New {
                opts: DEFAULT_OPTS,
                kinds: Vec::new(),
            },
        };
        let (_, encoded) = cpp_input(&i)?;
        if ffi::validate(Op::New, &encoded).is_ok() {
            return Ok(s);
        }
    }
    Err(harness("could not generate a valid polygon-cache surface"))
}
