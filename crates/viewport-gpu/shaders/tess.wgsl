// Display tessellation: surface points and normals at each vertex's (u, v).
//
// The GPU twin of the CPU surface evaluators (`EvalSurface::{derivs, eval}`): the same surface forms,
// the same algorithms (The NURBS Book A2.1 span search, A2.2 basis functions, A2.3 first
// derivatives, the rational quotient rule), in f32.

const PLANE: u32 = 0u;
const NURBS: u32 = 1u;
const REVOLVED: u32 = 2u;
// Basis slots for the highest degree (MAX_SPLINE_DEGREE = 16).
const W: u32 = 17u;

struct Surface {
    kind: u32,
    // Degrees and control point counts (u, v); a revolved surface's profile uses pu, nu.
    pu: u32,
    pv: u32,
    nu: u32,
    nv: u32,
    // Offsets into `data` (f32 units): knots along u, knots along v, homogeneous control
    // points (4 each, u fastest), and the placement (plane: origin, du, dv; revolved: origin,
    // axis, row-major 3×4 transform).
    knots_u: u32,
    knots_v: u32,
    ctrl: u32,
    extra: u32,
    // Revolved: parameters exchanged; mirror transform (normal reversed).
    swap: u32,
    mirror: u32,
    // NURBS: bit 0 the surface joins itself along u, bit 1 along v (the direction is periodic).
    closed: u32,
};

struct Vertex {
    u: f32,
    v: f32,
    surface: u32,
    // bit 0: normal reversed (face against its surface); bit 1: position fixed (p).
    flags: u32,
    p: vec4<f32>,
};

