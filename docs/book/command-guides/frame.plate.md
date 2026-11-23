# Mounting plates and bolted end plates

`frame.plate` creates a separate plate body on a planar support face. Use it for a
base plate beneath a post or a bolted end plate on a structural member. It does not
join the plate to the member or drill the member.

In **Weldments > Create > Mounting Plate**, select the flat end face, including the
thin ring around a hollow tube's opening. Choose a rectangular or round outline,
set its dimensions and thickness, then choose no holes, four holes on a rectangular
pitch, or holes on a bolt circle. The dialog previews the result. Edit Feature
restores the face and the plate's values.

The plate is centred on the current supporting face's area centroid, rather than
at the clicked point. Its thickness extends outwards along that face's normal, so
a plate on the underside of a post extends downwards. **Offset** moves it along
that normal; a negative value moves it into the support. **X Offset** and **Y Offset**
move it in the face's canonical axes before **Angle** rotates the plate. Bolt
patterns turn with the plate; a bolt circle also has its own pattern angle.

For example, on the top of a 40 mm square tube centred at X = 200, Y = 0, ending
at Z = 300:

```json
{
  "command": "frame.plate",
  "params": {
    "face": [219, 0, 300],
    "width": "100 mm",
    "height": "80 mm",
    "thickness": "8 mm",
    "holes": {
      "pattern": "rectangle",
      "diameter": "10 mm",
      "pitch_x": "70 mm",
      "pitch_y": "50 mm"
    },
    "body_name": "End plate"
  }
}
```

For an exact pick, also supply `body` and `index`, and `path` when selecting a
particular occurrence. The face index is only used to capture the initial face;
the feature saves its persistent identity. Lengths accept design parameters and
unit expressions. Bare length numbers use the design's length unit; bare angles
are degrees. World face coordinates are always millimetres.

A round plate uses `shape: "round"` and `diameter`, omitting `width` and `height`.
A bolt circle uses `holes: {"pattern": "circle", "diameter": "10 mm",
"pitch_diameter": "90 mm", "count": 6, "angle": "15 deg"}`. Circle counts must be
whole numbers from 2 to 128. All holes must remain separate and strictly inside
the plate perimeter; touching holes and edge breakouts are refused.

The plate follows supported changes to its face and its placement. A lost or
ambiguous face produces an error; the plate never silently attaches to a nearby
body. This first tool creates constant-thickness flat plates. Standard flange
size tables, arbitrary sketch outlines, welds and strength calculations are not
part of it.
