// The compiler: a rendered tree of intrinsic elements → SolidCraft commands, run one by one on a
// live session, so a selector can be resolved against the body SolidCraft built at that step.
//
// Each element becomes the SolidCraft feature that does the same thing, so the design keeps a
// real history: primitives are solid.box/cylinder/sphere/cone/torus, 2D shapes are sketch curves
// under solid.extrude/revolve/loft, booleans are the cutting or joining operation of the tool
// feature itself (or solid.combine between built bodies), holes are solid.hole, patterns are
// solid.pattern.*. Numbers that follow a parameter are written as expressions naming it. An
// element with no equivalent is an error that names its line; nothing is dropped.
import { type Node, where } from "./element.ts";
import { Lin, type Kind, type ParamSchema, paramKind } from "./params.ts";
import { resolveEdges, Sel, type ModelEdge } from "./select.ts";
import type { Session } from "./session.ts";

type V3 = [Lin, Lin, Lin];
const ZERO: V3 = [Lin.of(0), Lin.of(0), Lin.of(0)];

export class CompileError extends Error {
  override name = "CompileError";
}

export interface Ctx {
  /** Translation applied to everything below (expressions follow parameters). */
  off: V3;
}

export interface CompileReport {
  /** One line per element that SolidCraft does not have directly, and what it became. */
  expanded: string[];
  /** Numbers kept as values although they depend on a parameter. */
  fixed: string[];
  /** The parts: body name, material. */
  parts: { name: string; body: string; material?: string }[];
}

const SOLIDS = new Set(["box", "cylinder", "sphere", "cone", "torus", "extrude", "revolve", "loft"]);
const SHAPES2D = new Set(["rect", "roundedRect", "circle", "polygon", "slot"]);

export class Compiler {
  private bodies = 0;
  private names = new Set<string>();
  readonly report: CompileReport = { expanded: [], fixed: [], parts: [] };

  constructor(
    private s: Session,
    private schema: ParamSchema,
  ) {}

  // -- numbers ---------------------------------------------------------------------------

  /** A number (or Lin) for a field of this kind: a plain number, or an expression naming parameters. */
  ex(x: unknown, kind: Kind, what: string): number | string {
    const l = lin(x, what);
    if (!l.linked) return tidy(l.v);
    const terms: string[] = [];
    let c = l.v;
    for (const [name, k] of Object.entries(l.k)) {
      if (Math.abs(k) < 1e-12) continue;
      const spec = this.schema[name];
      const pk = spec ? paramKind(spec) : undefined;
      if (!spec || !pk) continue;
      c -= k * (this.paramValue(name) as number);
      // a parameter of another kind is converted: n (no unit) in a length is n * 1 mm
      const conv = pk === kind ? "" : kind === "length" && pk === "count" ? " * 1 mm" : kind === "angle" && pk === "count" ? " * 1 deg" : kind === "count" && pk === "length" ? " / 1 mm" : kind === "count" && pk === "angle" ? " / 1 deg" : null;
      if (conv === null) {
        this.report.fixed.push(`${what}: ${tidy(l.v)} (follows ${name}, a ${pk}, in a ${kind} field)`);
        return tidy(l.v);
      }
      terms.push(term(k, name + conv));
    }
    if (!terms.length) return tidy(l.v);
    let out = terms.join(" + ").replace(/\+ -/g, "- ");
    const cc = tidy(c);
    if (Math.abs(cc) > 1e-9) {
      const unit = kind === "angle" ? " deg" : "";
      out += cc < 0 ? ` - ${tidy(-cc)}${unit}` : ` + ${cc}${unit}`;
    }
    return out;
  }

  paramValues: Record<string, unknown> = {};
  private paramValue(name: string): unknown {
    return this.paramValues[name];
  }

  private v3(x: unknown, what: string): V3 {
    if (!Array.isArray(x) || x.length !== 3) throw new CompileError(`${what} must be [x, y, z]`);
    return [lin(x[0], what), lin(x[1], what), lin(x[2], what)];
  }

  private pt(p: V3, what: string): (number | string)[] {
    return p.map((c, i) => this.ex(c, "length", `${what}[${i}]`));
  }

