// A revolved knob: a stepped profile turned about Z, a rounded top rim and a bore for the shaft.
import { defineParams, edges, type ParamValues } from "solidcraft";

export const params = defineParams({
  diameter: { type: "number", default: 30, min: 15, max: 60, unit: "mm" },
  height: { type: "number", default: 14, min: 8, max: 30, unit: "mm" },
  skirt: { type: "number", default: 4, min: 2, max: 8, unit: "mm" },
  shaft: { type: "number", default: 6, min: 3, max: 10, unit: "mm" },
});

export default function Knob({ diameter, height, skirt, shaft }: ParamValues<typeof params>) {
  const r = diameter / 2;
  const hub = r - 3;
  return (
    <part name="knob" material="abs">
      <difference>
        <fillet radius={2} edges={edges.top()}>
          <revolve>
            <polygon points={[[0, 0], [r, 0], [r, skirt], [hub, skirt], [hub, height], [0, height]]} />
          </revolve>
        </fillet>
        <hole at={[0, 0, 0]} d={shaft} depth={height - 3} />
      </difference>
    </part>
  );
}
