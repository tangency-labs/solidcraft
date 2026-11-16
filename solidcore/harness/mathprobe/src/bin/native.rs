// Reads inputs/<fn>.bin (little-endian f64, arity per sample), writes <out>/<fn>.bin.
use std::fs;
fn main() {
    let out = std::env::args().nth(1).expect("out dir");
    fs::create_dir_all(&out).unwrap();
    for (name, arity, f) in mathprobe::table() {
        let b = fs::read(format!("inputs/{name}.bin")).unwrap();
        let v: Vec<f64> = b
            .chunks_exact(8)
            .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let r: Vec<u8> = v
            .chunks_exact(arity)
            .flat_map(|a| f(a[0], if arity == 2 { a[1] } else { 0.0 }).to_le_bytes())
            .collect();
        fs::write(format!("{out}/{name}.bin"), r).unwrap();
    }
}
