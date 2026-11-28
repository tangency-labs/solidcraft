// A tapered tray: a loft from a large rounded rectangle to a smaller one, hollowed by a
// second loft cut into it.
import { defineParams, type ParamValues } from "solidcraft";

export const params = defineParams({
  length: { type: "number", default: 80, min: 40, max: 200, unit: "mm" },
  width: { type: "number", default: 50, min: 30, max: 150, unit: "mm" },
  depth: { type: "number", default: 20, min: 10, max: 60, unit: "mm" },
  wall: { type: "number", default: 2, min: 1, max: 5, unit: "mm" },
});

export default function Tray({ length, width, depth, wall }: ParamValues<typeof params>) {
  return (
    <part name="tray" material="pla">
      <difference>
        <loft>
          <rect w={length - 10} h={width - 10} z={0} />
          <rect w={length} h={width} z={depth} />
        </loft>
        <loft z={wall}>
          <rect w={length - 10 - 2 * wall} h={width - 10 - 2 * wall} z={0} />
          <rect w={length - 2 * wall} h={width - 2 * wall} z={depth - wall} />
        </loft>
      </difference>
    </part>
  );
}
