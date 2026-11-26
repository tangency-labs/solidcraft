Cut a spline into a round shaft or a bore, or make a new splined shaft and the hub that fits it. The spline is one feature in the History: you can reopen it and change the tooth count, the sizes or the fit later.

#### Before you start

- For a new part, pick nothing. The shaft or hub stands on the origin along the Z axis, from 0 up to Length.
- To cut into a shaft you already have, pick the outside of its round face. To cut the teeth of a hub, pick the wall of its bore. Pick both to cut a shaft and a hub that fit each other.
- A round face has two ends, so each one you pick needs a second pick: the face the teeth start from. That is the part's flat end, a shoulder or a chamfer.
- Length is measured along the picked round face from the end the start face is at. It must not be longer than the face.
- Only the outside of a round shaft or the wall of a round bore can be picked.

#### Options

The numbers that size the teeth are always at the top of the dialog. Everything else is in groups that open and close when you click their titles: Tooth form, Fit, Tip corners, Chamfers, Groove end, Master spline, Hub and As made. Tooth form and Fit start open; Tip corners, Chamfers, Groove end and Master spline start closed.

| Choice in the dialog | Meaning | Example or reason to choose it |
| --- | --- | --- |
| Shaft, Bore (optional) | The outside of a round shaft to cut the spline into, and a round hole to cut the hub's teeth into. With nothing picked, a new part is made. | Pick the shaft end you turned on a lathe, or the hub's bore. |
| Shaft start, Bore start (with that part picked) | The face the teeth start from: the part's flat end, a shoulder, a step, a chamfer or a round. The teeth start at the end of the round face nearest it and run along the round face for the Length. OK waits until it is picked. | The shoulder the hub is pushed against, or the end of the shaft. |
| Fixed diameter (with a shaft picked) | On: the spline keeps the shaft's own diameter and works its tooth size out from it. Off: the size you type stands and the shaft follows it, so more teeth of one size make a larger shaft. | On for a shaft you have already sized. Off to design the spline first. |
| Type | Involute, Straight-sided or Serration. | Involute for curved flanks; Straight-sided for teeth with two parallel flanks; Serration for teeth whose flanks meet at the Flank angle. |
| Make (new part only) | Shaft, Hub, or Shaft and hub. | Shaft and hub to get a matching pair in one step. |
| Teeth | How many teeth. Involute takes 3 to 2000; the other types take 2 to 2000. | 16 teeth. |
| Length | How far the teeth run along the shaft or bore. | 30 mm. |
| Module, Pressure angle (Involute) | Tooth size is the module, the pitch diameter divided by the tooth count. The pressure angle is the slope of the flanks at the pitch circle. | Module 2 with the starting 30 degree pressure angle. |
| Major diameter, Minor diameter, Tooth width or Flank angle (Straight-sided, Serration) | The diameters over the tips and at the bottom of the spaces, the width of a straight-sided tooth, and the angle between a serration's flanks. | Set the two diameters so the teeth leave a root and a tip; the drawing says if they do not. |
| Tooth form | Involute: Addendum, Dedendum, Root fillet, Profile shift, all in modules, and Thinning, a length taken off each tooth on the pitch circle. Straight-sided and Serration: Root round, the radius where the flanks meet the minor circle. | Leave the starting values for standard teeth. |
| Fit | The clearances between the shaft and the hub. Involute: Side clearance, Hub addendum, Tip clearance, Hub root round, Form clearance. Other types: Side clearance, Major clearance, Minor clearance, Hub root round, Hub tip round. | More side clearance for a hub that slides, less for a snug one. |
| Tip corners (Straight-sided, Serration) | Plain, Chamfered or Rounded corners on the tips, and their Size. | Chamfered tips go into the hub more easily. |
| Chamfers: End chamfer (shaft) | A chamfer on the ends of the shaft's teeth. End chamfer is how far it runs along the shaft; Type says how far it runs in from the major diameter: Equal Distance, the same, at 45 degrees; Two Distances, Distance 2; Distance and Angle, the Angle to the shaft's surface. 0 for none. | 1 mm at 45 degrees, so the shaft starts into the hub. Distance and Angle when the drawing gives one, like 1 mm × 30°. |
| Chamfers: Entry chamfer (hub) | A chamfer where the teeth run out of the bore. Entry chamfer is how far it runs into the bore; its Type, Distance 2 or Angle say how far it runs out from the tops of the hub's teeth, as for the End chamfer. 0 for none. | 1 mm at 45 degrees, to guide the shaft in. |
| Groove end: Round the ends (shaft picked) | Each tooth space ends in the curve a cutter of the Cutter diameter leaves, which runs on past the Length until it leaves the shaft's surface. Off: the spaces end square. Teeth that run the whole length run right through. | Match the cutter that will cut the spaces, for a spline that stops short of the end. |
| Master spline | Tick Cut a master spline, then choose under On the shaft: Tooth left out, or Wide tooth (one space filled), so the hub goes on one way only. Angle puts it, counter-clockwise from angle 0, as the drawing shows it: 0 is right, 90 is top. | A shaft that must be fitted in one position. |
| Hub: Outside diameter (new hub) | The outside of the hub's body. It must leave a rim around the spaces. | 40 mm. |
| As made | Read-outs of the result: Positions the hub fits in, Turning play, Play on each side, Gap at the tips, Gap at the roots, and for involute teeth Gap at the fillet. | Check the play before you accept. |

The dialog draws the teeth as you change them. **Show** switches the drawing between the Whole spline, a close-up of the Teeth, and, when a master spline is on, the Master spline. When the numbers make no spline, the line under the drawing says why and the row to change is marked.

#### Try it: a splined shaft and its hub

1. Click Shaft Spline in the Create menu of Model, and leave both picks empty.
2. Set Type to Involute and Make to Shaft and hub.
3. Set Teeth to 16, Length to 30 mm and Outside diameter, under Hub, to 40 mm.
4. Look at the drawing and the As made rows. Positions the hub fits in shows how many ways the hub goes on.
5. Open Master spline, tick Cut a master spline, choose Tooth left out under On the shaft, and watch the drawing under Show > Master spline.
6. Open Chamfers and set End chamfer and Entry chamfer to 1 mm. To make the shaft's chamfer 30 degrees, set its Type to Distance and Angle and its Angle to 30.
7. Click OK. One Shaft Spline feature appears in the History, with a splined shaft and a hub.

#### Tips and common mistakes

- **Do** pick the start face after the round face. Length counts from the end of the round face that the start face is at.
- **Don't** look for a way to start the teeth partway along a plain round face. They start at one of its ends; turn a step or a groove where they must begin.
- **Do** tick Fixed diameter when the shaft already has the size it must keep. Otherwise the shaft is turned down or built up to the spline's size.
- **Do** use the Shaft Spline chamfers rather than a Chamfer on the shaft first: they follow the size and tooth count when you change them, and take the same three types as the Chamfer tool. A part you chamfered or rounded with the Chamfer or Fillet tool first is still cut through, and the teeth run out through it.
- **Do** reopen the feature in the History to change the teeth. Nothing else needs to be redone.
- **Don't** expect a Make choice when you pick a face. What is cut follows the picks.
- **Don't** set a Length longer than the picked face. The spline is refused.
- **Don't** type a tooth count with decimals. It must be a whole number.

#### See also

{{#command solid.thread}} puts a helical thread on a round face. {{#command solid.hole}} drills the bore a hub is cut in. {{#command solid.chamfer}} breaks an edge by hand. {{#command solid.pattern.circular}} repeats one feature around an axis.
