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

Next to `robot.xml` the export writes one binary STL per component, in metres, named as the URDF
export names them: `robot-<component>.stl`, the root component's bodies in `robot-base.stl`, and
`robot-<component>-collision.stl` when a component mixes closed solids with open surfaces. Every
path is checked before anything is written. The meshes are written first and the `.xml` last.

## Bodies

- **The world** holds the root component's bodies and every grounded occurrence, which hangs on
  it without a joint.
- **One body per placement of a component,** nested in the body it hangs on, named as the URDF
  link is (`Gripper`, or `Finger_1` for a component placed several times).
- **Mass and inertia** are the URDF link's: each body's mesh and material density, the mass in kg,
  the centre of mass and the inertia about it (`fullinertia`, kg m²) in the body frame.
- **Geoms:** the component's mesh. When it mixes closed solids with open surfaces, the mesh is
  drawn but does not collide (`contype="0" conaffinity="0"`), and the closed solids' mesh collides
  (`group="3"`). Open surfaces add no mass.
- **Colour:** the first body's colour, as the geom's `rgba`.

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
match. Motion links (gears, rack and pinion, screws) are not written: each joint's value is free.

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

- Motion links as `<equality>` couplings between joint values.
- Actuators, sensors, joint damping and friction.
- Simplified collision shapes: the collision mesh is the visual mesh.
- Decals and other pictures on the model.
- Joint motion inside the copies of a component placed several times.
- MJCF import.
