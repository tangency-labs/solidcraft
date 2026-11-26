# MJCF export

A design with components and joints exports as an MJCF model, MuJoCo's own format. It is the
same robot the [URDF export](urdf.md) writes: a body for each placement of a component, with its
mesh, mass and inertia, and joints that move the parts the way SolidCraft does. MJCF can say
more than URDF, so the model keeps what a URDF has to drop:

- A joint with several values (cylindrical, pin-slot, planar, parallel) is one body with a joint
  for each value. URDF needs a chain of links between them, each with a token mass.
- A ball joint is one MuJoCo `ball` joint, not three turns about z, y and z.
- A joint without a limit has no `range`. URDF needs ±1000 m for an unlimited slide.
- A joint that closes a loop is kept as an equality constraint. URDF is a tree and leaves it out.
- Motion links (gears, rack and pinion, screws) are equality constraints between two joints.

- **Desktop:** **File → Export MJCF…**, then a name and a place for the `.xml`
- **Commands:** `file.export_mjcf {"path": "robot.xml"}`, or `file.export {"path": "robot.xml",
  "format": "mjcf"}`
- **MCP:** the `export` tool with `format: "mjcf"`
- **CLI:** `solidcraft-cli run script.json --out robot.mjcf`

`.xml` is the usual name of an MJCF file, but a `.xml` path alone does not pick MJCF: it is any
XML, and a file extension SolidCraft does not write goes to the exporter plugin that claims it.
Name the format, or end the path in `.mjcf`, which MuJoCo reads as it reads any MJCF.

MJCF always exports the whole assembly, so `bodies` is refused. Load the file with MuJoCo
(`python -m mujoco.viewer --mjcf robot.xml`, or `mujoco.MjModel.from_xml_path`).

## Files

Next to `robot.xml` the export writes the binary STL meshes the URDF export writes, in metres and
with the same names ([Files](urdf.md#files)). Every path is checked before anything is written. The
meshes are written first and the `.xml` last.

## Bodies

- **The world** holds the root component's bodies and every grounded occurrence, which hangs on
  it without a joint.
- **One body per placement of a component,** nested in the body it hangs on, named as the URDF
  link is (`Gripper`, or `Finger_1` for a component placed several times). A part called `world`,
  MuJoCo's name for the world, becomes `world_2`.
- **Mass and inertia** are the URDF link's ([Links](urdf.md#links)), a component's mass override
  included: the mass in kg, the centre of mass and the inertia about it (`fullinertia`, kg m²) in
  the body frame.
- **Geoms:** the component's mesh, which collides as it is drawn unless the component has
  [collision shapes](urdf.md#collision-shapes) of its own. Then the mesh is only drawn
  (`contype="0" conaffinity="0"`), and each shape is a geom that collides (`group="3"`): a `box`,
  `cylinder` or `sphere`, or the mesh of the bodies that collide as theirs.
- **Colour:** the first body's colour, as the geom's `rgba`.
- **Frames** ([Frames](urdf.md#frames)) are `site`s of their part's body.

## Where the model starts

Every body is written where the design places it now, and each hinge's and slide's `ref` is its
current value. So MuJoCo's initial configuration (`qpos0`) is the design as it is, a loop's
constraint holds there, and each joint's value in MuJoCo equals SolidCraft's: the same angle (in
radians) or distance (in metres). A body with several joints keeps the URDF chain's motion: each
joint's `pos` and `axis` are where that step of the chain is, seen from the body.

A ball joint's value is a quaternion. It is the identity where the design is now.

## Joints

| SolidCraft joint | MJCF |
|---|---|
| Rigid | no joint: the body is nested in the one it hangs on |
| Revolute | `hinge` about z, with `range` when it has limits |
| Slider | `slide` along z, with `range` when it has limits |
| Cylindrical | `hinge` z and `slide` z |
| Pin-slot | `slide` x and `hinge` z |
| Planar | `slide` x, `slide` y and `hinge` z |
| Ball | `ball`; its limits are not written, with a warning |
| Parallel | `slide` x, y and z, and `hinge` z |
| Perpendicular, angle, point on plane, point on line, line on plane | no joint, fixed where it is, with a warning |

The axes are the joint frame's, as in the URDF ([Joints](urdf.md#joints)): when the tree reaches a
joint from its B side, its steps run backwards with their axes reversed, so the values still
match.

- **Limits of one value** (a joint held at 20°, say) are a `joint` constraint that holds the joint
  there: MuJoCo leaves a joint whose `range` is one value free.
- **Dynamics** ([Joint dynamics](urdf.md#joint-dynamics)): the damping and friction saved for a
  joint's value are its `damping` and `frictionloss`. A ball joint has one of each: those of its
  last value, with a warning if the others differ. The effort saved is the joint's motor's limit
  (below). MJCF has no velocity limit, so a saved velocity is not written.

## Actuators

Each `hinge` and `slide` that moves on its own gets a `motor` of the same name in `<actuator>`. Its
control is the torque (N m) or force (N) on the joint, and its `ctrlrange` is ± the effort saved
in [Joint dynamics](urdf.md#joint-dynamics); with none saved, the control has no limit. A control of
0 is no force, so until something sets the controls the model moves as it would without motors.
Drive a joint from Python with `data.ctrl[model.actuator("Hinge").id] = 2.0`.

There is no motor on a joint that a motion link drives, on one held at a single value, or on a ball
joint: the first two would fight the constraint that moves or holds them.

## Motion links

A gear pair, rack and pinion, screw or plain link is a `joint` constraint between the driven joint
and the first joint that drives it, as the URDF export's `<mimic>` names them: `polycoef="a0 a1 0 0
0"` with `a1` the ratio in metres and radians. MuJoCo couples how far each joint is from its `ref`,
so `a0` is 0 where the design holds the link. MuJoCo sets how stiff the constraint is without `a1`,
so where the ratio is below 1 (a rack's 0.005 m per radian) the driving joint is `joint1` and `a1`
its inverse: the other way round a rack follows a sixth of the way. A cam is left free, and so is a
link to or from a ball joint, each with a warning.

## Loops

The tree is built as the URDF export builds it ([The tree](urdf.md#the-tree)). A joint that would
close a loop becomes an `<equality>` constraint between the bodies of its two sides:

| Joint closing the loop | Constraint |
|---|---|
| Rigid | `weld` |
| Revolute | two `connect`s on its axis, 10 mm apart, which leave the one turn a hinge has |
| Ball | one `connect` at its centre |
| Any other | left out, with a warning |

## Not covered

- Sensors, velocity limits, and actuators other than a torque or force `motor` (position or velocity servos), or on ball joints.
- Decals and other pictures on the model.
- Joint motion inside the copies of a component placed several times.
- MJCF import.