struct Params {
    count: u32,
    _p0: u32,
    _p1: u32,
    _p2: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> surfaces: array<Surface>;
@group(0) @binding(2) var<storage, read> data: array<f32>;
@group(0) @binding(3) var<storage, read> vertices: array<Vertex>;
// Out: position (xyz, w = 1 when evaluated) and normal (xyz) per vertex.
@group(0) @binding(4) var<storage, read_write> out: array<vec4<f32>>;

struct Derivs {
    p: vec3<f32>,
    du: vec3<f32>,
    dv: vec3<f32>,
    ok: bool,
};

fn vec3_at(o: u32) -> vec3<f32> {
    return vec3<f32>(data[o], data[o + 1u], data[o + 2u]);
}

fn ctrl_at(o: u32) -> vec4<f32> {
    return vec4<f32>(data[o], data[o + 1u], data[o + 2u], data[o + 3u]);
}

// The last non-empty span starting at or before t, clamped to p ≤ k < n (A2.1, by bisection
// over the knots, which do not decrease).
fn span_of(k0: u32, p: u32, n: u32, t: f32) -> u32 {
    if (t >= data[k0 + n]) {
        // At or past the end: the last non-empty span.
        var k = n - 1u;
        while (k > p && data[k0 + k] >= data[k0 + k + 1u]) {
            k = k - 1u;
        }
        return k;
    }
    if (t < data[k0 + p]) {
        return p;
    }
    var lo = p;
    var hi = n;
    // Invariant: knot[lo] ≤ t < knot[hi].
    while (hi - lo > 1u) {
        let mid = (lo + hi) / 2u;
        if (data[k0 + mid] <= t) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    return lo;
}

// Every loop here is a `while` with its counter stepped by hand, not a `for`: on an OpenCore Legacy
// Patcher Mac (AMD Radeon R9 M370X, Intel Iris Pro) the `for` forms hang the GPU and the machine
// restarts to the login window, while the same loops as `while` run (#3080).
//
// The arrays below are one-dimensional locals, written by loop counters and returned whole:
// D3D12's older shader compiler (FXC) must unroll a loop that writes an array inside a struct
// (or a two-dimensional one) by a counter, and it cannot unroll loops whose trip count is only
// known at run time, so such a loop fails to compile there ("unable to unroll loop").

// The non-zero basis functions of degree p on span `span` (The NURBS Book A2.2).
fn basis_funs(k0: u32, span: u32, p: u32, t: f32) -> array<f32, W> {
    var n: array<f32, W>;
    n[0] = 1.0;
    var j = 1u;
    while (j <= p) {
        var saved = 0.0;
        var r = 0u;
        while (r < j) {
            // right[r + 1] and left[j - r] of A2.2 (span ≥ p ≥ j, so no underflow).
            let right = data[k0 + span + r + 1u] - t;
            let left = t - data[k0 + span + 1u + r - j];
            let den = right + left;
            var temp = 0.0;
            if (den != 0.0) {
                temp = n[r] / den;
            }
            n[r] = saved + right * temp;
            saved = left * temp;
            r = r + 1u;
        }
        n[j] = saved;
        j = j + 1u;
    }
    return n;
}

// Their first derivatives, from the degree p - 1 functions on the same span (A2.3, n = 1).
fn basis_ders(k0: u32, span: u32, p: u32, t: f32) -> array<f32, W> {
    var d: array<f32, W>;
    if (p == 0u) {
        return d;
    }
    let lower = basis_funs(k0, span, p - 1u, t);
    var r = 0u;
    while (r <= p) {
        var x = 0.0;
        if (r >= 1u) {
            let den = data[k0 + span + r] - data[k0 + span + r - p];
            if (den != 0.0) {
                x = x + lower[r - 1u] / den;
            }
        }
        if (r < p) {
            let den = data[k0 + span + r + 1u] - data[k0 + span + r + 1u - p];
            if (den != 0.0) {
                x = x - lower[r] / den;
            }
        }
        d[r] = x * f32(p);
        r = r + 1u;
    }
    return d;
}

// Point and derivative of a rational function from its homogeneous value and derivative.
fn rational_point(a: vec4<f32>) -> vec3<f32> {
    return a.xyz / a.w;
}

fn rational_der(a: vec4<f32>, da: vec4<f32>) -> vec3<f32> {
    return (da.xyz - rational_point(a) * da.w) / a.w;
}

// A parameter outside the knot range of a direction the surface joins itself along, moved by
// whole ranges back into it (the CPU evaluator's `EvalSurface::derivs` does the same).
fn wrap_closed(knots: u32, p: u32, n: u32, t: f32, closed: bool) -> f32 {
    if (!closed) { return t; }
    let a = data[knots + p];
    let b = data[knots + n];
    if (!(b > a) || (t >= a && t <= b)) { return t; }
    return a + (t - a) - floor((t - a) / (b - a)) * (b - a);
}

fn nurbs(s: Surface, u_in: f32, v_in: f32) -> Derivs {
    var r: Derivs;
    let u = wrap_closed(s.knots_u, s.pu, s.nu, u_in, (s.closed & 1u) != 0u);
    let v = wrap_closed(s.knots_v, s.pv, s.nv, v_in, (s.closed & 2u) != 0u);
    let su = span_of(s.knots_u, s.pu, s.nu, u);
    let sv = span_of(s.knots_v, s.pv, s.nv, v);
    let bu0 = basis_funs(s.knots_u, su, s.pu, u);
    var bu1: array<f32, W>;
    if (s.pu > 0u) { bu1 = basis_funs(s.knots_u, su, s.pu - 1u, u); }
    let bv0 = basis_funs(s.knots_v, sv, s.pv, v);
    var bv1: array<f32, W>;
    if (s.pv > 0u) { bv1 = basis_funs(s.knots_v, sv, s.pv - 1u, v); }
    var a = vec4<f32>(0.0);
    var au = vec4<f32>(0.0);
    var av = vec4<f32>(0.0);
    var j = 0u;
    while (j <= s.pv) {
        var i = 0u;
        while (i <= s.pu) {
            let c = ctrl_at(s.ctrl + 4u * ((sv + j - s.pv) * s.nu + su + i - s.pu));
            a = a + bu0[i] * bv0[j] * c;
            i = i + 1u;
        }
        j = j + 1u;
    }

    // Differentiate control points before summing. Constant collapsed pole
    // rows then have exactly zero tangent instead of cancelling large terms.
    j = 0u;
    while (j <= s.pv) {
        var i = 0u;
        while (i < s.pu) {
            let ix = su + i - s.pu;
            let k = (sv + j - s.pv) * s.nu + ix;
            let den = data[s.knots_u + ix + s.pu + 1u] - data[s.knots_u + ix + 1u];
            if (den > 0.0) { au = au + bu1[i] * bv0[j] * (f32(s.pu) / den) * (ctrl_at(s.ctrl + 4u * (k + 1u)) - ctrl_at(s.ctrl + 4u * k)); }
            i = i + 1u;
        }
        j = j + 1u;
    }
    j = 0u;
    while (j < s.pv) {
        var i = 0u;
        while (i <= s.pu) {
            let iy = sv + j - s.pv;
            let k = iy * s.nu + su + i - s.pu;
            let den = data[s.knots_v + iy + s.pv + 1u] - data[s.knots_v + iy + 1u];
            if (den > 0.0) { av = av + bu0[i] * bv1[j] * (f32(s.pv) / den) * (ctrl_at(s.ctrl + 4u * (k + s.nu)) - ctrl_at(s.ctrl + 4u * k)); }
            i = i + 1u;
        }
        j = j + 1u;
    }
    r.ok = abs(a.w) > 0.0;
    if (r.ok) {
        r.p = rational_point(a);
        r.du = rational_der(a, au);
        r.dv = rational_der(a, av);
    }
    return r;
}

// Rodrigues rotation of x by angle (sin, cos) about the unit axis k.
fn rotate(x: vec3<f32>, k: vec3<f32>, sn: f32, cs: f32) -> vec3<f32> {
    return x * cs + cross(k, x) * sn + k * (dot(k, x) * (1.0 - cs));
}

fn revolved(s: Surface, u: f32, v: f32) -> Derivs {
    var r: Derivs;
    var t = u;
    var angle = v;
    if (s.swap != 0u) {
        t = v;
        angle = u;
    }
    // The profile curve: a one-row B-spline.
    let span = span_of(s.knots_u, s.pu, s.nu, t);
    let b0 = basis_funs(s.knots_u, span, s.pu, t);
    let b1 = basis_ders(s.knots_u, span, s.pu, t);
    var c = vec4<f32>(0.0);
    var dc = vec4<f32>(0.0);
    var i = 0u;
    while (i <= s.pu) {
        let h = ctrl_at(s.ctrl + 4u * (span + i - s.pu));
        c = c + b0[i] * h;
        dc = dc + b1[i] * h;
        i = i + 1u;
    }
    r.ok = abs(c.w) > 0.0;
    if (!r.ok) {
        return r;
    }
    let o = vec3_at(s.extra);
    let k = vec3_at(s.extra + 3u);
    let m = s.extra + 6u;
    let row0 = vec4<f32>(data[m], data[m + 1u], data[m + 2u], data[m + 3u]);
    let row1 = vec4<f32>(data[m + 4u], data[m + 5u], data[m + 6u], data[m + 7u]);
    let row2 = vec4<f32>(data[m + 8u], data[m + 9u], data[m + 10u], data[m + 11u]);
    let sn = sin(angle);
    let cs = cos(angle);
    let radius = rotate(rational_point(c) - o, k, sn, cs);
    let p = o + radius;
    let dt = rotate(rational_der(c, dc), k, sn, cs);
    let da = cross(k, radius);
    let pp = vec3<f32>(dot(row0.xyz, p) + row0.w, dot(row1.xyz, p) + row1.w, dot(row2.xyz, p) + row2.w);
    let ldt = vec3<f32>(dot(row0.xyz, dt), dot(row1.xyz, dt), dot(row2.xyz, dt));
    let lda = vec3<f32>(dot(row0.xyz, da), dot(row1.xyz, da), dot(row2.xyz, da));
    r.p = pp;
    if (s.swap != 0u) {
        r.du = lda;
        r.dv = ldt;
    } else {
        r.du = ldt;
        r.dv = lda;
    }
    return r;
}

fn derivs(s: Surface, u: f32, v: f32) -> Derivs {
    if (s.kind == PLANE) {
        var r: Derivs;
        r.du = vec3_at(s.extra + 3u);
        r.dv = vec3_at(s.extra + 6u);
        r.p = vec3_at(s.extra) + r.du * u + r.dv * v;
        r.ok = true;
        return r;
    }
    if (s.kind == NURBS) {
        return nurbs(s, u, v);
    }
    return revolved(s, u, v);
}

fn oriented(s: Surface, n: vec3<f32>) -> vec3<f32> {
    if (s.mirror != 0u) {
        return -n;
    }
    return n;
}

// A scale-free direction and the sine between the tangents. Normalize
// components before crossing: tiny valid tangents must not become a +Z pole.
fn stable_normal(a: vec3<f32>, b: vec3<f32>) -> vec4<f32> {
    let sa = max(max(abs(a.x), abs(a.y)), abs(a.z));
    let sb = max(max(abs(b.x), abs(b.y)), abs(b.z));
    if (!(sa > 0.0) || !(sb > 0.0)) { return vec4<f32>(0.0); }
    let ua = a / sa;
    let ub = b / sb;
    let n = cross(ua, ub);
    let sn = max(max(abs(n.x), abs(n.y)), abs(n.z));
    if (!(sn > 0.0)) { return vec4<f32>(0.0); }
    // Scale again before squaring components so a small angle does not
    // underflow the normal's length. A regular patch can have nearly parallel tangents.
    let scaled = n / sn;
    let size = length(scaled);
    if (!(size > 0.0)) { return vec4<f32>(0.0); }
    let sine = sn * size / (length(ua) * length(ub));
    return vec4<f32>(scaled / size, sine);
}

fn available_step(s: Surface, uv: vec2<f32>, positive: vec2<bool>, nominal: f32) -> f32 {
    if (s.kind == NURBS) {
        let lo = vec2<f32>(data[s.knots_u + s.pu], data[s.knots_v + s.pv]);
        let hi = vec2<f32>(data[s.knots_u + s.nu], data[s.knots_v + s.nv]);
        let room = select(uv - lo, hi - uv, positive);
        return min(room.x, room.y);
    }
    if (s.kind == REVOLVED) {
        let t = select(uv.x, uv.y, s.swap != 0u);
        let forward = select(positive.x, positive.y, s.swap != 0u);
        return select(t - data[s.knots_u + s.pu], data[s.knots_u + s.nu] - t, forward);
    }
    return nominal * 64.0;
}

// A geometric pole may have different homogeneous weights along its clamped
// boundary. Do not turn quotient roundoff into a normal after placement. Only
// degree+1 controls of the active span are inspected, at most W per direction.
fn collapsed_boundary(s: Surface, u: f32, v: f32) -> bool {
    if (s.kind != NURBS) { return false; }
    var d = 0u;
    while (d < 2u) {
        let k0 = select(s.knots_u, s.knots_v, d != 0u);
        let p = select(s.pu, s.pv, d != 0u);
        let n = select(s.nu, s.nv, d != 0u);
        let t = select(u, v, d != 0u);
        let along_v = d != 0u;
        d = d + 1u;
        var row = 0u;
        var inward = 1u;
        var clamped = true;
        var j = 0u;
        if (t == data[k0 + p]) {
            while (j <= p) {
                clamped = clamped && data[k0 + j] == t;
                j = j + 1u;
            }
        } else if (t == data[k0 + n]) {
            row = n - 1u;
            inward = row - 1u;
            while (j <= p) {
                clamped = clamped && data[k0 + n + j] == t;
                j = j + 1u;
            }
        } else { continue; }
        if (!clamped) { continue; }
        let po = select(s.pv, s.pu, along_v);
        let no = select(s.nv, s.nu, along_v);
        let ko = select(s.knots_v, s.knots_u, along_v);
        let to = select(v, u, along_v);
        let span = span_of(ko, po, no, to);
        var anchor = vec3<f32>(0.0);
        var scale = 0.0;
        var deviation = 0.0;
        var valid = true;
        j = 0u;
        while (j <= po) {
            let i = span + j - po;
            let index = select(i * s.nu + row, row * s.nu + i, along_v);
            let next_index = select(i * s.nu + inward, inward * s.nu + i, along_v);
            let h = ctrl_at(s.ctrl + 4u * index);
            let next = ctrl_at(s.ctrl + 4u * next_index);
            if (h.w == 0.0 || next.w == 0.0) { valid = false; break; }
            let point = h.xyz / h.w;
            if (j == 0u) { anchor = point; }
            let delta = abs(point - anchor);
            let local = abs(next.xyz / next.w - point);
            deviation = max(deviation, max(max(delta.x, delta.y), delta.z));
            scale = max(scale, max(max(local.x, local.y), local.z));
            j = j + 1u;
        }
        if (valid && deviation <= 7.62939453125e-6 * scale) { return true; }
    }
    return false;
}
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.count) {
        return;
    }
    let vert = vertices[i];
    let s = surfaces[vert.surface];
    let d = derivs(s, vert.u, vert.v);
    var n = oriented(s, stable_normal(d.du, d.dv).xyz);
    if (collapsed_boundary(s, vert.u, vert.v)) { n = vec3<f32>(0.0); }
    if (!(dot(n, n) > 0.0) || !d.ok) {
        // The first interior normal can be ill-conditioned at a higher-order
        // pole. Require agreement across bounded increasing interior steps.
        let h = 1e-4 * (1.0 + max(abs(vert.u), abs(vert.v)));
        var found = false;
        var k = 0u;
        while (k < 4u) {
            let positive = vec2<bool>((k & 1u) == 0u, (k & 2u) == 0u);
            let room = available_step(s, vec2<f32>(vert.u, vert.v), positive, h);
            k = k + 1u;
            if (!(room > 0.0)) { continue; }
            var step = min(h, room / 64.0);
            var previous = vec3<f32>(0.0);
            var attempt = 0u;
            while (attempt < 6u) {
                let delta = select(vec2<f32>(-step), vec2<f32>(step), positive);
                let e = derivs(s, vert.u + delta.x, vert.v + delta.y);
                let m = stable_normal(e.du, e.dv);
                if (e.ok && dot(m.xyz, m.xyz) > 0.0) {
                    if (m.w > 1e-2 || (dot(previous, previous) > 0.0 && distance(previous, m.xyz) < 1e-3)) {
                        n = oriented(s, m.xyz);
                        found = true;
                        break;
                    }
                    previous = m.xyz;
                }
                step = step * 2.0;
                attempt = attempt + 1u;
            }
            if (found) { break; }
        }
    }
    var nn = vec3<f32>(0.0, 0.0, 1.0);
    if (dot(n, n) > 0.0) { nn = n; }
    if ((vert.flags & 1u) != 0u) {
        nn = -nn;
    }
    var p = d.p;
    if ((vert.flags & 2u) != 0u) {
        p = vert.p.xyz;
    }
    out[2u * i] = vec4<f32>(p, 1.0);
    out[2u * i + 1u] = vec4<f32>(nn, 0.0);
}
