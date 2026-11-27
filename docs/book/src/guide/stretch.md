# Stretch

**Stretch** makes a finished solid longer, wider or taller without going back to the sketches
that built it. It cuts the body across and fills the gap with the body's own shape, so holes,
rounds and walls keep their size. It works on imported solids too. Find it on the Model ribbon,
in the Modify panel's **Bodies** group, beside Scale.

## How it works

1. Click **Stretch** and pick one body.
2. Give a distance along one, two or all three axes (**X distance**, **Y distance**, **Z
   distance**). Leave a distance empty to leave that axis alone. A negative distance shrinks the
   body.
3. For each axis, choose the way the moving side goes (**+X** or **-X**, and so on). The opposite
   side stays where it is.
4. Drag the arrows on the model, or type the values. The preview shows the result.
5. Click **OK**, or press **Esc** to leave without a change.

The body is cut by a plane square to each axis. The part beyond the plane moves by the distance,
and the gap fills with the body's section at the plane. A shrink takes a slice out at the plane
instead.

With **Edit sketch** ticked (the default), Stretch changes the sketch instead and adds no step;
see [Edit sketch](#edit-sketch) below. When Edit sketch is off, or the sketch cannot be changed,
each axis you stretch along becomes part of one **Stretch** step on the timeline. Double-click
that step to change it. Its plain distances become parameters (`stretch1`, `stretch2` and so on)
when you click OK. Change a parameter in the Parameters window and the body follows.

## The cut plane

The see-through square on the model shows where the body is cut. A second arrow moves it.

- **Cut at** empty: the cut goes to the middle of the longest plain part of the body along that
  axis. A plain part is where every face runs straight along the axis, with no steps, slopes or
  holes across it.
- **Cut at** with a value: the cut is that far from the side that stays put.
- **Cut from faces**: pick a flat face, and **Cut at** is measured from that face. Picking the
  face also sets its axis's direction: the side the face looks at is the side that moves. A
  negative **Cut at** puts the cut behind the face, inside the body. The step keeps the face, so
  the cut follows it when an earlier change moves the face.

Put the cut where the body has the same shape all along the axis, away from holes, slopes and
rounds that cross it.

## Edit sketch

With **Edit sketch** ticked (the default for a new stretch), Stretch changes the sketch and
Extrude that made the body instead of adding a Stretch step:

- A stretch along the sketch's plane adds the distance to each of the sketch's dimensions that
  spans the cut. A 100 mm width becomes 115 mm. When the dimension is set by a parameter, the
  parameter changes instead.
- A stretch along the Extrude's direction adds the distance to the Extrude's depth.
- Holes, bosses and cuts drawn beyond the cut in this body's other sketches move with the part,
  and so do faces that later features of this body picked there. Other bodies stay where they are.

The sketch then shows the new sizes, and the part rebuilds from it. Stretch checks that the
rebuilt part is the stretched one. If it is not, for example when the sketch's other side moved
too, Stretch puts the sketch back and adds a Stretch step instead, and says why.

Edit sketch works best when the sketch is held on the side that stays put: a corner on the
origin, or a fixed line. A Stretch step is added instead when the body was not made by a
one-sided Extrude of a sketch (an imported part or a box), when no dimension spans the cut, or
when a dimension is a formula.

## When Stretch refuses

Stretch tells you why it refuses, and changes nothing:

- **The cut crosses a detail.** A sloped face, a round, a step, or a hole that runs across the
  axis lies at the cut. The message names the face. Move the cut to a plain part of the body.
- **A shrink goes into a detail.** The slice a shrink takes out must be plain all the way
  through. The message says how far the body can shrink there.
- **A shrink is too large.** It would take out everything in front of the cut.
- **The cut misses the body.** The cut is outside the body along that axis.
- **The body is a mesh.** Convert an imported mesh (STL, 3MF) to a solid first.
- **Two stretches along one axis.** One Stretch step takes each axis once.
