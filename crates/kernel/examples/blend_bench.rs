//! Time an exact fillet from a native body and its JSON edge picks. Input decoding,
//! validation and optional output encoding are outside the reported kernel time.
//! Usage: blend_bench body.bin params.json [runs=3] [result.bin]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use solidcraft_geom::Vec3;
use solidcraft_kernel as kernel;
use std::io::Read;
use std::time::Instant;

fn read(path: &str, limit: u64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("benchmark input exceeds size limit".into());
    }
    Ok(bytes)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let body_path = args.get(1).ok_or("body.bin params.json [runs=3] [result.bin]")?;
    let params_path = args.get(2).ok_or("params.json")?;
    let runs = args.get(3).map(|s| s.parse::<usize>()).transpose()?.unwrap_or(3).clamp(1, 9);
    let bytes = read(body_path, 256 << 20)?;
    let params: serde_json::Value = serde_json::from_slice(&read(params_path, 1 << 20)?)?;
    let edges =
        params.get("edges").and_then(|v| v.as_array()).filter(|edges| !edges.is_empty() && edges.len() <= 1000).ok_or("1..1000 edges required")?;
    let points: Vec<_> = edges
        .iter()
        .map(|v| -> Result<_, Box<dyn std::error::Error>> {
            let axis = |i| v.get(i).and_then(|v| v.as_f64()).filter(|x| x.is_finite()).ok_or("finite edge coordinate required");
            Ok(Vec3::new(axis(0)?, axis(1)?, axis(2)?))
        })
        .collect::<Result<_, _>>()?;
    let radius = params.get("radius").and_then(|v| v.as_f64()).ok_or("radius required")?;
    for run in 0..runs {
        let decode = Instant::now();
        let body = kernel::decode_native_body(&bytes)?;
        let decode_ms = decode.elapsed().as_secs_f64() * 1000.0;
        let start = Instant::now();
        let (result, times) = kernel::profile::capture(|| kernel::fillet(&body, &points, radius));
        let fillet_ms = start.elapsed().as_secs_f64() * 1000.0;
        let result = result?;
        if let Some(path) = args.get(4) {
            std::fs::write(path, kernel::encode_native_body(&result)?)?;
        }
        println!(
            "{}",
            serde_json::json!({"run":run,"decode_ms":decode_ms,"fillet_ms":fillet_ms,"tessellate_ms":times.tessellate_ms,"boolean_ms":times.boolean_ms,"faces":result.face_count(),"validity":result.validity()})
        );
    }
    Ok(())
}
