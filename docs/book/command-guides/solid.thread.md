Put an ISO metric thread on a round face: an external thread on a shaft or stud, or an internal thread in a bore. You pick the size and pitch, whether the thread is cut into the part, and how far it runs.

#### Before you start

- You need a body with a cylindrical face: the outside of a shaft for an external thread, or the wall of a bore for an internal one.
- Size the cylinder first. For an external thread, make the shaft the thread's major diameter (12 mm for M12). For an internal thread, make the bore the tap drill size.
- For an ordinary tapped hole, use {{#command solid.hole}} with Hole Tap Type set to Tapped instead. It drills the tap drill and keeps the thread callout on the hole.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Cylindrical face | The round face the thread goes on. | Click the outside of the shaft. |
| Profile | The thread standard. ISO metric. | M-series threads, such as M12. |
| Modeled | On: the thread groove is cut into the part. Off: the thread is carried as a callout only. | Leave it off unless you need to see the thread or 3D print it. A modeled thread makes the part slower to rebuild. |
| Full Length | On: the thread runs the whole face. Off: a Length box appears. | Turn it off for a threaded end on a longer shaft. |
| Automatic size | Picks the standard size that fits the face you selected. | Turn it on first. With it off, the dialog starts at M20, which won't fit a smaller shaft. |
| Size | The thread's nominal diameter from the ISO table. | 12 mm for M12. |
| Pitch | The thread pitch. The standard coarse pitch is picked for the size; fine pitches are in the list. | 1.75 mm for M12 coarse, 1.25 mm for M12 fine. |
| Designation | The thread callout, built from Size and Pitch. You can type it directly. | M12, or M12x1.25 for fine. |
| Thread direction | Right-hand or left-hand thread. | Right-hand unless the print calls for LH. |
| Length (Full Length off) | How far the thread runs from the end of the face. | 20 mm for a threaded end. |

#### Try it: a threaded shaft end

1. Sketch a 12 mm circle on the XY plane and extrude it 50 mm.
2. Click Thread, then click the round face of the shaft.
3. Turn on Automatic size. Size changes to 12 mm, Pitch to 1.75 mm and Designation to M12.
4. Turn off Full Length and set Length to 20.
5. Turn on Modeled to see the thread cut into the shaft.
6. Click OK. The end of the shaft carries a 20 mm long M12 thread.

#### Tips and common mistakes

- **Do** turn on Automatic size first. If the dialog says the thread won't fit, the size doesn't match the shaft: the dialog starts at M20.
- **Do** size the cylinder before threading it: major diameter for an external thread, tap drill size for an internal one.
- **Do** leave room for runout before a shoulder. A thread can't run full depth right up to a shoulder without a relief groove.
- **Don't** turn on Modeled just to show the part is threaded. The callout already says it.
- **Don't** expect inch threads here. Profile is ISO metric only; for a UNC or UNF tapped hole, use {{#command solid.hole}}.

#### See also

{{#command solid.hole}} makes tapped holes with the right tap drill, including UNC and UNF. {{#command solid.chamfer}} adds a lead-in chamfer to the start of a thread. {{#command solid.revolve}} makes a turned part to thread.
