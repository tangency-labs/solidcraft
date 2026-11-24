Make a coil: a compression spring, an extension spring body, a helical wire form, or a square-section helix. You set the diameter, the number of turns, the pitch and the wire size.

#### Before you start

- You don't need a sketch or a body. A coil can start a new design, standing on the origin and rising along Z.
- Section position decides what Diameter measures. Inside puts the wire inside the diameter, so Diameter is the outside diameter (OD). Outside puts the wire outside it, so Diameter is the inside diameter (ID). Center puts the diameter through the middle of the wire, the mean diameter.
- The coil's length is Revolutions × Pitch.
- The coil is right-handed unless you turn on Clockwise.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Profile | A closed sketch profile to use as the section instead of a circle or square. With a profile picked, Diameter and Section size are hidden; the sketch's place sets them. | A special wire shape. Leave it empty for round or square wire. |
| Diameter | The coil diameter. What it measures depends on Section position. It starts at 40 mm. | 0.750 in OD, with Section position on Inside. |
| Revolutions | How many turns. It starts at 4. | 8 coils. |
| Pitch | How far the coil rises in one turn, center to center of the wire. It starts at 8 mm. | 0.250 in. |
| Section size | The wire size: its diameter for Circular, its width for Square. It starts at 3 mm. | 0.062 in wire. |
| Section | Circular for round wire, Square for square wire. | Circular for most springs. |
| Section position | Inside, Center or Outside: where the wire sits on the Diameter. It starts at Center. | Inside when the print gives the OD; Outside when it gives the ID. |
| Start angle | Where the first turn starts around the axis. 0 deg starts toward +X. | Turn it to line the wire end up with a feature. |
| Clockwise | Turn the coil the other way, making it left-handed. Off by default. | Only when the print calls for a left-hand spring. |
| Operation | It starts at New Body. New Body makes a separate body. Join adds it to a body it touches. Cut takes it out. Intersect keeps only the material inside both. | New Body for a spring. |

#### Try it: model a compression spring

1. In a new design, click Coil.
2. Set Section position to Inside and Diameter to 0.75 in, so 0.75 in is the outside diameter.
3. Set Section size to 0.062 in, Revolutions to 8 and Pitch to 0.25 in. Leave Section on Circular.
4. Click OK. The spring is 0.750 in OD and 2.0 in long (8 × 0.25 in), right-handed.

#### Tips and common mistakes

- **Do** set Section position to match the print: Inside for an OD, Outside for an ID, Center for a mean diameter. Leaving it on Center with an OD makes the spring one wire size too big.
- **Do** remember Pitch is per turn. For a free length and coil count, Pitch = free length ÷ coils.
- **Don't** expect closed or ground ends. The coil has the same pitch from end to end, so it models the spring's body, not a finished spring's ends.
- **Do** use {{#command solid.thread}} for a screw thread. It uses the standard thread forms and sizes.
- **Do** type the unit with the number when the print uses another unit. In a millimetre design, typing 0.062 in enters 1.5748 mm.

#### See also

{{#command solid.thread}} cuts standard threads. {{#command solid.sweep}} runs any section along a path.
