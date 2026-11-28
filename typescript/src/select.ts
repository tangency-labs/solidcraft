// Edge selectors (`edges.vertical()`, `edges.top().and(edges.circles())`, ...). The compiler
// resolves one against the body SolidCraft built at that step (`model.edges`) and passes each
// chosen edge as the point halfway along it, the pick SolidCraft's own tools record: SolidCraft
// then keeps the fillet on that edge when the design is rebuilt or a parameter changes.
export type Axis = "x" | "y" | "z";
type Vec3 = [number, number, number];

/** One edge as `model.edges` lists it. */
export interface ModelEdge {
  index: number;
  name: string | null;
  start: Vec3 | null;
  end: Vec3 | null;
  mid: Vec3;
  length: number;
}

type Term =
  | { op: "all" }
  | { op: "none" }
  | { op: "parallel"; axis: Axis }
  | { op: "perpendicular"; axis: Axis }
  | { op: "circles"; axis: Axis }
  | { op: "radius"; r: number }
  | { op: "max"; axis: Axis }
  | { op: "min"; axis: Axis }
  | { op: "within"; min: Vec3; max: Vec3 }
  | { op: "index"; indices: number[] }
  | { op: "and"; of: Term[] }
  | { op: "or"; of: Term[] }
  | { op: "not"; of: Term };

export class Sel {
  constructor(readonly term: Term | undefined) {}
  private with(t: Term): Sel {
    return new Sel(this.term ? { op: "and", of: [this.term, t] } : t);
  }
  /** Every edge. */
  all(): Sel {
    return this.with({ op: "all" });
  }
  none(): Sel {
    return this.with({ op: "none" });
  }
  /** Edges lying wholly at the body's highest Z. */
  top(): Sel {
    return this.with({ op: "max", axis: "z" });
  }
  /** Edges lying wholly at the body's lowest Z. */
  bottom(): Sel {
    return this.with({ op: "min", axis: "z" });
  }
  /** Straight edges along Z. */
  vertical(): Sel {
    return this.with({ op: "parallel", axis: "z" });
  }
  /** Straight edges along the axis. */
  parallel(axis: Axis): Sel {
    return this.with({ op: "parallel", axis });
  }
  /** Straight edges square to the axis. */
  perpendicular(axis: Axis): Sel {
    return this.with({ op: "perpendicular", axis });
  }
  /** Circles and arcs about the axis (in a plane square to it). */
  circles(axis: Axis = "z"): Sel {
    return this.with({ op: "circles", axis });
  }
  /** Circles and arcs of this radius. */
  radius(r: number): Sel {
    return this.with({ op: "radius", r });
  }
  max(axis: Axis): Sel {
    return this.with({ op: "max", axis });
  }
  min(axis: Axis): Sel {
    return this.with({ op: "min", axis });
  }
  /** Edges whose points all lie in the box. */
  within(min: Vec3, max: Vec3): Sel {
    return this.with({ op: "within", min, max });
  }
  /** Edges by their index in `model.edges`: they change when the body is built differently. */
  index(...indices: (number | number[])[]): Sel {
    return this.with({ op: "index", indices: indices.flat() });
  }
  and(o: Sel): Sel {
    return new Sel({ op: "and", of: [this.term ?? { op: "all" }, o.term ?? { op: "all" }] });
  }
  or(o: Sel): Sel {
    return new Sel({ op: "or", of: [this.term ?? { op: "all" }, o.term ?? { op: "all" }] });
  }
  not(): Sel {
    return new Sel({ op: "not", of: this.term ?? { op: "all" } });
  }
  toJSON(): unknown {
    return this.term ?? { op: "all" };
  }
}

export const edges: Sel = new Sel(undefined);

const AX: Record<Axis, number> = { x: 0, y: 1, z: 2 };