  /** Placement props (x, y, z, at) of a node added to the context's offset. */
  private place(n: Node, ctx: Ctx): Ctx {
    if (n.props.rotate !== undefined || n.props.rotateX !== undefined || n.props.rotateY !== undefined || n.props.rotateZ !== undefined)
      throw new CompileError(`${n.tag}${where(n.source)}: rotate a solid with <rotate> around it`);
    const at = n.props.at !== undefined ? this.v3(n.props.at, `${n.tag}.at`) : ZERO;
    const d: V3 = [at[0].add(lin(n.props.x ?? 0, "x")), at[1].add(lin(n.props.y ?? 0, "y")), at[2].add(lin(n.props.z ?? 0, "z"))];
    return { off: [ctx.off[0].add(d[0]), ctx.off[1].add(d[1]), ctx.off[2].add(d[2])] };
  }

  private featureName(n: Node, label: string): string {
    const given = typeof n.props.name === "string" && n.tag !== "part" ? n.props.name : undefined;
    const file = n.source?.fileName?.split(/[\\/]/).pop();
    let base = given ?? `${label}${n.source?.lineNumber ? ` ${file}:${n.source.lineNumber}` : ""}`;
    let name = base;
    for (let i = 2; this.names.has(name); i++) name = `${base} (${i})`;
    this.names.add(name);
    return name;
  }

  private newBody(): string {
    return `__tmp${++this.bodies}`;
  }

  private async run(command: string, params: Record<string, unknown>, n?: Node): Promise<any> {
    try {
      return await this.s.run(command as never, params as never);
    } catch (e) {
      throw new CompileError(`${n ? `<${n.tag}>${where(n.source)}` : command}: ${(e as Error).message}`);
    }
  }

  // -- the tree ---------------------------------------------------------------------------

  /** Every part of the document: a root that is not a part is one part named "part". */
  async document(nodes: Node[]): Promise<void> {
    const parts = nodes.length === 1 && nodes[0]!.tag === "assembly" ? nodes[0]!.children : nodes;
    if (parts.every((p) => p.tag === "part")) {
      for (const p of parts) await this.part(p, p.props.name as string | undefined);
    } else {
      await this.part({ tag: "part", props: { name: "part" }, children: parts }, "part");
    }
  }

  private async part(n: Node, name = "part"): Promise<void> {
    const body = await this.union(n.children, this.place(n, { off: ZERO }), n);
    await this.run("body.rename", { body, name }, n);
    const material = typeof n.props.material === "string" ? n.props.material : undefined;
    if (material) await this.material(name, material, n);
    this.report.parts.push({ name, body: name, material });
  }

  private made = new Set<string>();
  private async material(body: string, key: string, n: Node): Promise<void> {
    const builtIn = BUILT_IN[key.toLowerCase()];
    const extra = EXTRA[key.toLowerCase()];
    let name = builtIn ?? extra?.name ?? key;
    if (!builtIn && extra && !this.made.has(name)) {
      // a printing plastic SolidCraft's library does not have: made once, with its density
      await this.run("material.create", { name, density_g_cm3: extra.density, color: extra.color }, n);
      this.made.add(name);
    }
    await this.run("material.assign", { bodies: [body], material: name }, n);
  }

  /** Builds a solid node as a new body and returns its name. */
  async solid(n: Node, ctx: Ctx): Promise<string> {
    switch (n.tag) {
      case "union":
      case "group":
        return this.union(n.children, this.place(n, ctx), n);
      case "difference":
      case "subtract":
        return this.boolean(n, ctx, "cut");
      case "intersection":
      case "intersect":
        return this.boolean(n, ctx, "intersect");
      case "translate": {
        const by = n.props.by !== undefined ? this.v3(n.props.by, "translate.by") : ZERO;
        const c = this.place(n, ctx);
        return this.union(n.children, { off: [c.off[0].add(by[0]), c.off[1].add(by[1]), c.off[2].add(by[2])] }, n);
      }
      case "rotate":
        return this.rotate(n, ctx);
      case "fillet":
      case "chamfer":
        return this.edgeFeature(n, ctx);
      default:
        if (SOLIDS.has(n.tag)) {
          const body = this.newBody();
          await this.leaf(n, ctx, "new", undefined, body);
          return body;
        }
        throw this.unsupported(n);
    }
  }

