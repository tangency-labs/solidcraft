Put a thread on a round face: an external thread on a shaft or stud, or an internal thread in a bore. You pick the thread form and size, how far the thread runs, and how its ends are finished.

#### Before you start

- You need a body with a cylindrical face: the outside of a shaft for an external thread, or the wall of a bore for an internal one.
- Size the cylinder first. For an external thread, make the shaft the thread's major diameter (12 mm for M12). For an internal thread, make the bore the tap drill size.
- Click the face near the end you want threaded. On a shaft with two free ends, that tells a limited-length thread where to start.
- For an ordinary tapped hole, including UNC and UNF, use {{#command solid.hole}} with Hole Tap Type set to Tapped instead. It drills the tap drill and keeps the thread callout on the hole.

#### Options

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Cylindrical face | The round face the thread goes on. | Click the outside of the shaft near the end to thread. |
| Form | The thread standard: ISO metric, Trapezoidal (ISO 2904), Acme (ASME B1.5), Buttress (DIN 513 or ASME B1.9), and the pipe threads Pipe G (ISO 228-1), Pipe Rp (ISO 7-1), Pipe taper R, Rc (ISO 7-1) and Pipe taper NPT (ASME B1.20.1). UNC and UNF are not in this list. | ISO metric for fasteners. Acme or Trapezoidal for lead screws. Buttress for a screw that takes load one way, like a vise screw. Pipe taper NPT for US tapered pipe threads. |
| Modeled | On: the thread groove is cut into the part. Off: the thread is carried as a callout only. | Leave it off unless you need to see the thread or 3D print it. A modeled thread makes the part slower to rebuild. |
| Automatic size | On by default: picks the standard size that fits the face you selected. | Leave it on when the shaft is a standard diameter. |
| Size and Pitch | The thread size from the standard's table, and for metric threads the pitch. | M12 with the standard 1.75 mm pitch, or 1.25 mm for M12 fine. |
| Designation | The thread callout. You can type it directly. | M12, M12x1.25. |
| Hand | Right, Left, or Both. Pipe threads offer Right or Left only. | Right unless the print calls for LH. |
| Starts | Number of thread starts, 1 to 8. | 1 for fasteners. 2 or more for a fast-moving lead screw: each turn advances the pitch times the number of starts. |
| Steep Flank (Buttress only) | Which end the steep, load-carrying flank faces. | Point it toward the load. |
| Extent | Full runs the whole face. Length runs a set distance. Up to stops at a face or plane you pick. | Length for a threaded end on a longer shaft. Up to for a thread that stops at a shoulder. |
| Start end (optional) | Which end of the face the thread is measured from. Shown once Extent is Length or Up to. | Pick it when the thread starts at the wrong end. |
| Start (Ends) | None leaves the thread's free end plain. Chamfer adds a lead-in chamfer and shows Chamfer Angle and Chamfer Depth; left blank, they use the ISO 4753 standard chamfer. | Chamfer the start of any thread that has to take a nut or go into a tapped hole by hand. |
| End (Ends) | Square stops the thread sharp. Fade runs it out over a length (ISO 3508 runout, 2.5 pitches standard). | Fade for a thread cut with a die or single-point tool that stops short of a shoulder. |

#### Try it: a threaded shaft end

1. Sketch a 12 mm circle on the XY plane and extrude it 50 mm.
2. Click Thread, then click the round face of the shaft near its top end.
3. Check that Form is ISO metric and Automatic size picked M12 with a 1.75 mm pitch.
4. Set Extent to Length and the length to 20 mm.
5. Turn on Modeled to see the thread cut into the shaft.
6. Under Ends, set Start to Chamfer. Leave Chamfer Angle and Chamfer Depth blank for the standard ISO 4753 chamfer.
7. Click OK. The top 20 mm of the shaft carries an M12 thread with a lead-in chamfer.

#### Tips and common mistakes

- **Do** size the cylinder before threading it: major diameter for an external thread, tap drill size for an internal one.
- **Do** click the face near the end you want threaded, or pick Start end. Otherwise a Length thread can start at the wrong end.
- **Do** chamfer the start of an external thread. The shop will put one there anyway, and a nut starts on it in the model the same way it does on the part.
- **Do** leave room for runout before a shoulder. A thread can't run full depth right up to a shoulder without a relief groove.
- **Don't** turn on Modeled just to show the part is threaded. The callout already says it.
- **Don't** look for UNC or UNF here. Inch fastener threads are only on {{#command solid.hole}} tapped holes for now; the inch forms in this list are Acme, ASME buttress and the pipe threads.
- **Don't** mix up NPT and the ISO taper (R, Rc, often called BSPT). Both are tapered pipe threads, but the thread angle and pitch differ, and they don't seal together.

#### See also

{{#command solid.hole}} makes tapped holes with the right tap drill, including UNC and UNF. {{#command solid.chamfer}} breaks an edge by hand. {{#command solid.revolve}} makes a turned part to thread.
