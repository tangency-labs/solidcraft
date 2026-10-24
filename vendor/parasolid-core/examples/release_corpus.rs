//! Catalog-free release evidence; fixtures are supplied separately from the crate.

#[path = "../tests/support/sch30000.rs"]
mod support;

use std::{env, fs, process};

use parasolid_core::brep::{BrepModel, map_xb_brep, map_xt_brep};
use parasolid_core::{
    BuiltinProfileRegistry, ComparisonOptions, DocumentLimits, InMemorySchemaProvider, InspectionLimits, ParseError, SchemaKey, SchemaProvider,
    compare_xb_documents, compare_xt_xb_documents, inspect_xb, inspect_xt, parse_xb, parse_xt,
};
use serde_json::{Value, json};

fn brep_summary(model: &BrepModel) -> Value {
    json!({
        "complete": model.complete,
        "counts": {
            "bodies": model.bodies.len(), "regions": model.regions.len(),
            "shells": model.shells.len(), "faces": model.faces.len(),
            "loops": model.loops.len(), "half_edges": model.half_edges.len(),
            "edges": model.edges.len(), "vertices": model.vertices.len(),
            "points": model.points.len(), "curves": model.curves.len(),
            "surfaces": model.surfaces.len(),
        },
        "topology": {
            "valid": model.topology.valid,
            "closed_loop_count": model.topology.closed_loop_count,
            "closed_edge_ring_count": model.topology.closed_edge_ring_count,
            "euler_characteristic": model.topology.euler_characteristic,
        },
        "diagnostic_codes": model.diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
    })
}

fn document<P: SchemaProvider>(bytes: &[u8], text: bool, brep: bool, key: &str, provider: &P) -> support::Result<Value> {
    let limits = DocumentLimits::default();
    let options = ComparisonOptions { absolute_tolerance: 0.0, relative_tolerance: 0.0, ..ComparisonOptions::default() };
    let (nodes, start, end, model, reencoded, replayed) = if text {
        let doc = parse_xt(bytes, provider, limits)?;
        let encoded = support::encode_text_document_nodes(key, &doc.nodes)?;
        let other = parse_xb(&encoded, provider, limits)?;
        (
            support::nodes_json(&doc.nodes),
            doc.terminator.byte_range.start,
            doc.terminator.byte_range.end,
            if brep { Some(map_xt_brep(&doc)?) } else { None },
            compare_xt_xb_documents(&doc, &other, options)?.equivalent,
            false, // Embedded text schemas are encoded from decoded edits/fields.
        )
    } else {
        let doc = parse_xb(bytes, provider, limits)?;
        let encoded = support::encode_document_nodes(key, &doc.nodes)?;
        let other = parse_xb(&encoded, provider, limits)?;
        (
            support::nodes_json(&doc.nodes),
            doc.terminator.byte_range.start,
            doc.terminator.byte_range.end,
            if brep { Some(map_xb_brep(&doc)?) } else { None },
            compare_xb_documents(&doc, &other, options)?.equivalent,
            doc.nodes.iter().any(|n| n.first_schema.is_some()),
        )
    };
    if end != bytes.len() || !reencoded {
        return Err("termination or independent value reencoding differs".into());
    }
    Ok(json!({
        "status": "parsed", "schema_key": key, "nodes": nodes,
        "termination": [start, end], "brep": model.as_ref().map(brep_summary),
        "value_reencoding": true, "schema_blobs_replayed": replayed,
    }))
}

fn run() -> support::Result<Value> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() != 3 || !["text", "binary"].contains(&args[1].as_str()) || !["raw", "brep"].contains(&args[2].as_str()) {
        return Err("usage: release_corpus PATH text|binary raw|brep".into());
    }
    let bytes = fs::read(&args[0])?;
    let text = args[1] == "text";
    let key =
        if text { inspect_xt(&bytes, InspectionLimits::default())?.schema_key } else { inspect_xb(&bytes, InspectionLimits::default())?.schema_key };
    let registry = BuiltinProfileRegistry::compiled()?;
    let provider = registry.provider_for_key(&SchemaKey::parse(&key)?);
    let mut result = if let Some(provider) = &provider {
        if support::profile_hash(provider.profile())? != provider.profile().metadata().profile_sha256 {
            return Err("compiled profile hash differs from its definition".into());
        }
        document(&bytes, text, args[2] == "brep", &key, provider)?
    } else {
        document(&bytes, text, args[2] == "brep", &key, &InMemorySchemaProvider::new())?
    };
    result["profile"] = provider.map_or(Value::Null, |p| {
        let m = p.profile().metadata();
        json!({"kind": "builtin", "profile_id": m.profile_id,
            "profile_revision": m.revision, "schema_key": key,
            "profile_sha256": m.profile_sha256, "coverage": m.coverage.as_str()})
    });
    Ok(result)
}

fn main() {
    let (mut result, exit) = match run() {
        Ok(value) => (value, 0),
        Err(error) => {
            let value = if let Some(error) = error.downcast_ref::<ParseError>() {
                json!({"status": "diagnostic", "code": error.kind().code(), "offset": error.offset()})
            } else {
                json!({"status": "failed", "error": error.to_string()})
            };
            (value, 1)
        }
    };
    result["core_version"] = json!(env!("CARGO_PKG_VERSION"));
    println!("{result}");
    process::exit(exit);
}