  private unsupported(n: Node): CompileError {
    const where_ = where(n.source);
    if (SHAPES2D.has(n.tag)) return new CompileError(`<${n.tag}>${where_} is a 2D shape: put it in <extrude>, <revolve> or <loft>`);
    if (n.tag === "hole" || n.tag.endsWith("Pattern"))
      return new CompileError(`<${n.tag}>${where_} works on a body: put it after the body in <union> or <difference>`);
    return new CompileError(`<${n.tag}>${where_} is not supported by this package yet`);
  }

  private async union(children: Node[], ctx: Ctx, parent: Node): Promise<string> {
    if (!children.length) throw new CompileError(`<${parent.tag}>${where(parent.source)} is empty`);
    const [first, ...rest] = children;
    const target = await this.solid(first!, ctx);
    for (const c of rest) await this.apply(target, c, ctx, "join");
    return target;
  }

  private async boolean(n: Node, ctx: Ctx, op: "cut" | "intersect"): Promise<string> {
    const c = this.place(n, ctx);
    const [first, ...rest] = n.children;
    if (!first) throw new CompileError(`<${n.tag}>${where(n.source)} is empty`);
    const target = await this.solid(first, c);
    for (const t of rest) await this.apply(target, t, c, op);
    return target;
  }

  /**
   * Applies a tool to the target body: a primitive or sketch feature does it itself (its
   * `operation` and `targets`), a hole or pattern is its own feature, anything else is built as a
   * body of its own and combined. Returns the features it made, for patterns.
   */
  private async apply(target: string, t: Node, ctx: Ctx, op: "join" | "cut" | "intersect"): Promise<string[]> {
    if (SOLIDS.has(t.tag)) return [await this.leaf(t, ctx, op, target)];
    if (t.tag === "hole") {
      if (op !== "cut" && op !== "join") throw new CompileError(`<hole>${where(t.source)} cannot be intersected`);
      return [await this.hole(t, ctx, target)];
    }
    if (t.tag === "polarPattern" || t.tag === "linearPattern") return [await this.pattern(t, ctx, target, op)];
    if ((t.tag === "group" || t.tag === "union" || t.tag === "translate") && op !== "intersect") {
      // a group of tools applies each one: the same result as combining their union
      let c = this.place(t, ctx);
      if (t.tag === "translate" && t.props.by !== undefined) {
        const by = this.v3(t.props.by, "translate.by");
        c = { off: [c.off[0].add(by[0]), c.off[1].add(by[1]), c.off[2].add(by[2])] };
      }
      const out: string[] = [];
      for (const x of t.children) out.push(...(await this.apply(target, x, c, op)));
      return out;
    }
    const tool = await this.solid(t, ctx);
    const name = this.featureName(t, op === "join" ? "Join" : op === "cut" ? "Cut" : "Intersect");
    await this.run("solid.combine", { target, tools: [tool], operation: op, name }, t);
    return [name];
  }

  // -- leaves ------------------------------------------------------------------------------

