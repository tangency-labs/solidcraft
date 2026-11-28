// Each example builds into a SolidCraft design, saves, reopens and rebuilds to the same body,
// with the volume worked out by hand; and the saved design's parameters still drive it
// (`solidcraft-cli eval --param`), which only works when the history is real.
import { afterAll, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { build, locateCli, reopen, type Module } from "../src/index.ts";

const dir = mkdtempSync(join(tmpdir(), "solidcraft-ts-"));
afterAll(() => rmSync(dir, { recursive: true, force: true }));
const PI = Math.PI;

/** The material a fillet of radius r takes off a 90° edge (or adds in a 90° corner), per mm. */
const round = (r: number) => r * r * (1 - PI / 4);
/** A counterbored hole: the bore below the counterbore, and the counterbore. */
const cbore = (d: number, t: number, cd: number, depth: number) => PI * (d / 2) ** 2 * (t - depth) + PI * (cd / 2) ** 2 * depth;

interface Case {
  file: string;
  body: string;
  faces?: number;
  /** The volume at the given parameter values (the defaults when empty). */
  volume: (p: Record<string, number>) => number;
  changes: Record<string, number>[];
}

const cases: Case[] = [
  {
    file: "nema17-mount.tsx",
    body: "mount",
    faces: 23,
    volume: ({ size = 60, thickness: t = 6, corner = 6 }) => size * size * t - 4 * round(corner) * t - PI * 11.25 ** 2 * t - 4 * cbore(3.4, t, 6.5, 3),
    changes: [{ size: 80 }, { thickness: 8 }, { corner: 10 }],
  },
  {
    file: "bracket.tsx",
    body: "bracket",
    faces: 21,
    volume: ({ width = 50, leg = 40, t = 5, bend = 4 }) => width * (2 * leg * t - t * t) + round(bend) * width - 4 * cbore(4.5, t, 8, 2.5),
    changes: [{ width: 80 }, { leg: 60 }, { bend: 6 }],
  },
  {
    // Pappus: the revolved profile, less the top fillet's ring (its section's centroid sits
    // (10 - 3π)/(3(4 - π)) r inside the corner) and the blind bore.
    file: "knob.tsx",
    body: "knob",
    faces: 8,
    volume: ({ diameter = 30, height = 14, skirt = 4, shaft = 6 }) => {
      const r = diameter / 2;
      const hub = r - 3;
      const f = 2;
      const ring = round(f) * 2 * PI * (hub - (f * (10 - 3 * PI)) / (3 * (4 - PI)));
      return PI * r * r * skirt + PI * hub * hub * (height - skirt) - ring - PI * (shaft / 2) ** 2 * (height - 3);
    },
    changes: [{ diameter: 40 }, { height: 20 }, { shaft: 8 }],
  },
  {
    file: "flange.tsx",
    body: "flange",
    faces: 13,
    volume: ({ od = 80, bolts = 6, bore = 20 }) => {
      const hubR = bore / 2 + 8;
      const c = 1.5;
      return PI * (od / 2) ** 2 * 8 + PI * hubR * hubR * 12 - (c * c / 2) * 2 * PI * (hubR - c / 3) - PI * (bore / 2) ** 2 * 20 - bolts * PI * 3.3 ** 2 * 8;
    },
    changes: [{ bolts: 8 }, { od: 100 }, { bore: 16 }],
  },
  {
    // A loft between rectangles whose sides change linearly: h/6 (A0 + 4 Amid + A1).
    file: "tray.tsx",
    body: "tray",
    faces: 11,
    volume: ({ length = 80, width = 50, depth = 20, wall = 2 }) => {
      const prism = (h: number, a: [number, number], b: [number, number]) => (h / 6) * (a[0] * a[1] + (a[0] + b[0]) * (a[1] + b[1]) + b[0] * b[1]);
      const outer = prism(depth, [length - 10, width - 10], [length, width]);
      const inner = prism(depth - wall, [length - 10 - 2 * wall, width - 10 - 2 * wall], [length - 2 * wall, width - 2 * wall]);
      return outer - inner;
    },
    changes: [{ wall: 3 }, { depth: 30 }, { length: 120 }],
  },
];

const rel = (a: number, b: number) => Math.abs(a - b) / Math.max(1, Math.abs(b));

for (const c of cases) {
  test(`${c.file} builds a design that reopens, rebuilds and follows its parameters`, async () => {
    const cli = (await locateCli()).path;
    const mod = (await import(`../examples/${c.file}`)) as Module;
    const out = join(dir, c.file.replace(/\.tsx$/, ".sc"));
    const r = await build(mod, { out, cli });
    const body = r.bodies.find((b) => b.name === c.body);
    expect(body).toBeDefined();
    expect(r.bodies.length).toBe(1);
    expect(rel(body!.volume, c.volume({}))).toBeLessThan(1e-4);
    if (c.faces !== undefined) expect(body!.faces).toBe(c.faces);
    // every numeric parameter followed into an expression: nothing kept as a value
    expect(r.report.structural).toEqual([]);
    expect(r.report.fixed).toEqual([]);
    expect(r.report.nonlinear).toEqual([]);
    expect(r.reopened!.errors).toEqual([]);
    expect(r.reopened!.bodies.map((b) => b.name)).toEqual([c.body]);
    expect(rel(r.reopened!.bodies[0]!.volume, body!.volume)).toBeLessThan(1e-9);
    for (const change of c.changes) {
      const again = reopen(cli, out, Object.fromEntries(Object.entries(change).map(([k, v]) => [k, String(v)])));
      expect(again.errors).toEqual([]);
      expect({ change, rel: rel(again.bodies[0]!.volume, c.volume(change)) < 1e-4 }).toEqual({ change, rel: true });
    }
  });
}
