// The parts of the compiler that need no SolidCraft: following parameters through the tree,
// and reading edges for selectors.
import { expect, test } from "bun:test";
import { defineParams, edges, link, resolveEdges, type ModelEdge } from "../src/index.ts";
import { shape } from "../src/select.ts";

test("numbers that move with a parameter become its expression; others are reported", () => {
  const params = defineParams({
    w: { type: "number", default: 40, unit: "mm" },
    n: { type: "int", default: 4 },
    rounded: { type: "boolean", default: true },
  });
  const mod = {
    params,
    default: ({ w, n, rounded }: { w: number; n: number; rounded: boolean }) => (
      <part name="p">
        <box size={[w, w / 2 + 3, 60 / w]} />
        <linearPattern count={n} dx={w - 2 * 5}>
          <hole at={[0, 0, 0]} d={rounded ? 3 : 4} />
        </linearPattern>
      </part>
    ),
  };
  const l = link(mod);
  const box = l.nodes[0]!.children[0]!;
  const size = box.props.size as { v: number; k: Record<string, number> }[];
  expect(size[0]!.k).toEqual({ w: 1 });
  expect(size[1]!.k).toEqual({ w: 0.5 });
  expect(size[1]!.v).toBe(23);
  expect(l.nonlinear.some((s) => s.includes("size[2]"))).toBe(true);
  expect(l.structural).toEqual(["rounded"]);
  const pat = l.nodes[0]!.children[1]!;
  expect((pat.props.count as { k: Record<string, number> }).k).toEqual({ n: 1 });
  expect((pat.props.dx as { k: Record<string, number> }).k).toEqual({ w: 1 });
});

test("a parameter that adds elements keeps its value and is listed", () => {
  const params = defineParams({ n: { type: "int", default: 2 } });
  const mod = {
    params,
    default: ({ n }: { n: number }) => (
      <part>
        {Array.from({ length: n }, (_, i) => (
          <box size={[1, 1, 1]} x={i * 2} />
        ))}
      </part>
    ),
  };
  expect(link(mod).structural).toEqual(["n"]);
});

const e = (index: number, start: number[] | null, end: number[] | null, mid: number[], length: number): ModelEdge => ({
  index,
  name: null,
  start: start as never,
  end: end as never,
  mid: mid as never,
  length,
});

test("edges are told apart by their ends, midpoint and length", () => {
  expect(shape(e(0, [0, 0, 0], [0, 0, 5], [0, 0, 2.5], 5), 1e-6)).toEqual({ kind: "line", dir: [0, 0, 1] });
  const full = shape(e(1, [10, 0, 0], [10, 0, 0], [-10, 0, 0], 2 * Math.PI * 10), 1e-6);
  expect(full.kind).toBe("arc");
  expect((full as { radius: number }).radius).toBeCloseTo(10, 9);
  const s = Math.SQRT1_2 * 5;
  const quarter = shape(e(2, [5, 0, 0], [0, 5, 0], [s, s, 0], (Math.PI / 2) * 5), 1e-6);
  expect((quarter as { radius: number }).radius).toBeCloseTo(5, 9);
  const threeQuarter = shape(e(3, [5, 0, 0], [0, -5, 0], [-s, s, 0], 1.5 * Math.PI * 5), 1e-6);
  expect((threeQuarter as { radius: number }).radius).toBeCloseTo(5, 9);
});

test("selectors pick a box's edges as the names say", () => {
  // a 10 x 10 x 4 box from the origin: four edges on top, four on the bottom, four upright
  const box: ModelEdge[] = [];
  const c = [
    [0, 0],
    [10, 0],
    [10, 10],
    [0, 10],
  ];
  let i = 0;
  for (const z of [0, 4])
    for (let k = 0; k < 4; k++) {
      const a = c[k]!;
      const b = c[(k + 1) % 4]!;
      box.push(e(i++, [a[0]!, a[1]!, z], [b[0]!, b[1]!, z], [(a[0]! + b[0]!) / 2, (a[1]! + b[1]!) / 2, z], 10));
    }
  for (const a of c) box.push(e(i++, [a[0]!, a[1]!, 0], [a[0]!, a[1]!, 4], [a[0]!, a[1]!, 2], 4));
  expect(resolveEdges(edges.vertical(), box).length).toBe(4);
  expect(resolveEdges(edges.top(), box).map((x) => x.index)).toEqual([4, 5, 6, 7]);
  expect(resolveEdges(edges.bottom().and(edges.parallel("x")), box).length).toBe(2);
  expect(resolveEdges(edges.vertical().not(), box).length).toBe(8);
  expect(resolveEdges(edges.within([9, -1, -1], [11, 11, 5]), box).length).toBe(4); // the x = 10 side: two flat, two upright
  expect(resolveEdges(edges.circles(), box).length).toBe(0);
  expect(resolveEdges(undefined, box).length).toBe(12);
});
