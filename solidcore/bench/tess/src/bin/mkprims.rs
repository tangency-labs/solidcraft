use solidcore_record::Vec3;
use solidcraft_kernel as k;
fn main() {
    let out = std::env::args().nth(1).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    let bodies: Vec<(&str, k::Body)> = vec![
        ("prim-box-20x10x5", k::box_solid(Vec3::ZERO, Vec3::new(20.0, 10.0, 5.0)).unwrap()),
        ("prim-box-40x30x20", k::box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap()),
        ("prim-cube-10", k::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap()),
        ("prim-cylinder-r5-h20", k::cylinder(Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), 5.0, 20.0).unwrap()),
        ("prim-sphere-r10", k::sphere(Vec3::ZERO, 10.0).unwrap()),
        ("prim-torus-20-4", k::torus(Vec3::ZERO, 20.0, 4.0).unwrap()),
    ];
    for (n, b) in bodies {
        std::fs::write(format!("{out}/{n}.nb"), k::encode_native_body(&b).unwrap()).unwrap();
    }
}
