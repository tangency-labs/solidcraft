Turn a sketch profile around an axis to make a round part: a shaft, a pin, a pulley or a bushing. You draw the half-section the way it is drawn on a lathe print, and Revolve spins it.

#### Before you start

- Draw a closed profile of one side of the part: the half-section, from the centre line out.
- You need an axis to spin about: a sketch line, an origin axis, a construction axis, a straight edge, or a cylinder face.
- The profile must stay on one side of the axis. It can touch the axis but not cross it.
- A flat face of the part can also be used as the profile.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Profile | The closed area to spin. | The half-section of a stepped shaft. |
| Axis | The line it spins around. | The sketch line along the centre of the shaft, or the X axis. |
| Type | Angle spins by the angle you type. To Object spins until it reaches a face or point you pick in the To Object box, which replaces Direction and Angle. | Angle for most parts. |
| Direction | One Side spins one way. Two Sides spins a different angle each way. Symmetric splits the angle evenly both ways from the sketch. Shown for Type: Angle. | Symmetric for a 90° segment centred on the sketch. |
| Angle | How far to spin. | 360 deg for a full round part. |
| Angle 2 | The angle on the other side, for Two Sides. | 30 deg one way, 60 deg the other. |
| Operation | New Body, Join, Cut or Intersect. Cut removes material from an existing body. | Cut to turn a groove into an existing shaft. |

#### Try it: a stepped pin

1. Start a sketch on the XY plane. If the document is in mm, type in after each value (for example 0.25in). Draw this closed outline from the origin along the X axis: up 0.25 in, over 1 in, up 0.125 in, over 0.5 in, down 0.375 in back to the X axis, and close it along the axis. Click Finish Sketch.
2. Click Revolve. Click the profile.
3. Click Axis, then click the sketch line along the X axis. Check the Axis box: if it reads Y, you picked the wrong axis and the preview comes out flat. Pick again.
4. Leave Type on Angle, Direction on One Side, and Angle at 360 deg. Set Operation to New Body so the pin is its own part.
5. Click OK. You have a pin 0.5 in diameter by 1 in long with a 0.75 in diameter head 0.5 in long.
6. For a groove, start a new sketch on the XY plane (it runs through the pin's centre line). Draw a rectangle that cuts into the shaft, with corners at (0.4 in, 0.20 in) and (0.5 in, 0.30 in), and click Finish Sketch. Revolve it with Operation set to Cut. For the Axis, click the sketch line along the X axis, or click the round side of the pin (not the flat end); its centre line is used. The result is a groove 0.1 in wide and 0.05 in deep.

#### Several revolves from one sketch

Right-click a sketch and choose **Revolve**: the window opens with the sketch shown, even when
an earlier feature uses it. **Apply** makes the revolve and keeps the window and the sketch for
the next one, as for [Extrude](solid.extrude.md).

#### Tips and common mistakes

- **Do** draw the centre line as part of the profile, or as a sketch line in the same sketch. Then picking the axis is one click and it moves with the sketch.
- **Do** use Direction: Symmetric for a partial revolve that should sit centred on the sketch plane.
- **Don't** let the profile cross the axis. Keep the whole half-section on one side.
- **Don't** draw the full section of the part. Draw one half only; Revolve makes the other half.

#### See also

{{#command solid.extrude}} pushes a profile straight instead of spinning it. {{#command solid.thread}} adds a thread to the revolved shaft. {{#command solid.chamfer}} breaks its edges.
