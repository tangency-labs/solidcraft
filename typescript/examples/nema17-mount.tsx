// A NEMA 17 motor mount: a plate with rounded corners, the motor's pilot bore and four
// counterbored M3 holes on its 31 mm square.
import { defineParams, edges, type ParamValues } from "solidcraft";

export const params = defineParams({
  thickness: { type: "number", default: 6, min: 3, max: 12, unit: "mm" },
  size: { type: "number", default: 60, min: 45, max: 100, unit: "mm" },
  corner: { type: "number", default: 6, min: 1, max: 12, unit: "mm" },
});

const SPACING = 31; // NEMA 17 bolt square
const PILOT = 22;

export default function MotorMount({ thickness: t, size, corner }: ParamValues<typeof params>) {
  return (
    <part name="mount" material="PETG">
      <difference>
        <fillet radius={corner} edges={edges.vertical()}>
          <box size={[size, size, t]} center={[true, true, false]} />
        </fillet>
        <cylinder d={PILOT + 0.5} h={t} />
        <linearPattern count={2} dx={SPACING}>
          <linearPattern count={2} dy={SPACING}>
            <hole at={[-SPACING / 2, -SPACING / 2, t]} d={3.4} counterbore={{ d: 6.5, depth: 3 }} />
          </linearPattern>
        </linearPattern>
      </difference>
    </part>
  );
}
