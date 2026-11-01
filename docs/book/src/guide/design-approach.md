# How a design is built

SolidCraft is a parametric, history-based modeller. You don't shape geometry directly: you
describe the steps that make it, and SolidCraft computes the result from those steps. Change a
step or a value and everything after it is computed again. This page explains that idea and the
usual ways of working with it. The pages after it cover each tool in detail; the
[glossary](../reference/glossary.md) defines most of the terms used here.

## The idea in one paragraph

A **design** holds a **timeline**: an ordered list of **features**. Most parts start with a
**sketch**, a 2D drawing on a plane, held in shape by **constraints** and **dimensions**. A
feature such as Extrude or Revolve turns the sketch's closed regions into a solid **body**; later
features add to it, cut into it or finish it (holes, fillets, shells). Every dimension and feature
input is a **parameter**, so a size can be changed at any time, and the timeline rebuilds from the
first feature that uses it.

```text
sketch (plane + constraints + dimensions)
   └─ feature (extrude, revolve, …)  →  body
         └─ more features (holes, fillets, patterns, shell, …)
               └─ change a parameter  →  the timeline rebuilds
```

## Principles that make a design easy to change

- **Capture intent, not just shape.** A hole "centred on the face" or "10 mm from that edge" stays
  right when the part grows; a hole placed by eye doesn't. Use constraints and dimensions for what
  matters, and name the important values as parameters (type `width = 40` in any value box: see
  [Parameters and expressions](parameters.md)).
- **Fully constrain sketches.** Under-constrained sketch geometry is drawn blue and the Sketch
  Palette shows how many degrees of freedom are left; once none are, the geometry changes colour
  (white in the default dark theme) and the status reads **Fully constrained** (see
  [Sketching and constraints](sketching.md)). A fully constrained sketch changes only the way you
  tell it to.
- **Keep sketches simple, let features do the work.** A rectangle and an extrude, then a fillet, is
  easier to change than one sketch that draws the rounded outline. Holes, patterns, mirrors,
  fillets and chamfers are features of their own for the same reason.
- **Build in a sensible order.** The base shape first, then the features that add and cut, then
  the finishing features (fillets, chamfers, shell, draft). Features depend on the ones before
  them, so an early change ripples forward.
- **Sketch on stable references.** An origin plane (XY, XZ, YZ, or Top, Front, Right) or a
  construction plane changes less than a face that a later edit may remove.

## The timeline is the design

The [timeline](ui-tour.md), the **History** strip along the bottom of the window, lists every
feature in the order it is computed. Each feature can be edited (double-click), suppressed,
renamed, reordered when nothing it depends on moves after it, and deleted; deleting asks first
when other features depend on it.

Drag the **rollback marker** back to see the design as it was at an earlier step. Features after
the marker are kept, not deleted, and a new feature is added at the marker. This is how you add
something early in the design (a sketch the later features should use) without rebuilding the rest.

The timeline is not the undo history: Undo and Redo move back and forth through what you did; the
timeline is what the design is made of.

## Bodies and components

A **body** is one connected solid or surface shape. A design can hold many bodies, and features
can make a new body, join or cut existing ones, or intersect them.

A **component** groups bodies, sketches and construction geometry, with its own origin, and can be
placed more than once; each placement is an **occurrence**. Editing a component changes every
occurrence of it; moving an occurrence moves only that copy. The **active component** (the radio
button in the [browser](ui-tour.md)) receives new sketches and features.

Two ways of working follow from this:

- **One part, several bodies.** For a single part, or a few parts that are designed against each
  other, model bodies in one design. Bodies are quick to make and to combine, split or cut with each
  other.
- **Components and joints.** When parts are separate pieces (they move, repeat, or go on a bill of
  materials), make them components: **New Component**, or **Create Components from Bodies** to turn
  bodies you already modelled into components. Then connect them with **joints** (rigid, revolute,
  slider, cylindrical, pin-slot, planar, ball) and drive the motion. See
  [Components and joints](assemblies.md).

Starting with bodies and turning them into components later is a common path: it keeps the early
design quick and makes the assembly when its structure is clear.

## Common workflows

| You want to | Start with | Read |
|---|---|---|
| Model a part from scratch | **New Sketch** (Create Sketch) on an origin plane, constrain and dimension it, Extrude or Revolve | [Sketching](sketching.md), [Solid features](features.md) |
| Make a design you can resize | User parameters, added with **Parameters** (Change Parameters), used in sketch dimensions and feature inputs | [Parameters and expressions](parameters.md) |
| Build an assembly | Bodies turned into components, or New Component, then joints | [Components and joints](assemblies.md) |
| Design a bent sheet part | A base flange from a sketch, then flanges, hems and folds under a sheet metal rule | [Sheet metal](sheet-metal.md) |
| Design a plastic enclosure | A shelled body, then bosses, lip and groove, snap fits, ribs | [Plastic parts](plastic.md) |
| Work on a part from another program | Open a STEP, IGES, 3MF, STL or OBJ file as a new design | [Import and export](files.md) |
| Script a design or hand it to an agent | Commands in a script for `solidcraft-cli`, or the MCP server | [Getting started](getting-started.md#cli), [MCP](mcp.md) |

Everything you click is a **command** with named parameters, and the toolbar, scripts, the CLI and
AI agents all run the same commands. A design built in the window can be turned into a script with
`solidcraft-cli script`, which rebuilds it without the window (designs with components or joints
are not supported there yet).

## Terms if you come from another CAD program

Other programs use other words for the same ideas. SolidCraft uses one term for each:

| SolidCraft | You may know it as |
|---|---|
| timeline (shown as the History strip) | history, feature tree |
| rollback marker | rollback bar, end-of-part marker |
| browser | model tree, feature tree, design tree |
| body | solid body, surface body |
| component | part, sub-assembly |
| occurrence | instance, component instance |
| design | document, model, part file, assembly file |

The full list, with what each term must not be confused with, is in the
[glossary](../reference/glossary.md).

## Finding help

- **Help (?) › Documentation** in the title bar opens this manual.
- **Help (?) › Keyboard Shortcuts** lists the shortcuts; see also
  [Keyboard shortcuts](shortcuts.md).
- The [command reference](../reference/commands.md) describes every command and its parameters.
- **Report a bug**, in the title bar, drafts an issue report with the details the maintainers
  need, for you to review and file on GitHub; the app doesn't send anything itself.
- Questions and ideas: the [Discord](https://discord.gg/FtU4NQNVDH) and the
  [issue tracker](https://github.com/tangency-labs/solidcraft/issues).
