// Parameters: `defineParams` declares them, the component gets plain numbers, and the compiler
// finds which numbers in the tree follow which parameter (see `link`), so each becomes a
// SolidCraft user parameter that the features' expressions name.
import { render, type Child, type Component, type Node } from "./element.ts";

export type ParamSpec =
  | { type: "number"; default: number; min?: number; max?: number; step?: number; unit?: string; label?: string; comment?: string }
  | { type: "int"; default: number; min?: number; max?: number; label?: string; comment?: string }
  | { type: "boolean"; default: boolean; label?: string; comment?: string }
  | { type: "choice"; default: string; options: readonly string[]; label?: string; comment?: string };
export type ParamSchema = Record<string, ParamSpec>;

export type ParamValues<S extends ParamSchema> = {
  [K in keyof S]: S[K] extends { type: "boolean" } ? boolean : S[K] extends { type: "choice"; options: readonly (infer O)[] } ? O : number;
};

const NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;

export function defineParams<const S extends ParamSchema>(schema: S): S {
  for (const [k, spec] of Object.entries(schema)) {
    if (!NAME.test(k)) throw new TypeError(`param "${k}": a SolidCraft parameter name is a letter or _, then letters, digits or _`);
    if ((spec.type === "number" || spec.type === "int") && !Number.isFinite(spec.default)) throw new TypeError(`param "${k}": default must be a finite number`);
    if (spec.type === "choice" && !spec.options.includes(spec.default)) throw new TypeError(`param "${k}": default is not one of the options`);
  }
  return schema;
}

/** The kind of quantity a parameter is, which decides where its name may appear. */
export type Kind = "length" | "angle" | "count";

export function paramKind(spec: ParamSpec): Kind | undefined {
  if (spec.type === "int") return "count";
  if (spec.type !== "number") return undefined;
  const u = (spec.unit ?? "mm").trim();
  if (u === "deg" || u === "°" || u === "rad") return "angle";
  if (u === "") return "count";
  return "length";
}

export function paramExpression(spec: ParamSpec, value: number): { expression: string; unit: string } {
  if (spec.type === "int") return { expression: String(value), unit: "" };
  const u = spec.type === "number" ? (spec.unit ?? "mm").trim() : "";
  const unit = u === "°" ? "deg" : u;
  return { expression: unit ? `${value} ${unit}` : String(value), unit };
}

export interface Module {
  default: Component;
  params?: ParamSchema;
}

/** A number in the tree: its value, and how much it moves per unit of each parameter. */
export class Lin {
  constructor(
    readonly v: number,
    readonly k: Readonly<Record<string, number>> = {},
  ) {}
  static of(v: number): Lin {
    return new Lin(v);
  }
  add(o: Lin | number): Lin {
    if (typeof o === "number") return new Lin(this.v + o, this.k);
    const k: Record<string, number> = { ...this.k };
    for (const [n, d] of Object.entries(o.k)) k[n] = (k[n] ?? 0) + d;
    return new Lin(this.v + o.v, k);
  }
  sub(o: Lin | number): Lin {
    return this.add(typeof o === "number" ? -o : o.scale(-1));
  }
  scale(s: number): Lin {
    const k: Record<string, number> = {};
    for (const [n, d] of Object.entries(this.k)) k[n] = d * s;
    return new Lin(this.v * s, k);
  }
  get linked(): boolean {
    return Object.values(this.k).some((d) => Math.abs(d) > 1e-12);
  }
}

export interface Linked {
  /** The tree at the parameters' values, every number in it a Lin. */
  nodes: Node[];
  values: Record<string, unknown>;
  /** Parameters that changed the tree's shape when moved: their numbers are kept as values. */
  structural: string[];
  /** Props whose number follows a parameter in a way that is not linear. */
  nonlinear: string[];
}

/**
 * Renders the component at the parameters' values and once more with each numeric parameter
 * moved up and down a little. A number in the tree that moves in proportion with a parameter
 * (`t`, `t / 2 + 3`, `w - 2 * wall`) is that parameter's expression; one that does not (`60 / n`)
 * keeps its value, and is listed in `nonlinear`. A parameter that changes the tree's shape when
 * moved (a count that adds elements, a boolean) keeps its value everywhere, listed in `structural`.
 */