  /** A primitive or sketch feature; with `target`, made with `op` into that body. */
  private async leaf(n: Node, ctx0: Ctx, op: "new" | "join" | "cut" | "intersect", target?: string, body?: string): Promise<string> {
    const ctx = this.place(n, ctx0);
    const p = n.props;
    const L = (x: unknown, what: string) => this.ex(x, "length", `<${n.tag}>${where(n.source)} ${what}`);
    const common: Record<string, unknown> = { operation: op };
    if (target) common.targets = [target];
    if (body) common.body_name = body;
    const o = ctx.off;
    switch (n.tag) {
      case "box": {
        const [w, d, h] = boxSize(p, n);
        const c = centerSpec(p.center);
        // tscad's box: lower corner at the placement unless centred (per axis)
        const ctr: V3 = [o[0].add(c[0] ? 0 : w.scale(0.5)), o[1].add(c[1] ? 0 : d.scale(0.5)), o[2].add(c[2] ? 0 : h.scale(0.5))];
        const name = this.featureName(n, "Box");
        await this.run("solid.box", { length: L(w, "width"), width: L(d, "depth"), height: L(h, "height"), center: this.pt(ctr, "box.center"), name, ...common }, n);
        return name;
      }
      case "cylinder":
      case "cone": {
        const h = lin(p.h ?? p.height, `${n.tag}.h`);
        const zc = centerSpec(p.center)[2];
        const base: V3 = [o[0], o[1], zc ? o[2].sub(h.scale(0.5)) : o[2]];
        const name = this.featureName(n, n.tag === "cone" ? "Cone" : "Cylinder");
        if (n.tag === "cylinder") {
          const r = radius(p, n);
          await this.run("solid.cylinder", { radius: L(r, "radius"), height: L(h, "height"), base: this.pt(base, "cylinder.base"), name, ...common }, n);
        } else {
          const r1 = p.r1 !== undefined ? lin(p.r1, "r1") : lin(p.d1, "d1").scale(0.5);
          const r2 = p.r2 !== undefined ? lin(p.r2, "r2") : p.d2 !== undefined ? lin(p.d2, "d2").scale(0.5) : Lin.of(0);
          await this.run("solid.cone", { radius: L(r1, "r1"), top_radius: L(r2, "r2"), height: L(h, "height"), base: this.pt(base, "cone.base"), name, ...common }, n);
        }
        return name;
      }
      case "sphere": {
        const name = this.featureName(n, "Sphere");
        await this.run("solid.sphere", { radius: L(radius(p, n), "radius"), center: this.pt(o, "sphere.center"), name, ...common }, n);
        return name;
      }
      case "torus": {
        const name = this.featureName(n, "Torus");
        await this.run("solid.torus", { major: L(p.R ?? p.major, "R"), minor: L(p.r ?? p.minor, "r"), center: this.pt(o, "torus.center"), name, ...common }, n);
        return name;
      }
      case "extrude": {
        const sketch = await this.sketch(n, n.children, ctx, (p.plane as string) ?? "XY", Lin.of(0));
        const name = this.featureName(n, "Extrude");
        const dist = p.h ?? p.height ?? p.distance;
        const params: Record<string, unknown> = { sketch, distance: L(dist, "height"), name, ...common };
        if (p.draft !== undefined) params.taper = this.ex(p.draft, "angle", "extrude.draft");
        if (op === "cut") params.operation = "cut";
        await this.run("solid.extrude", params, n);
        return name;
      }
      case "revolve": {
        const sketch = await this.sketch(n, n.children, ctx, "revolve", Lin.of(0));
        const name = this.featureName(n, "Revolve");
        const params: Record<string, unknown> = { sketch, axis: "y", name, ...common };
        if (p.angle !== undefined) params.angle = this.ex(p.angle, "angle", "revolve.angle");
        await this.run("solid.revolve", params, n);
        return name;
      }
      case "loft": {
        const sections: { sketch: string }[] = [];
        for (const c of n.children) {
          const shapes = c.tag === "section" ? c.children : [c];
          const z = lin(c.props.z ?? 0, "section.z");
          sections.push({ sketch: await this.sketch(c, shapes, ctx, "XY", z, true) });
        }
        const name = this.featureName(n, "Loft");
        await this.run("solid.loft", { sections, name, ...common }, n);
        return name;
      }
    }
    throw this.unsupported(n);
  }