/** The edges of `list` that `sel` chooses. */
export function resolveEdges(sel: Sel | undefined, list: ModelEdge[]): ModelEdge[] {
  const pts = list.flatMap((e) => samples(e));
  const lo: Vec3 = [0, 1, 2].map((i) => Math.min(...pts.map((p) => p[i]!))) as Vec3;
  const hi: Vec3 = [0, 1, 2].map((i) => Math.max(...pts.map((p) => p[i]!))) as Vec3;
  const size = Math.max(...hi.map((h, i) => h - lo[i]!), 1);
  const tol = 1e-6 * size + 1e-7;
  const test = (t: Term, e: ModelEdge): boolean => {
    const s = shape(e, tol);
    switch (t.op) {
      case "all":
        return true;
      case "none":
        return false;
      case "parallel": {
        if (s.kind !== "line") return false;
        const d = s.dir;
        return Math.abs(Math.abs(d[AX[t.axis]]!) - 1) < 1e-6;
      }
      case "perpendicular":
        return s.kind === "line" && Math.abs(s.dir[AX[t.axis]]!) < 1e-6;
      case "circles":
        return s.kind === "arc" && samples(e).every((p) => Math.abs(p[AX[t.axis]]! - e.mid[AX[t.axis]]!) < tol);
      case "radius":
        return s.kind === "arc" && s.radius !== undefined && Math.abs(s.radius - t.r) < 1e-4 * Math.max(1, t.r);
      case "max":
        return samples(e).every((p) => Math.abs(p[AX[t.axis]]! - hi[AX[t.axis]]!) < tol);
      case "min":
        return samples(e).every((p) => Math.abs(p[AX[t.axis]]! - lo[AX[t.axis]]!) < tol);
      case "within":
        return samples(e).every((p) => p.every((c, i) => c >= t.min[i]! - tol && c <= t.max[i]! + tol));
      case "index":
        return t.indices.includes(e.index);
      case "and":
        return t.of.every((x) => test(x, e));
      case "or":
        return t.of.some((x) => test(x, e));
      case "not":
        return !test(t.of, e);
    }
  };
  const term = sel?.term ?? { op: "all" };
  return list.filter((e) => test(term, e));
}

function samples(e: ModelEdge): Vec3[] {
  return [e.start, e.end, e.mid].filter((p): p is Vec3 => !!p);
}

type Shape = { kind: "line"; dir: Vec3 } | { kind: "arc"; radius?: number } | { kind: "curve" };

/**
 * What an edge is, from what `model.edges` gives (its ends, its midpoint and its length): a
 * straight edge is as long as its chord and has its midpoint on it; an edge whose three points and
 * length fit one circle is an arc (a closed edge, a full circle, has its midpoint a diameter away
 * from its start). Anything else is some other curve, which no line or circle selector picks.
 */
export function shape(e: ModelEdge, tol: number): Shape {
  const { start: a, end: b, mid: m, length: len } = e;
  if (!a || !b) return { kind: "curve" };
  const chord = dist(a, b);
  if (chord > tol && Math.abs(chord - len) < 1e-6 * Math.max(1, len)) {
    return { kind: "line", dir: [(b[0] - a[0]) / chord, (b[1] - a[1]) / chord, (b[2] - a[2]) / chord] };
  }
  if (chord <= tol) {
    // closed: a full circle has length 2πr and its midpoint 2r from its start
    const r = len / (2 * Math.PI);
    return Math.abs(dist(a, m) - 2 * r) < 1e-4 * Math.max(1, r) ? { kind: "arc", radius: r } : { kind: "curve" };
  }
  // an open arc: the circle through a, m, b; its length must match r·θ
  const r = circumradius(a, m, b);
  if (!Number.isFinite(r)) return { kind: "curve" };
  const half = Math.asin(Math.min(1, chord / (2 * r)));
  const long = dist(a, m) > Math.SQRT2 * r + tol; // past a half turn
  const theta = long ? 2 * Math.PI - 2 * half : 2 * half;
  return Math.abs(r * theta - len) < 1e-4 * Math.max(1, len) ? { kind: "arc", radius: r } : { kind: "curve" };
}

function dist(a: Vec3, b: Vec3): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
}
function circumradius(a: Vec3, b: Vec3, c: Vec3): number {
  const ab = dist(a, b);
  const bc = dist(b, c);
  const ca = dist(c, a);
  const u: Vec3 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
  const v: Vec3 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
  const cross = Math.hypot(u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]);
  return cross < 1e-12 ? Infinity : (ab * bc * ca) / (2 * cross);
}
