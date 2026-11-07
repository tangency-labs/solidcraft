Put a hole in a solid body: a simple hole, a counterbore, a countersink or a tapped hole. You set the location, size, depth and drill point, the same things a print calls out.

#### Before you start

- You need a solid body with a flat face to drill into. The hole goes straight into the face.
- Locate the hole from edges of that face, the same way a print dimensions a hole from two edges. Pick an edge as Reference 1 and type its distance, then do the same for Reference 2.
- To drill several holes at once, sketch a point for each one on the face first. Every point gets a hole.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Placement | Single drills one hole on the face you pick. | Use Single for one hole located from edges. |
| Face | The face the hole is drilled into. | Click the top face of the part. |
| Reference 1 and Distance 1 | An edge of the face, and how far the hole center is from it. | The left edge, 0.75 in. |
| Reference 2 and Distance 2 | A second edge, and how far the hole center is from it. | The front edge, 0.75 in. |
| Extents | How the depth is set. Distance drills to the Depth you type. | Use Distance for a blind hole. |
| Hole Type | Simple is a straight hole. Counterbore and Countersink add a bore or a cone at the top. | Counterbore for a socket head cap screw. Countersink for a flat head screw: 82° for inch screws, 90° for metric. |
| Hole Tap Type | Simple is a plain drilled hole with a Diameter you type. Tapped makes a threaded hole and shows the thread settings. | Choose Tapped for any tapped hole. |
| Profile (Tapped) | The thread standard. | UNC for coarse inch threads, UNF for fine inch threads. |
| Size (Tapped) | The thread size, from a list. The Designation box shows the callout, and Tap drill diameter shows the drill the app uses. | 1/4-20 UNC uses a 0.201 in (#7) tap drill. The app shows it as 5.1054 mm. |
| Full thread depth (Tapped) | On: the thread runs the full depth of the hole. Off: a Thread depth box appears. | Turn it off for a blind tapped hole. |
| Thread depth (Tapped) | How deep the full thread goes, separate from the drill Depth. | Drill 0.625 in deep with 0.50 in of thread, so a standard tap doesn't bottom out. |
| Thread direction (Tapped) | Right-hand or left-hand thread. | Right-hand unless the print calls for LH. |
| Drill Point | The bottom of the hole. Angle leaves a drill point. | Use Angle for a hole made with a twist drill. |
| Depth | How deep the hole goes. | Measured to the end of the full diameter, not the tip of the drill point. |
| Tip Angle | The angle of the drill point. Default is 118°. | 118° for a standard jobber drill. 135° for most carbide and split-point drills. |
| Diameter (Simple) | The drill size for a plain hole. | Type the drill size, such as 0.201 in. |

#### Try it: a 1/4-20 tapped hole 0.75 in from two edges

1. Click New Sketch and choose the XY plane. Draw a 2-point rectangle from the origin, 3 in × 2 in. Click Finish Sketch.
2. Click Extrude and set the distance to 0.75 in. Click OK.
3. Click Hole, then click the top face.
4. Click Reference 1, pick the left edge of the top face and set Distance 1 to 0.75 in. Click Reference 2, pick the front edge and set Distance 2 to 0.75 in.
5. Set Hole Tap Type to Tapped, Profile to UNC and Size to 1/4-20 UNC. Check that Tap drill diameter shows 5.1054 mm (0.201 in).
6. Turn off Full thread depth. Set Depth to 0.625 in and Thread depth to 0.50 in. Click OK.
#### Tips and common mistakes

- **Do** locate holes from edges the way the print dimensions them. When the part changes size, the holes keep their distances from those edges.
- **Do** use Tapped and pick the size from the list instead of typing a drill diameter. The hole then carries the thread callout, and the tap drill comes from the tap maker's table.
- **Do** check the Tap drill diameter before clicking OK. It is shown in millimetres: 5.1054 mm is 0.201 in.
- **Do** turn off Full thread depth on blind tapped holes and set a Thread depth shorter than the drill Depth, so a standard tap doesn't bottom out.
- **Do** type the unit with the number when the print is in another unit. In a millimetre design, typing 0.75 in enters 19.05 mm.
- **Don't** expect every hole to work on sheet metal. Sheet metal holes must go straight into a flat panel; holes into the edge of the sheet, countersinks and drill points are refused there.

#### See also

{{#command solid.thread}} cuts a thread on any round face, including the outside of a shaft. {{#command solid.pattern.circular}} repeats a hole around a center for a bolt circle. {{#command solid.chamfer}} breaks the edge of a hole.