  /** A sketch on a plane through the context's offset, with the 2D shapes drawn in it. */
  private async sketch(owner: Node, shapes: Node[], ctx: Ctx, plane: string, z: Lin, ignoreZ = false): Promise<string> {
    const o = ctx.off;
    const frames: Record<string, { x: number[]; y: number[] }> = {
      XY: { x: [1, 0, 0], y: [0, 1, 0] },
      XZ: { x: [1, 0, 0], y: [0, 0, 1] },
      YZ: { x: [0, 1, 0], y: [0, 0, 1] },
      revolve: { x: [1, 0, 0], y: [0, 0, 1] },
    };
    const f = frames[plane];
    if (!f) throw new CompileError(`<${owner.tag}>${where(owner.source)}: plane is XY, XZ or YZ`);
    const origin: V3 = [o[0], o[1], o[2].add(z)];
    const name = this.featureName(owner, owner.tag === "revolve" ? "Revolve profile" : owner.tag === "section" ? "Section" : `${cap(owner.tag)} sketch`);
    await this.run("sketch.create", { plane: { origin: this.pt(origin, "sketch origin"), x_dir: f.x, y_dir: f.y }, name }, owner);
    if (!shapes.length) throw new CompileError(`<${owner.tag}>${where(owner.source)} has no 2D shape`);
    for (const s of shapes) await this.shape2d(s, ignoreZ);
    await this.run("sketch.finish", {}, owner);
    return name;
  }

  private async shape2d(n: Node, ignoreZ: boolean): Promise<void> {
    const p = n.props;
    const L = (x: unknown, what: string) => this.ex(x, "length", `<${n.tag}>${where(n.source)} ${what}`);
    if (p.rotate !== undefined || p.mirror !== undefined) throw new CompileError(`<${n.tag}>${where(n.source)}: rotate and mirror are not supported on 2D shapes yet`);
    if (p.z !== undefined && !ignoreZ) throw new CompileError(`<${n.tag}>${where(n.source)}: z only applies inside <loft>`);
    const at = Array.isArray(p.at) ? [lin(p.at[0], "at"), lin(p.at[1], "at")] : [Lin.of(0), Lin.of(0)];
    const cx = at[0]!.add(lin(p.x ?? 0, "x"));
    const cy = at[1]!.add(lin(p.y ?? 0, "y"));
    const P = (x: Lin, y: Lin) => [L(x, "x"), L(y, "y")];
    switch (n.tag) {
      case "circle": {
        const r = radius(p, n);
        await this.run("sketch.circle.center", { center: P(cx, cy), radius: L(r, "radius") }, n);
        return;
      }
      case "rect":
      case "roundedRect": {
        const [w, h] = rectSize(p, n);
        const centred = p.center !== false;
        const x0 = centred ? cx.sub(w.scale(0.5)) : cx;
        const y0 = centred ? cy.sub(h.scale(0.5)) : cy;
        const x1 = x0.add(w);
        const y1 = y0.add(h);
        const r = await this.run("sketch.line", { points: [P(x0, y0), P(x1, y0), P(x1, y1), P(x0, y1)], closed: true }, n);
        if (n.tag === "roundedRect") {
          const rr = lin(p.r ?? p.radius, "roundedRect.r");
          if (rr.v > 0) {
            const ls: string[] = (r?.created?.curves ?? []) as string[];
            if (ls.length !== 4) throw new CompileError(`<roundedRect>${where(n.source)}: expected 4 lines, got ${JSON.stringify(r?.created)}`);
            const corners = [0, 1, 2, 3].map((i) => ({ a: ls[i], b: ls[(i + 1) % 4] }));
            await this.run("sketch.fillet", { points: corners, radius: L(rr, "radius") }, n);
            this.report.expanded.push(`<roundedRect>${where(n.source)}: four lines with sketch fillets`);
          }
        }
        return;
      }
      case "polygon": {
        const pts = p.points as unknown[];
        if (!Array.isArray(pts) || pts.length < 3) throw new CompileError(`<polygon>${where(n.source)} needs 3 or more points`);
        await this.run("sketch.line", { points: pts.map((q) => P(cx.add(lin((q as unknown[])[0], "x")), cy.add(lin((q as unknown[])[1], "y")))), closed: true }, n);
        return;
      }
      case "slot": {
        // a slot from its centre: overall length along sketch X and width
        const len = lin(p.length ?? p.w ?? p.l, "slot.length");
        const wd = lin(p.width ?? p.h ?? p.d, "slot.width");
        const half = len.sub(wd).scale(0.5);
        await this.run("sketch.slot.center_to_center", { p0: P(cx.sub(half), cy), p1: P(cx.add(half), cy), width: L(wd, "width") }, n);
        return;
      }
    }
    throw new CompileError(`<${n.tag}>${where(n.source)} is not a 2D shape this package can draw yet`);
  }

