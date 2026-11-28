// A pipe flange: a disc and a hub, a chamfer round the hub's top, the bore, and a bolt circle
// of holes whose count is a parameter.
import { defineParams, edges, type ParamValues } from "solidcraft";

export const params = defineParams({
  od: { type: "number", default: 80, min: 50, max: 160, unit: "mm", label: "Outside diameter" },
  bcd: { type: "number", default: 60, min: 35, max: 140, unit: "mm", label: "Bolt circle" },
  bolts: { type: "int", default: 6, min: 3, max: 12 },
  bore: { type: "number", default: 20, min: 8, max: 40, unit: "mm" },
});

export default function Flange({ od, bcd, bolts, bore }: ParamValues<typeof params>) {
  const t = 8;
  const hub = 20;
  return (
    <part name="flange" material="steel">
      <difference>
        <chamfer distance={1.5} edges={edges.top()}>
          <cylinder d={od} h={t} />
          <cylinder d={bore + 16} h={hub} />
        </chamfer>
        <hole at={[0, 0, hub]} d={bore} />
        <polarPattern count={bolts} radius={bcd / 2}>
          <hole at={[0, 0, t]} d={6.6} />
        </polarPattern>
      </difference>
    </part>
  );
}
