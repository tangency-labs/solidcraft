# URDF export

A design with components and joints exports as a robot description in URDF (Unified Robot
Description Format), the format ROS, Gazebo, Isaac Sim, PyBullet and MuJoCo read. Each part
becomes a link with its mesh, mass and inertia. Each joint becomes a URDF joint, or a short
chain of them, that moves the parts the way SolidCraft does.

- **Desktop:** **File → Export URDF…**
- **Commands:** `file.export {"path": "robot.urdf"}` (or `"format": "urdf"`)
- **MCP:** the `export` tool with a `.urdf` path or `format: "urdf"`
- **CLI:** `solidcraft-cli run script.json --out robot.urdf`

URDF always exports the whole assembly, so `bodies` is refused.

## Files

Next to `robot.urdf` the export writes one binary STL per component, in metres:

- `robot-<component>.stl` holds all of the component's bodies. The root component's bodies go
  in `robot-base.stl`.
- `robot-<component>-collision.stl` appears only when a component mixes closed solids with
  open surfaces: it holds the closed ones, which are the collision geometry.

Every path is checked before anything is written, including against the folders an agent may
use (`allow_paths`). The meshes are written first and the `.urdf` last. If a write fails part
way, the error names the files already written and the one that failed.

## Parameters

| Parameter | Default | Meaning |
|---|---|---|
| `mesh_prefix` | none | Put before each mesh file name. Leave it empty for loaders that resolve against the URDF's folder (Isaac Sim, PyBullet, MuJoCo, yourdfpy). RViz and other ROS tools need `package://<pkg>/meshes/` or an absolute folder. |
| `effort` | 1000 | Effort limit for joint values with none saved, in N (sliding) or N m (turning). |
| `velocity` | 10 | Velocity limit for joint values with none saved, in m/s or rad/s. |
| `mimic` | true | Write `<mimic>` for motion links. `false` leaves each joint free, as older versions did. |

URDF requires `effort` and `velocity` on every moving joint. A joint's own values come first, then
these parameters, then the defaults, with a warning naming the joints that used them. A value that
is not a positive number is refused.

## Joint dynamics

`joint.dynamics {joint, index?, effort?, velocity?, damping?, friction?}` saves what a simulator
reads for one joint value, in the design, in SI units: effort in N or N m, velocity in m/s or rad/s,
damping in N s/m or N m s/rad, friction in N or N m. Editing a joint shows them under **Dynamics**.
The joint solver does not use them. `damping` and `friction` are written as `<dynamics>` only where
set. In a chain, each step uses the values of the joint value it follows.

## Links

- **`base_link`** holds the root component's bodies and stays fixed in the world. Every
  grounded occurrence hangs on it by a `fixed` joint at its current place.
- **One link per placement of a component.** A component placed once gives its name to the
  link (`Gripper`). One placed several times gives the occurrence names (`Finger:1` becomes
  `Finger_1`).
- **Names** use only `A-Z a-z 0-9 _` (mesh files also keep `-`). Links, joints and files share
  one set of names, because Gazebo refuses a link and a joint with the same name. A name
  already taken gets `_2`, `_3` and so on. Joint names are taken first, so a joint keeps the
  name you gave it. Mesh file names are compared without case, since Windows and macOS ignore
  it.
- **Mass and inertia** come from each body's mesh and material density: the mass in kg, the
  centre of mass and the inertia tensor about it in the link frame, in kg m². Bodies of the
  same link combine with the parallel-axis theorem.
- **Open surfaces** are drawn but add no mass or collision, with a warning naming them.
- **A link with no mass** (only surfaces, no bodies, or a chain's in-between link) gets a token
  mass of 1e-6 kg and inertia of 1e-12 kg m², because Gazebo drops massless links and breaks
  the chain there.
- **Colour:** the first body's colour, as the visual's material.

## Joints

| SolidCraft joint | URDF |
|---|---|
| Rigid | `fixed` |
| Revolute | `revolute` about z with limits, or `continuous` without |
| Slider | `prismatic` along z |
| Cylindrical | `revolute` z, then `prismatic` z |
| Pin-slot | `prismatic` x, then `revolute` z |
| Planar | `prismatic` x, `prismatic` y, then `revolute` z |
| Ball | `revolute` z, `revolute` y, then `revolute` z |
| Parallel | `prismatic` x, y and z, then `revolute` z |
| Perpendicular, angle, point on plane, point on line, line on plane | `fixed` where it is, with a warning: their turns follow a fixed tilt or offset, which a URDF chain can't hold |

- **Chains:** a joint with several values becomes one URDF joint per value. Between them sit
  links named `<joint>_link1`, `<joint>_link2`. The steps before the last are named
  `<joint>_dof1`, `<joint>_dof2`, and the last one keeps the joint's name.
- **Values:** each URDF joint value equals the SolidCraft value it comes from: the same angle
  (in radians) or distance (in metres). This holds whichever side of the joint is nearer the
  base. When the tree reaches a joint from its B side, the chain runs backwards with its axes
  reversed, so the values still match.
- **Limits** are written as `lower <= upper` in radians or metres. A sliding step without a
  limit gets ±1000 m, since URDF requires one, with a warning.
- **Motion links** (gears, racks, screws and plain links) are written as `<mimic>` on the driven
  joint, converted to metres and radians. A chain of links is composed down to its first driver,
  since many tools accept only a mimic of a joint that is not one itself. A warning names a driven
  joint left free: a cam (not linear), a cycle, or a driver that is not in the URDF. A driver whose
  range can exceed the driven joint's limit also gets a warning.

The URDF is written at the design's current joint values, with every part where it is shown,
including moves not captured yet. The exception is a part the joints place: it exports where its
joints put it, since they put it back as soon as one moves, and a warning says so. Moving a joint in a simulator puts
the parts the joint solver places where SolidCraft's **Drive Joints** would.

## The tree

URDF is a tree hanging from `base_link`. The export builds it the way the joint solver walks
the assembly.

1. From `base_link` and the grounded occurrences, it follows the joints outwards.
2. A group of parts joined to each other but to nothing grounded is fixed where it is by the
   A side of its first joint, as the solver does when nothing is grounded. A warning says the
   part is not grounded.
3. A part no joint reaches is fixed where it is. A nested one hangs on its parent part, so it
   moves with it, as in SolidCraft. A top-level one hangs on `base_link`, with a warning.
4. A joint that would close a loop is left out, with a warning, since a tree cannot hold it.
5. A joint whose snap points no longer resolve (for example, the face it was on is gone) is
   left out, with a warning naming it.

When a component is placed several times, what is nested in it appears in each placement. In
the URDF the first placement is the one its joints move; the others are copies fixed in their
parent at their current pose, with a warning when the copied part has a joint. (In SolidCraft
the copies move together, which a URDF could only express with `<mimic>`.) The export stops copying past
64 levels or 100 000 placements, with a warning.

## Not covered

- Simplified collision shapes: the collision mesh is the visual mesh.
- SDF and MJCF export, and URDF import.
- Joint motion inside the copies of a component placed several times.
