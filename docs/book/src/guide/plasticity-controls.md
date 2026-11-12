# Plasticity controls

SolidCraft can work with the keys and mouse of Plasticity, so you can start modelling the
way you're used to. A new installation starts with it; switch in **Preferences › Navigation ›
Control scheme** (*Plasticity* or *SolidCraft*). Your own key changes (Preferences › Keyboard
Shortcuts) apply on top of either scheme.

Inside a sketch the scheme steps aside and SolidCraft's sketch keys work as usual (L line,
R rectangle, C circle, D dimension, T trim, O offset…): Plasticity draws curves without a
sketch mode, so its keys have no sketch meaning.

## Mouse

| Input | Action |
|---|---|
| Middle drag | Orbit, around the point under the cursor |
| Shift + middle drag | Pan |
| Ctrl + middle drag, wheel | Zoom (at the cursor) |
| Right drag / Shift + right drag | Orbit / pan |
| Left click / drag | Select / box select (right to left: crossing) |

## Modelling

| Key | Command |
|---|---|
| E | Extrude (faces push/pull, profiles extrude) |
| O | Push face (Offset Face). Plasticity's O offsets a loose curve; SolidCraft's curves live in a sketch, where O already offsets them |
| B | Fillet · Alt+B chamfer |
| Q | Boolean (Combine) |
| C | Cut (Split Body) |
| L | Loft · Shift+P sweep |
| Shift+X / Shift+Y | Box / Cylinder (Plasticity gives its primitives no key; Shift+B and Shift+C are its Bridge Curve and Center Circle) |
| G, R | Move / rotate (the move triad does both) |
| Ctrl (held, while dragging the triad) | Snap the body onto the place under the cursor: a corner, the middle of an edge, a centre, a quarter turn. Alt drags freely instead |
| S | Scale |
| Alt+X | Mirror |
| Shift+D | Duplicate, then move the copy |
| M | Appearance (material) |
| W, Ctrl+= | Measure |
| X, Delete, Backspace | Delete |
| F | Search commands |
| Shift+A, Shift+C | In a sketch: line, circle from the centre (Plasticity's own curve keys, beside SolidCraft's L and C) |
| Shift+R | Repeat last command |
| Ctrl+Z / Ctrl+Shift+Z | Undo / redo |

## Selection, visibility and view

| Key | Action |
|---|---|
| 1 2 3 4 5 | Select control points / edges / faces / solids / everything |
| Shift+1…4 | Toggle that kind |
| A / Alt+A | Select all / invert selection |
| H, Shift+H, Alt+H, Ctrl+H | Hide selected, hide unselected, unhide all, invert hidden |
| . | Isolate the selection; again comes back to what was hidden before it, not to everything |
| / | Focus: frame the selection, or everything |
| Space | Navigate to the selection (with something selected; otherwise hold to pan) |
| Shift+Space | Construction plane from selection (starts a sketch) |
| Alt+1, Alt+3, Alt+7 | Front, right, top view (Ctrl+Alt: back, left, bottom) |
| Alt+0 / Alt+5 | Isometric view / toggle orthographic |
| Alt+Z / Shift+Z | X-ray / wireframe |

The views are on Alt+digit because the toolkit can't tell the numeric keypad's digits from the
top row, which the selection modes use. The list is also under Preferences › Keyboard
Shortcuts (the *Plasticity scheme* section).

## A pick is already the command

Clicking a face or an edge with no command running opens the one that shapes it, standing
ready: Press Pull for a face, Fillet for an edge. It changes nothing until you use it -- the
size starts at zero and the model stays as it was -- and it keeps out of the way while it
waits, so further clicks go on picking and the letter keys stay the shortcuts they are. Esc
gets out of it, and picking the same thing again does not reopen it.

Or type the size: a digit, a point or a minus goes straight into the command's number, the first one taking the keyboard and the rest carrying on from it. Click an edge and type 7.5 and the round is 7.5 mm. Letters are still shortcuts, so E is still Extrude.

The handle on what you picked drags straight into the command. Which way you drag an edge
says what it becomes: the way the blend grows rounds it off, the other way takes the corner
off square. A face pushes out or pulls in.

With the SolidCraft scheme a pick is a pick. That scheme has its own face gesture instead:
press on a planar face and pull, and Extrude starts on it.

## Drawing a primitive

With this scheme a Box, Cylinder or Sphere is drawn in the view rather than dropped at the
origin: click out its base on the face under the cursor or on the ground, pull it to its
height, and click again to confirm. The places the model offers are what the clicks lock onto,
and each size gets a draggable arrow while the shape is being adjusted. Nothing is built until
the shape has been said, and the card's numbers can be typed at any point -- a size typed in is
not drawn over.

A shape pulled into the solid it stands on cuts it; pulled away it stands on its own. The keys
say otherwise: **Q** join, **W** cut, **Shift+E** intersect, **B** new body, **T** keep the
shape as a body too. **Backspace** takes the last point back and **Esc** drops the shape. The
strip under the view names what to do now and every key the shape answers to.

With the SolidCraft scheme the cards place a primitive their own way instead: the X, Y and Z of
its corner, centre or base, typed or picked in the view, and a Cylinder dimensioned from two
reference edges of the face it stands on.

## Opening Plasticity files

File › Open (or Insert, or dropping the file on the window) reads `.plasticity` files. Each
solid becomes a mesh body named as in Plasticity, in one import feature; items hidden in
Plasticity start hidden. A Plasticity file keeps its exact geometry in a format SolidCraft
can't read, so the bodies come from the display mesh Plasticity saves with it, refined back to
round holes, fillets and other curved faces. Curves and measurements aren't read (the import
says how many were left out). To edit a prism-like part as solid faces, use Convert Mesh.
