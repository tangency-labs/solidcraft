// An L bracket: a base and a wall, a fillet along the inside corner, and two counterbored M4
// holes in each leg.
import { defineParams, edges, type ParamValues } from "solidcraft";

export const params = defineParams({
  width: { type: "number", default: 50, min: 30, max: 120, unit: "mm" },
  leg: { type: "number", default: 40, min: 25, max: 80, unit: "mm" },
  t: { type: "number", default: 5, min: 3, max: 10, unit: "mm", label: "Thickness" },
  bend: { type: "number", default: 4, min: 1, max: 8, unit: "mm", label: "Inside radius" },
});

export default function Bracket({ width, leg, t, bend }: ParamValues<typeof params>) {
  const inset = 12;
  const at = leg / 2 + t / 2; // hole centres, halfway along the flat part of each leg
  return (
    <part name="bracket" material="aluminum">
      <difference>
        <fillet radius={bend} edges={edges.parallel("x").and(edges.within([-1, t - 0.01, t - 0.01], [width + 1, t + 0.01, t + 0.01]))}>
          <box w={width} d={leg} h={t} />
          <box w={width} d={t} h={leg} />
        </fillet>
        <linearPattern count={2} dx={width - 2 * inset}>
          <hole at={[inset, at, t]} d={4.5} counterbore={{ d: 8, depth: 2.5 }} />
        </linearPattern>
        <linearPattern count={2} dx={width - 2 * inset}>
          <hole at={[inset, t, at]} d={4.5} counterbore={{ d: 8, depth: 2.5 }} />
        </linearPattern>
      </difference>
    </part>
  );
}