  // -- features on a body -------------------------------------------------------------------

  private async edgeFeature(n: Node, ctx: Ctx): Promise<string> {
    const c = this.place(n, ctx);
    const body = await this.union(n.children, c, n);
    const sel = n.props.edges as Sel | undefined;
    if (sel !== undefined && !(sel instanceof Sel)) throw new CompileError(`<${n.tag}>${where(n.source)}: edges must be a selector such as edges.vertical()`);
    const list = (await this.run("model.edges", { body }, n)) as { edges: ModelEdge[] };
    const chosen = resolveEdges(sel, list.edges);
    if (!chosen.length) throw new CompileError(`<${n.tag}>${where(n.source)}: the selector picks no edge of the body`);
    const name = this.featureName(n, n.tag === "fillet" ? "Fillet" : "Chamfer");
    const picks = chosen.map((e) => e.mid.map(tidy));
    if (n.tag === "fillet") {
      await this.run("solid.fillet", { body, edges: picks, radius: this.ex(n.props.radius ?? n.props.r, "length", "fillet.radius"), tangent_chain: false, name }, n);
    } else {
      await this.run("solid.chamfer", { body, edges: picks, distance: this.ex(n.props.distance ?? n.props.d, "length", "chamfer.distance"), tangent_chain: false, name }, n);
    }
    return body;
  }

  private async hole(n: Node, ctx0: Ctx, target: string): Promise<string> {
    const ctx = this.place(n, ctx0);
    const p = n.props;
    const L = (x: unknown, what: string) => this.ex(x, "length", `<hole>${where(n.source)} ${what}`);
    const params: Record<string, unknown> = { position: this.pt(ctx.off, "hole.position"), targets: [target] };
    if (p.size !== undefined) params.clearance = p.size;
    else params.diameter = L(p.d ?? p.diameter, "diameter");
    if (p.fit !== undefined) params.fit = p.fit;
    if (p.depth !== undefined) params.depth = L(p.depth, "depth");
    if (p.head !== undefined) params.head = p.head;
    const cb = p.counterbore as Record<string, unknown> | undefined;
    const cs = p.countersink as Record<string, unknown> | undefined;
    if (cb) Object.assign(params, { type: "counterbore", cb_diameter: L(cb.d ?? cb.diameter, "counterbore.d"), cb_depth: L(cb.depth, "counterbore.depth") });
    if (cs) Object.assign(params, { type: "countersink", cs_diameter: L(cs.d ?? cs.diameter, "countersink.d"), cs_angle: this.ex(cs.angle ?? 90, "angle", "countersink.angle") });
    if (p.direction !== undefined) params.direction = p.direction;
    params.name = this.featureName(n, cb ? "Counterbore hole" : cs ? "Countersink hole" : "Hole");
    await this.run("solid.hole", params, n);
    return params.name as string;
  }

