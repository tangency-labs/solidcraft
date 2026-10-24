use parasolid_core::brep::{map_xb_brep_with_diagnostic_limit, map_xt_brep_with_diagnostic_limit};
use parasolid_core::{BuiltinProfileRegistry, DocumentLimits, InspectionLimits, SchemaKey, inspect_xb, inspect_xt, parse_xb, parse_xt};
use serde_json::json;
use std::{env, fs, hint::black_box, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    assert_eq!(args.len(), 3, "mode encoding path");
    assert!(args[0] == "parse" || args[0] == "brep");
    assert!(args[1] == "binary" || args[1] == "text");
    let limits = DocumentLimits {
        max_file_size: 65536,
        max_nodes: 2048,
        max_schema_types: 1024,
        max_fields_per_type: 128,
        max_string_bytes: 8192,
        max_variable_elements: 4096,
    };
    assert!(fs::metadata(&args[2])?.len() <= limits.max_file_size as u64);
    let bytes = fs::read(&args[2])?;
    let inspection = InspectionLimits { max_file_size: 65536, max_string_bytes: 8192 };
    let key = if args[1] == "binary" { inspect_xb(&bytes, inspection)?.schema_key } else { inspect_xt(&bytes, inspection)?.schema_key };
    let registry = BuiltinProfileRegistry::compiled()?;
    let schema = SchemaKey::parse(&key)?;
    let provider = registry.provider_for_key(&schema).ok_or("unsupported exact key")?;
    // Each fresh process performs one warmup, then one measured call. Parent repeats 5 times.
    // B-Rep timing excludes parse; process peak RSS still includes the raw document and warmup.
    macro_rules! measure {
        ($parse:ident, $map:ident) => {{
            let document = $parse(&bytes, &provider, limits)?;
            let nodes = document.nodes.len();
            let (elapsed, bodies) = if args[0] == "parse" {
                drop(document);
                let started = Instant::now();
                let result = black_box($parse(black_box(&bytes), &provider, limits)?);
                let elapsed = started.elapsed().as_secs_f64();
                assert_eq!(result.nodes.len(), nodes);
                (elapsed, None)
            } else {
                drop(black_box($map(&document, 256)?));
                let started = Instant::now();
                let result = black_box($map(black_box(&document), 256)?);
                let elapsed = started.elapsed().as_secs_f64();
                assert!(result.complete);
                (elapsed, Some(result.bodies.len()))
            };
            println!("{}", json!({"status":"passed", "mode":args[0], "encoding":args[1],
                "seconds":elapsed, "nodes":nodes, "bodies":bodies, "schema_key":key,
                "core_version":env!("CARGO_PKG_VERSION"), "warmup":1}));
        }};
    }
    if args[1] == "binary" {
        measure!(parse_xb, map_xb_brep_with_diagnostic_limit);
    } else {
        measure!(parse_xt, map_xt_brep_with_diagnostic_limit);
    }
    Ok(())
}