export function link(mod: Module, overrides: Record<string, unknown> = {}): Linked {
  const schema = mod.params ?? {};
  const values: Record<string, unknown> = {};
  for (const [k, s] of Object.entries(schema)) values[k] = k in overrides ? overrides[k] : s.default;
  for (const k of Object.keys(overrides)) if (!(k in schema)) throw new TypeError(`unknown param "${k}"`);
  const draw = (v: Record<string, unknown>) => render(mod.default(v) as Child);
  const base = draw(values);
  const structural: string[] = [];
  const nonlinear = new Set<string>();
  const sens: { name: string; up: Node[]; down: Node[]; h: number }[] = [];
  for (const [name, spec] of Object.entries(schema)) {
    if (spec.type !== "number" && spec.type !== "int") {
      structural.push(name);
      continue;
    }
    const v = values[name] as number;
    // A count moves by whole steps; a length by a step small enough to stay inside the
    // ranges that conditionals in the component usually test.
    const h = spec.type === "int" ? 1 : Math.max(Math.abs(v) * 1e-3, 1e-3);
    const up = draw({ ...values, [name]: v + h });
    const down = draw({ ...values, [name]: v - h });
    if (!sameShape(base, up) || !sameShape(base, down)) {
      structural.push(name);
      continue;
    }
    sens.push({ name, up, down, h });
  }
  const nodes = base.map((n, i) =>
    mapNode(
      n,
      sens.map((s) => ({ ...s, up: s.up[i]!, down: s.down[i]! })),
      nonlinear,
      n.tag,
    ),
  );
  return { nodes, values, structural, nonlinear: [...nonlinear] };
}

type Lane = { name: string; up: Node; down: Node; h: number };

function mapNode(n: Node, lanes: Lane[], nonlinear: Set<string>, path: string): Node {
  const props: Record<string, unknown> = {};
  for (const [key, val] of Object.entries(n.props))
    props[key] = mapValue(
      val,
      lanes.map((l) => ({ ...l, up: l.up.props[key], down: l.down.props[key] })) as never,
      nonlinear,
      `${path}.${key}`,
    );
  const children = n.children.map((c, i) =>
    mapNode(
      c,
      lanes.map((l) => ({ ...l, up: l.up.children[i]!, down: l.down.children[i]! })),
      nonlinear,
      `${path}/${c.tag}`,
    ),
  );
  return { ...n, props, children };
}

type ValLane = { name: string; up: unknown; down: unknown; h: number };

function mapValue(v: unknown, lanes: ValLane[], nonlinear: Set<string>, path: string): unknown {
  if (typeof v === "number") {
    const k: Record<string, number> = {};
    for (const l of lanes) {
      const up = l.up as number;
      const down = l.down as number;
      const d1 = (up - v) / l.h;
      const d2 = (v - down) / l.h;
      const scale = Math.max(1, Math.abs(d1), Math.abs(d2));
      if (Math.abs(d1 - d2) > 1e-6 * scale) {
        nonlinear.add(`${path} (${l.name})`);
        continue;
      }
      const d = round((d1 + d2) / 2);
      if (d !== 0) k[l.name] = d;
    }
    return new Lin(v, k);
  }
  if (Array.isArray(v)) return v.map((x, i) => mapValue(x, lanes.map((l) => ({ ...l, up: (l.up as unknown[])[i], down: (l.down as unknown[])[i] })), nonlinear, `${path}[${i}]`));
  if (v && typeof v === "object" && Object.getPrototypeOf(v) === Object.prototype) {
    const o: Record<string, unknown> = {};
    for (const [key, x] of Object.entries(v)) o[key] = mapValue(x, lanes.map((l) => ({ ...l, up: (l.up as Record<string, unknown>)[key], down: (l.down as Record<string, unknown>)[key] })), nonlinear, `${path}.${key}`);
    return o;
  }
  return v;
}

/** A sensitivity read back from a finite difference, snapped to the nearest simple fraction. */
function round(d: number): number {
  for (const den of [1, 2, 3, 4, 5, 6, 8, 10, 12, 16, 100]) {
    const n = Math.round(d * den);
    if (Math.abs(d * den - n) < 1e-6 * Math.max(1, Math.abs(d * den))) return n / den;
  }
  return d;
}

function sameShape(a: Node[], b: Node[]): boolean {
  if (a.length !== b.length) return false;
  return a.every((n, i) => sameNode(n, b[i]!));
}
function sameNode(a: Node, b: Node): boolean {
  if (a.tag !== b.tag || a.children.length !== b.children.length) return false;
  if (!sameValueShape(a.props, b.props)) return false;
  return a.children.every((c, i) => sameNode(c, b.children[i]!));
}
function sameValueShape(a: unknown, b: unknown): boolean {
  if (typeof a === "number") return typeof b === "number";
  // a selector or other object with a JSON form: the same shape when its JSON has it
  if (a && typeof a === "object" && typeof (a as { toJSON?: unknown }).toJSON === "function")
    return !!b && typeof (b as { toJSON?: unknown }).toJSON === "function" && sameValueShape((a as { toJSON(): unknown }).toJSON(), (b as { toJSON(): unknown }).toJSON());
  if (Array.isArray(a)) return Array.isArray(b) && a.length === b.length && a.every((x, i) => sameValueShape(x, b[i]));
  if (a && typeof a === "object" && Object.getPrototypeOf(a) === Object.prototype) {
    if (!b || typeof b !== "object") return false;
    const ka = Object.keys(a);
    const kb = Object.keys(b as object);
    return ka.length === kb.length && ka.every((k) => sameValueShape((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]));
  }
  if (typeof a === "function") return typeof b === "function";
  return JSON.stringify(a) === JSON.stringify(b);
}
