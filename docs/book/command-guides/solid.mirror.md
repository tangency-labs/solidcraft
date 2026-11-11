Copy bodies or features to the other side of a plane: model half of a symmetric part and mirror it, or make the left-hand version of a right-hand bracket.

#### Before you start

- Make the half (or the feature) you want to copy first.
- Choose what to copy with Object Type. Bodies copies whole solid bodies. Features repeats a step from the History, such as a Hole or a cut Extrude, on the other side of the same body.
- A sketch is neither. To mirror sketch curves, use the sketch's own Mirror, or extrude the sketch first.
- You need a plane to mirror about: an origin plane (XY, XZ or YZ), a construction plane, or a flat face of the part.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Object Type | Bodies copies whole bodies. Features repeats History steps. | Features to put a second pocket on the other side of the centre line. Bodies to mirror half a part. |
| Objects or Features | What to mirror. The box is named Objects or Features to match Object Type. With Features, pick the steps in the History; you can pick more than one. | The half body, or two Hole steps at once. |
| Mirror Plane | The plane the copy is reflected across. | The YZ plane when the part is centred on the origin. |
| Operation (Bodies only) | New Body keeps the copy as its own body. Join merges it with the original into one body. | Join when you modelled half a part and want one solid. New Body for a left-hand and right-hand pair. |

#### Try it: model half, mirror the rest

1. Sketch a 2 in × 1.5 in rectangle on the XY plane with one 1.5 in short edge on the Y axis, so the 2 in side runs along X, and Extrude it 0.5 in. This is the right half of a 4 in wide block.
2. Click Mirror. Leave Object Type on Bodies and click the block.
3. Click Mirror Plane, then click the YZ plane.
4. Set Operation to Join. Click OK. The block is now 4 in wide and one body.
5. Add a hole on one side, then Mirror again with Object Type set to Features and the Hole step picked. The hole appears at the same spot on the other side.

#### Tips and common mistakes

- **Do** draw symmetric parts centred on an origin plane. Then that plane is the mirror plane, and the halves line up.
- **Do** use Join when the halves touch and should be one part. With New Body you get two bodies sitting face to face.
- **Do** mirror the feature, not the body, when you just need the same hole or pocket on the other side. Change the original and the mirrored one follows.
- **Don't** expect a mirrored copy to be the same part. A mirrored right-hand bracket is a left-hand bracket, and a mirrored right-hand thread is left-hand.

#### See also

{{#command sketch.mirror}} mirrors curves inside a sketch. {{#command solid.pattern.rectangular}} and {{#command solid.pattern.circular}} repeat features more than once.