  private async pattern(n: Node, ctx0: Ctx, target: string, op: "join" | "cut" | "intersect"): Promise<string> {
    const ctx = this.place(n, ctx0);
    const p = n.props;
    let inner = ctx;
    if (n.tag === "polarPattern" && p.radius !== undefined) inner = { off: [ctx.off[0].add(lin(p.radius, "radius")), ctx.off[1], ctx.off[2]] };
    if (n.tag === "polarPattern" && p.startAngle !== undefined && lin(p.startAngle, "startAngle").v !== 0)
      throw new CompileError(`<polarPattern>${where(n.source)}: startAngle is not supported yet; place the first copy with x/y`);
    // a linear pattern of one linear pattern is one feature with two directions
    const grid = n.tag === "linearPattern" && n.children.length === 1 && n.children[0]!.tag === "linearPattern" ? n.children[0]! : undefined;
    if (grid) inner = this.place(grid, inner);
    const features: string[] = [];
    for (const c of grid ? grid.children : n.children) {
      if (c.tag === "polarPattern" || c.tag === "linearPattern")
        throw new CompileError(`<${c.tag}>${where(c.source)}: SolidCraft does not pattern a pattern; put the copies in one <linearPattern> of a <linearPattern>, or pattern the feature itself`);
      features.push(...(await this.apply(target, c, inner, op)));
    }
    const name = this.featureName(n, n.tag === "polarPattern" ? "Circular pattern" : "Pattern");
    if (n.tag === "polarPattern") {
      const axis = ((p.axis as string) ?? "z").toLowerCase();
      const dir = axis === "x" ? [1, 0, 0] : axis === "y" ? [0, 1, 0] : [0, 0, 1];
      const params: Record<string, unknown> = { features, axis: { origin: this.pt(ctx0.off, "pattern axis"), dir }, count: this.ex(p.count, "count", "polarPattern.count"), name };
      if (p.angle !== undefined && lin(p.angle, "angle").v !== 360) params.angle = this.ex(p.angle, "angle", "polarPattern.angle");
      await this.run("solid.pattern.circular", params, n);
    } else {
      const params: Record<string, unknown> = { features, name };
      const dir = (m: Node, k: 1 | 2) => {
        const q = m.props;
        const step: V3 = q.step !== undefined ? this.v3(q.step, "step") : [lin(q.dx ?? 0, "dx"), lin(q.dy ?? 0, "dy"), lin(q.dz ?? 0, "dz")];
        if (q.center) throw new CompileError(`<linearPattern>${where(m.source)}: center is not supported yet; place the first copy instead`);
        const lenv = Math.hypot(step[0].v, step[1].v, step[2].v);
        if (lenv === 0) throw new CompileError(`<linearPattern>${where(m.source)} needs a step`);
        // the direction from the values; the spacing (the step's length) follows a parameter
        // when the step lies along one axis
        const axisOnly = step.filter((s) => s.v !== 0).length === 1;
        const spacing = axisOnly ? step.find((s) => s.v !== 0)! : Lin.of(lenv);
        params[`dir${k}`] = step.map((s) => tidy(s.v / lenv));
        params[`count${k}`] = this.ex(q.count, "count", "linearPattern.count");
        params[`spacing${k}`] = this.ex(spacing.v < 0 ? spacing.scale(-1) : spacing, "length", "linearPattern.step");
      };
      dir(n, 1);
      if (grid) dir(grid, 2);
      await this.run("solid.pattern.rectangular", params, n);
    }
    return name;
  }

  private async rotate(n: Node, ctx: Ctx): Promise<string> {
    // children are built where they are, then turned about the origin and moved to the offset
    const body = await this.union(n.children, { off: ZERO }, n);
    const p = n.props;
    const turns: { axis: number[]; angle: unknown }[] = [];
    if (p.axis !== undefined || p.angle !== undefined) {
      const a = p.axis ?? "z";
      const axis = typeof a === "string" ? { x: [1, 0, 0], y: [0, 1, 0], z: [0, 0, 1] }[a.toLowerCase()] : (a as unknown[]).map((c) => lin(c, "axis").v);
      if (!axis) throw new CompileError(`<rotate>${where(n.source)}: axis is x, y, z or [x, y, z]`);
      turns.push({ axis, angle: p.angle });
    } else {
      const by = (p.by as unknown[] | undefined) ?? [p.x ?? 0, p.y ?? 0, p.z ?? 0];
      [[1, 0, 0], [0, 1, 0], [0, 0, 1]].forEach((axis, i) => {
        if (lin(by[i] ?? 0, "rotate").v !== 0) turns.push({ axis, angle: by[i] });
      });
    }
    for (const t of turns) {
      await this.run("solid.move", { bodies: [body], axis: t.axis, angle: this.ex(t.angle, "angle", "rotate.angle"), origin: [0, 0, 0], name: this.featureName(n, "Rotate") }, n);
    }
    if (ctx.off.some((c) => c.v !== 0 || c.linked)) {
      await this.run("solid.move", { bodies: [body], translate: this.pt(ctx.off, "rotate placement"), name: this.featureName(n, "Move") }, n);
    }
    this.report.expanded.push(`<rotate>${where(n.source)}: Move features on the body it turns`);
    return body;
  }
}

