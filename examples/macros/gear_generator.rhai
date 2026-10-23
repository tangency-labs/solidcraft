// Full-depth involute spur gear (Machinery's Handbook)
// @param teeth Teeth int | 12
// @param gear_module Module number | 2
// @param bore Bore diameter length | 8
// @param thickness Thickness length | 8
// @param pa Pressure angle (deg) number | 20
fn clamp_unit(x) {
    if x > 1.0 { return 1.0; }
    if x < -1.0 { return -1.0; }
    x
}
fn inv_of(cos_a) {
    let a = acos(clamp_unit(cos_a));
    tan(a) - a
}
let z = teeth;
let m = gear_module;
let bore_d = bore;
let t = thickness;
let pi = 3.141592653589793;
let alpha = pa * pi / 180.0;
let r = m * z / 2.0;
let rb = r * cos(alpha);
let ra = r + m;
let rf = r - 1.25 * m;
let r_inner = if rb > rf { rb } else { rf };
if rf < m * 0.2 {
    throw "root diameter is too small for this teeth/module";
}
let inv_p = inv_of(cos(alpha));
let half_w = pi / (2.0 * z);
let n_flank = 16;
let n_tip = 5;
let n_root = 8;
let pts = [];
let tooth = 0;
while tooth < z {
    let phi = 2.0 * pi * tooth / z;
    let g_l = phi - half_w;
    let g_r = phi + half_w;
    let k = 0;
    while k <= n_flank {
        let rr = r_inner + (ra - r_inner) * (k * 1.0) / (n_flank * 1.0);
        let inv_r = inv_of(rb / rr);
        let ang = g_l + (inv_r - inv_p);
        pts.push([rr * cos(ang), rr * sin(ang)]);
        k = k + 1;
    }
    let inv_t = inv_of(rb / ra);
    let tip_l = g_l + (inv_t - inv_p);
    let tip_r = g_r - (inv_t - inv_p);
    let k = 1;
    while k < n_tip {
        let ang = tip_l + (tip_r - tip_l) * (k * 1.0) / (n_tip * 1.0);
        pts.push([ra * cos(ang), ra * sin(ang)]);
        k = k + 1;
    }
    let k = n_flank;
    while k >= 0 {
        let rr = r_inner + (ra - r_inner) * (k * 1.0) / (n_flank * 1.0);
        let inv_r = inv_of(rb / rr);
        let ang = g_r - (inv_r - inv_p);
        pts.push([rr * cos(ang), rr * sin(ang)]);
        k = k - 1;
    }
    let phi_n = 2.0 * pi * (tooth + 1.0) / z;
    let g_l_n = phi_n - half_w;
    let inv_i = inv_of(rb / r_inner);
    let a_r = g_r - (inv_i - inv_p);
    let a_l = g_l_n + (inv_i - inv_p);
    if r_inner > rf + m * 0.001 {
        pts.push([rf * cos(a_r), rf * sin(a_r)]);
    }
    let k = 1;
    while k < n_root {
        let ang = a_r + (a_l - a_r) * (k * 1.0) / (n_root * 1.0);
        pts.push([rf * cos(ang), rf * sin(ang)]);
        k = k + 1;
    }
    if r_inner > rf + m * 0.001 {
        pts.push([rf * cos(a_l), rf * sin(a_l)]);
    }
    tooth = tooth + 1;
}
run("sketch.create", #{ plane: "XY", name: "Gear" });
run("sketch.line", #{ points: pts, closed: true });
run("sketch.finish");
run("solid.extrude", #{ sketch: "Gear", distance: t, name: "Gear" });
run("solid.hole", #{ position: [0, 0, t], diameter: bore_d });
