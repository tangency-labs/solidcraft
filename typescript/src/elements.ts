// The intrinsic elements' props, for the editor. Numbers are mm and degrees.
import type { Child } from "./element.ts";
import type { Sel } from "./select.ts";

type Vec2 = [number, number];
type Vec3 = [number, number, number];
type Children = { children?: Child };

/** Moves an element by x, y, z (added to `at`). */
interface Placement {
  x?: number;
  y?: number;
  z?: number;
  at?: Vec3;
  /** Feature name in SolidCraft's timeline (default: the element and its line). */
  name?: string;
}
interface Placement2D {
  x?: number;
  y?: number;
  at?: Vec2;
}

export interface PartProps extends Placement, Children {
  /** The body's name in SolidCraft. */
  name?: string;
  /** A SolidCraft material (Steel, Aluminum, PLA, PETG, ...). */
  material?: string;
}
export interface BoxProps extends Placement {
  size?: number | Vec3;
  w?: number;
  d?: number;
  h?: number;
  width?: number;
  depth?: number;
  height?: number;
  /** Centre on the placement (per axis with [x, y, z]); default: lower corner there. */
  center?: boolean | [boolean, boolean, boolean];
}
interface Round {
  r?: number;
  radius?: number;
  d?: number;
  diameter?: number;
}
export interface CylinderProps extends Placement, Round {
  h?: number;
  height?: number;
  /** Centre along Z; default: the base on the placement. */
  center?: boolean;
}
export interface ConeProps extends Placement {
  r1?: number;
  r2?: number;
  d1?: number;
  d2?: number;
  h?: number;
  height?: number;
  center?: boolean;
}
export interface SphereProps extends Placement, Round {}
export interface TorusProps extends Placement {
  R: number;
  r: number;
}
export interface ExtrudeProps extends Placement, Children {
  h?: number;
  height?: number;
  distance?: number;
  /** Taper, degrees (positive grows outward). */
  draft?: number;
  /** Sketch plane: XY (default), XZ (sketch y is +Z) or YZ. */
  plane?: "XY" | "XZ" | "YZ";
}
/** Revolves 2D shapes about Z: sketch x is the radius, sketch y the height. */
export interface RevolveProps extends Placement, Children {
  angle?: number;
}
/** Skins 2D sections stacked along Z: give each shape a `z`, or wrap it in `<section z>`. */
export interface LoftProps extends Placement, Children {}
export interface SectionProps extends Children {
  z: number;
}
export interface RectProps extends Placement2D {
  w?: number;
  h?: number;
  width?: number;
  height?: number;
  size?: number | Vec2;
  /** false puts the lower-left corner on the position. Default true. */
  center?: boolean;
  /** Inside <loft>: the section's height. */
  z?: number;
}
export interface RoundedRectProps extends RectProps {
  r?: number;
  radius?: number;
}
export interface CircleProps extends Placement2D, Round {
  z?: number;
}
export interface PolygonProps extends Placement2D {
  points: Vec2[];
  z?: number;
}
export interface SlotProps extends Placement2D {
  /** Overall length along sketch X. */
  length: number;
  width: number;
}
export interface FilletProps extends Placement, Children {
  radius?: number;
  r?: number;
  /** Which edges of the body: `edges.vertical()`, `edges.top()`, ... (default all). */
  edges?: Sel;
}
export interface ChamferProps extends Placement, Children {
  distance?: number;
  d?: number;
  edges?: Sel;
}
/** A hole drilled into the body's face at its position (x, y, z on the face). */
export interface HoleProps extends Placement {
  d?: number;
  diameter?: number;
  /** A clearance hole for this screw ("M3"), ISO 273. */
  size?: string;
  fit?: "close" | "normal" | "loose";
  /** Default: through all. */
  depth?: number;
  counterbore?: { d: number; depth: number };
  countersink?: { d: number; angle?: number };
  /** With size: the screw head's recess, "ISO 4762" (socket head), "ISO 7380-1", "ISO 10642". */
  head?: string;
  direction?: Vec3;
}
export interface PolarPatternProps extends Placement, Children {
  count: number;
  angle?: number;
  axis?: "x" | "y" | "z";
  /** Move the children out along +X first (bolt circles). */
  radius?: number;
}
export interface LinearPatternProps extends Placement, Children {
  count: number;
  step?: Vec3;
  dx?: number;
  dy?: number;
  dz?: number;
}
export interface RotateProps extends Children {
  /** Degrees about X, then Y, then Z. */
  x?: number;
  y?: number;
  z?: number;
  by?: Vec3;
  axis?: "x" | "y" | "z" | Vec3;
  angle?: number;
}
export interface TranslateProps extends Placement, Children {
  by?: Vec3;
}
export interface GroupProps extends Placement, Children {}

export interface IntrinsicElements {
  assembly: Children;
  part: PartProps;
  group: GroupProps;
  union: GroupProps;
  difference: GroupProps;
  subtract: GroupProps;
  intersection: GroupProps;
  intersect: GroupProps;
  translate: TranslateProps;
  rotate: RotateProps;
  box: BoxProps;
  cylinder: CylinderProps;
  cone: ConeProps;
  sphere: SphereProps;
  torus: TorusProps;
  extrude: ExtrudeProps;
  revolve: RevolveProps;
  loft: LoftProps;
  section: SectionProps;
  rect: RectProps;
  roundedRect: RoundedRectProps;
  circle: CircleProps;
  polygon: PolygonProps;
  slot: SlotProps;
  fillet: FilletProps;
  chamfer: ChamferProps;
  hole: HoleProps;
  polarPattern: PolarPatternProps;
  linearPattern: LinearPatternProps;
}