// -- helpers -----------------------------------------------------------------------------------

function lin(x: unknown, what: string): Lin {
  if (x instanceof Lin) return x;
  if (typeof x === "number" && Number.isFinite(x)) return Lin.of(x);
  throw new CompileError(`${what} must be a number, got ${JSON.stringify(x)}`);
}

function tidy(v: number): number {
  const r = Math.round(v * 1e9) / 1e9;
  return Object.is(r, -0) ? 0 : r;
}

function term(k: number, name: string): string {
  if (k === 1) return name;
  if (k === -1) return `-${name}`;
  for (const den of [2, 3, 4, 5, 6, 8, 10, 12, 16]) {
    const num = k * den;
    if (Math.abs(num - Math.round(num)) < 1e-9) {
      const nn = Math.round(num);
      return nn === 1 ? `${name} / ${den}` : nn === -1 ? `-${name} / ${den}` : `${nn} * ${name} / ${den}`;
    }
  }
  return `${tidy(k)} * ${name}`;
}

function cap(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

function centerSpec(c: unknown): [boolean, boolean, boolean] {
  if (Array.isArray(c)) return [!!c[0], !!c[1], !!c[2]];
  return [!!c, !!c, !!c];
}

function boxSize(p: Record<string, unknown>, n: Node): [Lin, Lin, Lin] {
  if (p.size !== undefined) {
    if (Array.isArray(p.size)) return [lin(p.size[0], "size"), lin(p.size[1], "size"), lin(p.size[2], "size")];
    const s = lin(p.size, "size");
    return [s, s, s];
  }
  const w = p.w ?? p.width;
  const d = p.d ?? p.depth;
  const h = p.h ?? p.height;
  if (w === undefined || d === undefined || h === undefined) throw new CompileError(`<box>${where(n.source)} needs size or w, d, h`);
  return [lin(w, "w"), lin(d, "d"), lin(h, "h")];
}

function rectSize(p: Record<string, unknown>, n: Node): [Lin, Lin] {
  if (p.size !== undefined) {
    if (Array.isArray(p.size)) return [lin(p.size[0], "size"), lin(p.size[1], "size")];
    const s = lin(p.size, "size");
    return [s, s];
  }
  const w = p.w ?? p.width;
  const h = p.h ?? p.height;
  if (w === undefined || h === undefined) throw new CompileError(`<${n.tag}>${where(n.source)} needs size or w and h`);
  return [lin(w, "w"), lin(h, "h")];
}

function radius(p: Record<string, unknown>, n: Node): Lin {
  if (p.r !== undefined) return lin(p.r, "r");
  if (p.radius !== undefined) return lin(p.radius, "radius");
  if (p.d !== undefined) return lin(p.d, "d").scale(0.5);
  if (p.diameter !== undefined) return lin(p.diameter, "diameter").scale(0.5);
  throw new CompileError(`<${n.tag}>${where(n.source)} needs r or d`);
}

/** Short material keys to SolidCraft's library names. */
const BUILT_IN: Record<string, string> = {
  default: "Default", steel: "Steel", "stainless steel": "Stainless Steel", stainless: "Stainless Steel", aluminum: "Aluminum", aluminium: "Aluminum",
  brass: "Brass", copper: "Copper", titanium: "Titanium", "cast iron": "Cast Iron", abs: "ABS Plastic", "abs plastic": "ABS Plastic", pla: "PLA",
  nylon: "Nylon", pa: "Nylon", polycarbonate: "Polycarbonate", pc: "Polycarbonate", oak: "Oak", wood: "Oak", glass: "Glass",
};
/** Printing plastics the library lacks, made as materials of the design. */
const EXTRA: Record<string, { name: string; density: number; color: string }> = {
  petg: { name: "PETG", density: 1.27, color: "#4aa3df" },
  asa: { name: "ASA", density: 1.07, color: "#dcdcdc" },
  tpu: { name: "TPU", density: 1.21, color: "#333333" },
};
