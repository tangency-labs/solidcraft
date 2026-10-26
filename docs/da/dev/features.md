# Features (funktionsoversigt)

SolidCrafts kommandoer opdelt efter område, genereret af `cargo xtask features` fra kommandoregisteret (`solidcraft-cli commands`). Planlagte kommandoer er markeret i tabellerne. Kommandoerne virker ens fra værktøjslinjen, scripts, `solidcraft-cli exec <id> '<json>'`, kontrolkanalen og MCP. Parametrene findes i [kommandoreferencen](book/src/reference/commands.md) (`cargo xtask book`). Status og planer: [ROADMAP](../ROADMAP.md).

## Skitsering

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Center Point Arc | `sketch.arc.center_point` | SKETCH › CREATE | Bue med centrum |
| Tangent Arc | `sketch.arc.tangent` | SKETCH › CREATE | Tangentbue |
| 3-Point Arc | `sketch.arc.three_point` | SKETCH › CREATE | Trepunktsbue |
| AutoConstrain | `sketch.auto_constrain` | SKETCH › CONSTRAINTS | Automatisk binding |
| AutoConstrain from datum | `sketch.auto_constrain_from_datum` | SKETCH › CONSTRAINTS | Automatisk binding fra reference |
| Auto-Project Edges on Reference | `sketch.auto_project` | – | Projicér referencekanter automatisk |
| Blend Curve | `sketch.blend_curve` | SKETCH › MODIFY | Overgangskurve |
| Break | `sketch.break` | SKETCH › MODIFY | Break |
| Break Link | `sketch.break_link` | – | Bryd tilknytning |
| Centerline | `sketch.centerline` | SKETCH › CREATE | Centerlinje |
| Chain | `sketch.chain` | – | Kæde |
| Distance and Angle Chamfer | `sketch.chamfer.distance_angle` | SKETCH › MODIFY | Affasning med afstand og vinkel |
| Equal Distance Chamfer | `sketch.chamfer.equal_distance` | SKETCH › MODIFY | Affasning med ens afstande |
| Two Distance Chamfer | `sketch.chamfer.two_distance` | SKETCH › MODIFY | Affasning med Til afstande |
| Center Diameter Circle (C) | `sketch.circle.center` | SKETCH › CREATE | Cirkel fra centrum og Diameter (C) |
| 3-Point Circle | `sketch.circle.three_point` | SKETCH › CREATE | Trepunktscirkel |
| 3-Tangent Circle | `sketch.circle.three_tangent` | SKETCH › CREATE | Cirkel Tangent til tre |
| 2-Point Circle | `sketch.circle.two_point` | SKETCH › CREATE | Topunktscirkel |
| 2-Tangent Circle | `sketch.circle.two_tangent` | SKETCH › CREATE | Cirkel Tangent til Til |
| Conic Curve | `sketch.conic` | SKETCH › CREATE | Keglesnitskurve |
| Sketch Constrainer | `sketch.constrainer` | SKETCH › CREATE | Skitsebindinger |
| Coincident | `sketch.constraint.coincident` | SKETCH › CONSTRAINTS | Sammenfaldende |
| Collinear | `sketch.constraint.collinear` | SKETCH › CONSTRAINTS | På samme linje |
| Concentric | `sketch.constraint.concentric` | SKETCH › CONSTRAINTS | Koncentrisk |
| Curvature | `sketch.constraint.curvature` | SKETCH › CONSTRAINTS | Krumning |
| Equal | `sketch.constraint.equal` | SKETCH › CONSTRAINTS | Ens |
| Fix/UnFix | `sketch.constraint.fix` | SKETCH › CONSTRAINTS | Fastlås/UnFix |
| Horizontal/Vertical | `sketch.constraint.horizontal_vertical` | SKETCH › CONSTRAINTS | Vandret/Lodret |
| MidPoint | `sketch.constraint.midpoint` | SKETCH › CONSTRAINTS | Midtpunkt |
| Parallel | `sketch.constraint.parallel` | SKETCH › CONSTRAINTS | Parallel |
| Perpendicular | `sketch.constraint.perpendicular` | SKETCH › CONSTRAINTS | Vinkelret |
| Polygon | `sketch.constraint.polygon` | SKETCH › CONSTRAINTS | Polygon |
| Symmetry | `sketch.constraint.symmetry` | SKETCH › CONSTRAINTS | Symmetri |
| Tangent | `sketch.constraint.tangent` | SKETCH › CONSTRAINTS | Tangent |
| Normal/Construction (X) | `sketch.construction` | SKETCH › CREATE | Normal/Construction (X) |
| Copy Sketch Entities | `sketch.copy` | – | Kopiér Skitse Entities |
| Create Sketch | `sketch.create` | SOLID › CREATE | Opret Skitse |
| Cut Sketch Entities | `sketch.cut` | – | Cut Skitse Entities |
| Delete Sketch Entities | `sketch.delete` | – | Slet Skitse Entities |
| Sketch Dimension (D) | `sketch.dimension` | SKETCH › CREATE | Skitse Dimension (D) |
| Move Dimension Text | `sketch.dimension_text` | – | Flyt Dimension Text |
| Drag Sketch Geometry | `sketch.drag` | – | Drag Skitse Geometry |
| End Sketch Drag | `sketch.drag_end` | – | End Skitse Drag |
| Draw Dimensioned Sketch Geometry | `sketch.draw.dimensioned` | – | Draw Dimensioned Skitse Geometry |
| Edit Sketch | `sketch.edit` | – | Rediger Skitse |
| Edit Text | `sketch.edit_text` | – | Rediger Text |
| Ellipse | `sketch.ellipse` | SKETCH › CREATE | Ellipse |
| Save As DXF | `sketch.export_dxf` | – | Gem As DXF |
| Extend | `sketch.extend` | SKETCH › MODIFY | Forlæng |
| Fillet | `sketch.fillet` | SKETCH › MODIFY | Afrunding |
| Finish Sketch | `sketch.finish` | SKETCH › FINISH SKETCH | Finish Skitse |
| Finish with AutoConstrain | `sketch.finish_auto_constrain` | SKETCH › FINISH SKETCH | Finish Med Automatisk binding |
| Fit Curves to Mesh Section | `sketch.fit_curves_to_section` | SKETCH › CREATE | Fit Curves Til Mesh Snit |
| Constraint Glyphs | `sketch.glyphs` | – | Binding Glyphs |
| Include 3D Geometry | `sketch.include_3d` | SKETCH › CREATE | Include 3D Geometry |
| Infinite Line | `sketch.infinite` | – | Infinite Linje |
| Insert DXF | `sketch.insert_dxf` | SKETCH › INSERT | Indsæt DXF |
| Insert SVG | `sketch.insert_svg` | SKETCH › INSERT | Indsæt SVG |
| Inspect Sketch | `sketch.inspect` | – | Undersøg Skitse |
| Intersect | `sketch.intersect` | SKETCH › CREATE | Intersect |
| Intersection Curve | `sketch.intersection_curve` | SKETCH › CREATE | Intersection Curve |
| Isoparametric Curve | `sketch.isoparametric_curve` | SKETCH › CREATE | Isoparametric Curve |
| Line (L) | `sketch.line` | SKETCH › CREATE | Linje (L) |
| Midpoint Line | `sketch.line.midpoint` | SKETCH › CREATE | Midtpunkt Linje |
| 3D Line | `sketch.line3d` | – | 3D Linje |
| Loose Ends | `sketch.loose_ends` | – | Loose Ends |
| Tie Loose Ends | `sketch.loose_ends.tie` | – | Tie Loose Ends |
| Mirror | `sketch.mirror` | SKETCH › PATTERN | Spejl |
| Move/Copy Sketch Entities | `sketch.move` | SKETCH › MODIFY | Flyt/Kopiér Skitse Entities |
| Move 3D Point | `sketch.move3d` | – | Flyt 3D Punkt |
| Drag Sketch Point | `sketch.move_point` | – | Drag Skitse Punkt |
| Offset (O) | `sketch.offset` | SKETCH › MODIFY | Forskyd (O) |
| Break Offset Association | `sketch.offset.break` | – | Break Forskyd Association |
| Sketch Palette Options | `sketch.options` | – | Skitse Palette Options |
| Paste Sketch Entities | `sketch.paste` | – | Paste Skitse Entities |
| Circular Pattern | `sketch.pattern.circular` | SKETCH › PATTERN | Cirkulært Mønster |
| Rectangular Pattern | `sketch.pattern.rectangular` | SKETCH › PATTERN | Rektangulært Mønster |
| Point | `sketch.point` | SKETCH › CREATE | Punkt |
| 3D Point | `sketch.point3d` | – | 3D Punkt |
| Circumscribed Polygon | `sketch.polygon.circumscribed` | SKETCH › CREATE | Circumscribed Polygon |
| Edge Polygon | `sketch.polygon.edge` | SKETCH › CREATE | Edge Polygon |
| Inscribed Polygon | `sketch.polygon.inscribed` | SKETCH › CREATE | Inscribed Polygon |
| Project (P) | `sketch.project` | SKETCH › CREATE | Projicér (P) |
| Project To Surface | `sketch.project_to_surface` | SKETCH › CREATE | Projicér Til Overflade |
| Center Rectangle | `sketch.rectangle.center` | SKETCH › CREATE | Center Rektangel |
| Rounded Rectangle | `sketch.rectangle.rounded` | SKETCH › CREATE | Rounded Rektangel |
| 3-Point Rectangle | `sketch.rectangle.three_point` | SKETCH › CREATE | 3-Punkt Rektangel |
| 2-Point Rectangle (R) | `sketch.rectangle.two_point` | SKETCH › CREATE | 2-Punkt Rektangel (R) |
| Redefine Sketch Plane | `sketch.redefine` | – | Redefine Skitse Plan |
| Sketch Scale | `sketch.scale` | SKETCH › MODIFY | Skitse Skalér |
| Center Point Arc Slot | `sketch.slot.arc_center` | SKETCH › CREATE | Bue med centrum Slot |
| Three Point Arc Slot | `sketch.slot.arc_three_point` | SKETCH › CREATE | Three Punkt Bue Slot |
| Center Point Slot | `sketch.slot.center_point` | SKETCH › CREATE | Center Punkt Slot |
| Center to Center Slot | `sketch.slot.center_to_center` | SKETCH › CREATE | Center Til Center Slot |
| Overall Slot | `sketch.slot.overall` | SKETCH › CREATE | Overall Slot |
| Snap Inference | `sketch.snap` | – | Snap Inference |
| Solve Sketch | `sketch.solve` | – | Solve Skitse |
| Control Point Spline | `sketch.spline.control_point` | SKETCH › CREATE | Control Punkt Spline |
| Control Point Spline (Degree 5) | `sketch.spline.control_point_5` | SKETCH › CREATE | Control Punkt Spline (Degree 5) |
| Fit Point Spline | `sketch.spline.fit_point` | SKETCH › CREATE | Fit Punkt Spline |
| 3D Spline | `sketch.spline3d` | – | 3D Spline |
| Spun Profile | `sketch.spun_profile` | SKETCH › CREATE | Spun Profile |
| Text | `sketch.text` | SKETCH › CREATE | Text |
| Toggle Driven Dimension | `sketch.toggle_driven` | – | Toggle Driven Dimension |
| Trim (T) | `sketch.trim` | SKETCH › MODIFY | Beskær (T) |

## Solidmodellering

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Align | `solid.align` | SOLID › MODIFY | Justér |
| Boundary Fill | `solid.boundary_fill` | SOLID › CREATE | Afgrænsning Udfyld |
| Bounding Solid | `solid.bounding_solid` | SOLID › CREATE | Afgrænsende Solid |
| Box | `solid.box` | SOLID › CREATE | Kasse |
| Chamfer | `solid.chamfer` | SOLID › MODIFY | Affasning |
| Coil | `solid.coil` | SOLID › CREATE | Spiral |
| Combine | `solid.combine` | SOLID › MODIFY | Combine |
| Cylinder | `solid.cylinder` | SOLID › CREATE | Cylinder |
| Draft | `solid.draft` | SOLID › MODIFY | Formslip |
| Emboss | `solid.emboss` | SOLID › CREATE | Emboss |
| Extrude (E) | `solid.extrude` | SOLID › CREATE | Ekstrudér (E) |
| Fillet (F) | `solid.fillet` | SOLID › MODIFY | Afrunding (F) |
| Hole (H) | `solid.hole` | SOLID › CREATE | Hul (H) |
| Loft | `solid.loft` | SOLID › CREATE | Overgang mellem profiler |
| Mirror | `solid.mirror` | SOLID › CREATE | Spejl |
| Move/Copy (M) | `solid.move` | SOLID › MODIFY | Flyt/Kopiér (M) |
| Offset Face | `solid.offset_face` | SOLID › MODIFY | Forskyd Flade |
| Circular Pattern | `solid.pattern.circular` | SOLID › CREATE | Cirkulært Mønster |
| Pattern on Path | `solid.pattern.path` | SOLID › CREATE | Mønster På Sti |
| Rectangular Pattern | `solid.pattern.rectangular` | SOLID › CREATE | Rektangulært Mønster |
| Pipe | `solid.pipe` | SOLID › CREATE | Rør |
| Press Pull (Q) | `solid.press_pull` | SOLID › MODIFY | Træk/skub (Q) |
| Remove | `solid.remove` | SOLID › MODIFY | Fjern |
| Replace Face | `solid.replace_face` | SOLID › MODIFY | Replace Flade |
| Revolve | `solid.revolve` | SOLID › CREATE | Omdrej |
| Rib | `solid.rib` | SOLID › CREATE | Forstærkningsribbe |
| Scale | `solid.scale` | SOLID › MODIFY | Skalér |
| Shell | `solid.shell` | SOLID › MODIFY | Udhul |
| Sphere | `solid.sphere` | SOLID › CREATE | Kugle |
| Split Body | `solid.split_body` | SOLID › MODIFY | Opdel Emne |
| Split Face | `solid.split_face` | SOLID › MODIFY | Opdel Flade |
| Sweep | `solid.sweep` | SOLID › CREATE | Før langs sti |
| Thread | `solid.thread` | SOLID › CREATE | Gevind |
| Torus | `solid.torus` | SOLID › CREATE | Torus |
| Web | `solid.web` | SOLID › CREATE | Forstærkningsvæg |

## Overflader

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Blend Surface | `surface.blend` | SURFACE › CREATE | Blend Overflade |
| Boundary Surface | `surface.boundary` | SURFACE › CREATE | Afgrænsning Overflade |
| Extend | `surface.extend` | SURFACE › MODIFY | Forlæng |
| Surface Extrude | `surface.extrude` | SURFACE › CREATE | Overflade Ekstrudér |
| Fair Surface | `surface.fair` | SURFACE › MODIFY | Fair Overflade |
| Surface Fillet *(planned: [#769](https://github.com/bherbruck/solidcraft/issues/769))* | `surface.fillet` | SURFACE › MODIFY | Overflade Afrunding *(planned: [#769](https://github.com/bherbruck/solidcraft/issues/769))* |
| Surface Loft | `surface.loft` | SURFACE › CREATE | Overflade Overgang mellem profiler |
| Match Surface | `surface.match` | SURFACE › MODIFY | Match Overflade |
| Offset Surface | `surface.offset` | SURFACE › CREATE | Forskyd Overflade |
| Patch | `surface.patch` | SURFACE › CREATE | Patch |
| Reverse Normal | `surface.reverse_normal` | SURFACE › MODIFY | Vend Normal |
| Surface Revolve | `surface.revolve` | SURFACE › CREATE | Overflade Omdrej |
| Ruled Surface | `surface.ruled` | SURFACE › CREATE | Ruled Overflade |
| Stitch | `surface.stitch` | SURFACE › MODIFY | Sammensy |
| Surface Sweep | `surface.sweep` | SURFACE › CREATE | Overflade Før langs sti |
| Thicken | `surface.thicken` | SURFACE › CREATE | Fortyk |
| Trim | `surface.trim` | SURFACE › MODIFY | Beskær |
| Unstitch | `surface.unstitch` | SURFACE › MODIFY | Ophæv sammensyning |
| Untrim | `surface.untrim` | SURFACE › MODIFY | Fjern beskæring |

## Konstruktionsgeometri

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Axis Through Edge | `construct.axis.edge` | SOLID › CONSTRUCT | Akse Gennem Edge |
| Axis Perpendicular To Face | `construct.axis.normal_to_face` | SOLID › CONSTRUCT | Akse Vinkelret Til Flade |
| Axis Through Cylinder/Cone/Torus | `construct.axis.through_cylinder` | SOLID › CONSTRUCT | Akse Gennem Cylinder/Cone/Torus |
| Axis Through Two Planes | `construct.axis.two_planes` | SOLID › CONSTRUCT | Akse Gennem Two Planes |
| Axis Through Two Points | `construct.axis.two_points` | SOLID › CONSTRUCT | Akse Gennem Two Points |
| Plane Along Path | `construct.plane.along_path` | SOLID › CONSTRUCT | Plan Langs Sti |
| Plane at Angle | `construct.plane.at_angle` | SOLID › CONSTRUCT | Plan Ved Vinkel |
| Midplane | `construct.plane.midplane` | SOLID › CONSTRUCT | Midplane |
| Offset Plane | `construct.plane.offset` | SOLID › CONSTRUCT | Forskyd Plan |
| Perpendicular Plane | `construct.plane.perpendicular` | SOLID › CONSTRUCT | Vinkelret Plan |
| Tangent Plane | `construct.plane.tangent` | SOLID › CONSTRUCT | Tangent Plan |
| Plane Through Three Points | `construct.plane.three_points` | SOLID › CONSTRUCT | Plan Gennem Three Points |
| Plane Through Two Edges | `construct.plane.two_edges` | SOLID › CONSTRUCT | Plan Gennem Two Edges |
| Point Along Path | `construct.point.along_path` | SOLID › CONSTRUCT | Punkt Langs Sti |
| Point At Center Of Circle/Sphere/Torus | `construct.point.center` | SOLID › CONSTRUCT | Punkt Ved Center Of Cirkel/Kugle/Torus |
| Point At Edge And Plane | `construct.point.edge_plane` | SOLID › CONSTRUCT | Punkt Ved Edge And Plan |
| Point Through Three Planes | `construct.point.three_planes` | SOLID › CONSTRUCT | Punkt Gennem Three Planes |
| Point Through Two Edges | `construct.point.two_edges` | SOLID › CONSTRUCT | Punkt Gennem Two Edges |
| Point At Vertex | `construct.point.vertex` | SOLID › CONSTRUCT | Punkt Ved Vertex |

## Pladekonstruktion

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Convert to Sheet Metal | `sheet.convert` | SHEET METAL › CREATE | Konvertér Til Pladekonstruktion |
| Export Bend Table | `sheet.export_bend_table` | – | Eksportér Buk Table |
| Export Flat Pattern as DXF | `sheet.export_dxf` | – | Eksportér Udfoldning as DXF |
| Flange | `sheet.flange` | SHEET METAL › CREATE | Flange |
| Create Flat Pattern | `sheet.flat_pattern` | SHEET METAL › CREATE | Opret Udfoldning |
| Fold | `sheet.fold` | SHEET METAL › CREATE | Buk |
| Hem | `sheet.hem` | SHEET METAL › CREATE | Ombuk |
| Sheet Metal Rules | `sheet.manage_rules` | SHEET METAL › MODIFY | Pladekonstruktion Rules |
| Perforate | `sheet.perforate` | SHEET METAL › CREATE | Perforate |
| Refold | `sheet.refold` | SHEET METAL › MODIFY | Genbuk |
| List Sheet Metal Rules | `sheet.rules` | – | Vis Pladekonstruktion Rules |
| Unfold | `sheet.unfold` | SHEET METAL › MODIFY | Udfold |

## Plastemner

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Assign Plastic Rule | `plastic.assign_rule` | PLASTIC › SETUP | Tildel Plastic Rule |
| Boss | `plastic.boss` | PLASTIC › CREATE | Skruefæste |
| Lip | `plastic.lip` | PLASTIC › CREATE | Kant |
| Manage Plastic Rules | `plastic.manage_rules` | PLASTIC › SETUP | Administrér Plastic Rules |
| Rest | `plastic.rest` | PLASTIC › CREATE | Støtteflade |
| List Plastic Rules | `plastic.rules` | – | Vis Plastic Rules |
| Snap Fit | `plastic.snap_fit` | PLASTIC › CREATE | Klikforbindelse |

## Komponenter

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Activate Component | `component.activate` | – | Activate Komponent |
| Break Link | `component.break_link` | – | Bryd tilknytning |
| Capture Position | `component.capture_position` | SOLID › POSITION | Optag Position |
| New Component | `component.create` | SOLID › ASSEMBLE | Ny Komponent |
| Delete Component | `component.delete` | – | Slet Komponent |
| Create Components from Bodies | `component.from_bodies` | – | Opret Komponenter Fra Emner |
| Insert Design | `component.insert_design` | SOLID › ASSEMBLE | Indsæt Design |
| List Links | `component.links` | – | Vis Links |
| List Components | `component.list` | – | Vis Komponenter |
| Part Properties | `component.metadata` | – | Del Egenskaber |
| Change Part Properties | `component.metadata.set` | – | Ændr Del Egenskaber |
| Move Bodies to Component | `component.move_bodies` | – | Flyt Emner Til Komponent |
| Move Sketches to Component | `component.move_sketches` | – | Flyt Sketches Til Komponent |
| Paste New | `component.paste_new` | – | Paste Ny |
| Component Physical Properties | `component.properties` | – | Komponent Fysisk Egenskaber |
| Relink Source | `component.relink` | – | Relink Source |
| Rename Component | `component.rename` | – | Omdøb Komponent |
| Revert Position | `component.revert_position` | SOLID › POSITION | Revert Position |
| Unlock Contents | `component.unlock` | – | Unlock Contents |
| Update Links | `component.update_links` | – | Update Links |

## Ledforbindelser i samlinger

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| As-Built Joint | `joint.as_built` | SOLID › ASSEMBLE | As-Built Ledforbindelse |
| Cam | `joint.cam` | SOLID › ASSEMBLE | Knast |
| Joint (J) | `joint.create` | SOLID › ASSEMBLE | Ledforbindelse (J) |
| Delete Joint | `joint.delete` | – | Slet Ledforbindelse |
| Drive Joints | `joint.drive` | SOLID › ASSEMBLE | Bevæg Ledforbindelser |
| Edit Joint | `joint.edit` | – | Rediger Ledforbindelse |
| Gear | `joint.gear` | SOLID › ASSEMBLE | Tandhjul |
| Joint Limits | `joint.limits` | – | Ledforbindelse Limits |
| List Joints | `joint.list` | – | Vis Ledforbindelser |
| Motion Link | `joint.motion_link` | SOLID › ASSEMBLE | Bevægelse Link |
| Joint Origin | `joint.origin` | SOLID › CREATE | Ledforbindelse Origin |
| Rack and Pinion | `joint.rack_pinion` | SOLID › ASSEMBLE | Tandstang and Tandhjul |
| Reach Point | `joint.reach` | – | Reach Punkt |
| Rigid Group | `joint.rigid_group` | SOLID › ASSEMBLE | Rigid Gruppe |
| Screw | `joint.screw` | SOLID › ASSEMBLE | Skrue |
| Solve Joints | `joint.solve` | – | Solve Ledforbindelser |
| Sweep Joint | `joint.sweep` | – | Før langs sti Ledforbindelse |

## Konfigurationer

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Activate Configuration | `config.activate` | – | Activate Konfiguration |
| Configuration Column | `config.column` | – | Konfiguration Column |
| Configure | `config.configure` | SOLID › CONFIGURE | Configure |
| Preview Configuration | `config.preview` | – | Preview Konfiguration |
| Configuration Row | `config.row` | – | Konfiguration Row |
| Configuration Rule | `config.rule` | – | Konfiguration Rule |
| Configuration Rules | `config.rules` | – | Konfiguration Rules |
| Display Configuration Table | `config.table` | SOLID › CONFIGURE | Display Konfiguration Table |

## Tidslinje

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Add Recorded Feature | `timeline.add` | – | Add Recorded Feature |
| Compute All | `timeline.compute_all` | SOLID › MODIFY | Compute All |
| Copy Features | `timeline.copy` | – | Kopiér Features |
| Delete (Delete) | `timeline.delete` | SOLID › MODIFY | Slet (Slet) |
| Feature Dependents | `timeline.dependents` | – | Feature Dependents |
| Edit Feature | `timeline.edit` | – | Rediger Feature |
| Export Command Script | `timeline.export_script` | – | Eksportér Command Script |
| New Feature Folder | `timeline.group` | – | Ny Feature Folder |
| Inspect Feature | `timeline.inspect` | – | Undersøg Feature |
| Move Timeline Items | `timeline.move_many` | – | Flyt Tidslinje Items |
| Move Features to Folder | `timeline.move_to_group` | – | Flyt Features Til Folder |
| Paste Features | `timeline.paste` | – | Paste Features |
| Edit Feature | `timeline.redefine` | – | Rediger Feature |
| Rename | `timeline.rename` | – | Omdøb |
| Rename Feature Folder | `timeline.rename_group` | – | Omdøb Feature Folder |
| Reorder Feature | `timeline.reorder` | – | Reorder Feature |
| Reorder Features | `timeline.reorder_many` | – | Reorder Features |
| Roll History Marker | `timeline.roll_to` | – | Roll History Marker |
| Move Timeline Marker | `timeline.rollback` | – | Flyt Tidslinje Marker |
| Suppress Feature | `timeline.suppress` | – | Suppress Feature |
| Remove Feature Folders | `timeline.ungroup` | – | Fjern Feature Folders |

## Undersøg

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Accessibility Analysis | `inspect.accessibility` | SKETCH › INSPECT | Accessibility Analyse |
| Center of Mass | `inspect.center_of_mass` | SKETCH › INSPECT | Center of Masse |
| Clearance | `inspect.clearance` | – | Clearance |
| Compare Bodies | `inspect.compare` | – | Sammenlign Emner |
| Continuity Analysis | `inspect.continuity` | SURFACE › INSPECT | Continuity Analyse |
| Curvature Comb Analysis | `inspect.curvature_comb` | SKETCH › INSPECT | Krumning Comb Analyse |
| Curvature Map Analysis | `inspect.curvature_map` | SKETCH › INSPECT | Krumning Map Analyse |
| Draft Analysis | `inspect.draft` | SKETCH › INSPECT | Formslip Analyse |
| Environment Map Analysis | `inspect.environment_map` | SKETCH › INSPECT | Environment Map Analyse |
| Interference | `inspect.interference` | SOLID › INSPECT | Overlap |
| Isocurve Analysis | `inspect.isocurve` | SKETCH › INSPECT | Isocurve Analyse |
| Measure (I) | `inspect.measure` | SOLID › INSPECT | Mål (I) |
| Minimum Radius Analysis | `inspect.minimum_radius` | SKETCH › INSPECT | Minimum Radius Analyse |
| Section Analysis | `inspect.section` | SOLID › INSPECT | Snit Analyse |
| Silhouette | `inspect.silhouette` | – | Silhouette |
| Zebra Analysis | `inspect.zebra` | SKETCH › INSPECT | Zebra Analyse |

## Filer

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Export | `file.export` | – | Eksportér |
| Export Current View as DXF | `file.export_view_dxf` | – | Eksportér Current Visning as DXF |
| Insert Mesh | `file.insert_mesh` | SOLID › INSERT | Indsæt Mesh |
| Insert STEP | `file.insert_step` | SOLID › INSERT | Indsæt STEP |
| New Design (Ctrl+N) | `file.new` | – | Ny Design (Ctrl+N) |
| Save (Ctrl+S) | `file.save` | – | Gem (Ctrl+S) |
| Save As | `file.save_as` | – | Gem As |
| Save As Mesh | `file.save_mesh` | MESH › MODIFY | Gem As Mesh |
| Installed Slicers | `file.slicers` | – | Installed Slicers |

## Dokumenter

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Open (Ctrl+O) | `doc.open` | – | Åbn (Ctrl+O) |
| Recover Design | `doc.recover` | – | Recover Design |
| Discard Recovered Design | `doc.recovery_discard` | – | Discard Recovered Design |
| Recoverable Designs | `doc.recovery_list` | – | Recoverable Designs |

## Redigering

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Checkpoint | `edit.checkpoint` | – | Checkpoint |
| Redo (Ctrl+Y) | `edit.redo` | – | Gentag (Ctrl+Y) |
| Restore Checkpoint | `edit.restore_checkpoint` | – | Restore Checkpoint |
| Undo (Ctrl+Z) | `edit.undo` | – | Fortryd (Ctrl+Z) |

## `appearance.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Appearance (A) | `appearance.assign` | SOLID › MODIFY | Udseende (A) |
| Appearance Library | `appearance.library` | – | Udseende Bibliotek |
| List Appearances | `appearance.list` | – | Vis Appearances |

## `assembly.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Bill of Materials | `assembly.bom` | SOLID › INSPECT | Stykliste |

## `body.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Rename Body | `body.rename` | – | Omdøb Emne |

## `browser.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| New Group | `browser.group` | – | Ny Gruppe |
| Move Browser Items | `browser.move` | – | Flyt Browser Items |
| Order Browser Items | `browser.order` | – | Order Browser Items |
| Rename Group | `browser.rename_group` | – | Omdøb Gruppe |
| Ungroup | `browser.ungroup` | – | Ungroup |
| Show/Hide | `browser.visibility` | – | Vis/Skjul |

## `canvas.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Calibrate Canvas | `canvas.calibrate` | – | Kalibrér Billedflade |
| Decal | `canvas.decal` | SKETCH › INSERT | Dekoration |
| Delete Canvas | `canvas.delete` | – | Slet Billedflade |
| Edit Canvas | `canvas.edit` | SKETCH › INSERT | Rediger Billedflade |
| Canvas | `canvas.insert` | SKETCH › INSERT | Billedflade |

## `contact.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Check Contact | `contact.check` | – | Kontrollér Kontakt |
| New Contact Set | `contact.create` | SOLID › ASSEMBLE | Ny Kontakt Indstil |
| Disable Contact | `contact.disable_all` | SOLID › ASSEMBLE | Disable Kontakt |
| Edit Contact Set | `contact.edit` | – | Rediger Kontakt Indstil |
| Enable All Contact | `contact.enable_all` | SOLID › ASSEMBLE | Enable All Kontakt |
| Enable Contact Sets | `contact.enable_sets` | SOLID › ASSEMBLE | Enable Kontakt Sets |
| List Contact Sets | `contact.list` | – | Vis Kontakt Sets |

## `diagnostics.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Crash Report | `diagnostics.crash_report` | – | Crash Report |
| Crash Reports | `diagnostics.crash_reports` | – | Crash Reports |
| Delete Crash Reports | `diagnostics.crash_reports.clear` | – | Slet Crash Reports |
| Crash Test | `diagnostics.crash_test` | – | Crash Test |

## `document.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Inspect Design | `document.inspect` | – | Undersøg Design |
| Design Properties | `document.metadata` | – | Design Egenskaber |
| Remove Design Property | `document.metadata.remove` | – | Fjern Design Property |
| Change Design Properties | `document.metadata.set` | – | Ændr Design Egenskaber |
| Change Design Units | `document.units` | – | Ændr Design Units |

## `engine.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| List Commands | `engine.commands` | – | Vis Commands |
| Describe Command | `engine.describe` | – | Beskriv Command |

## `explode.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Exploded View | `explode.create` | SOLID › ASSEMBLE | Exploded Visning |
| Delete Exploded View | `explode.delete` | – | Slet Exploded Visning |
| List Exploded Views | `explode.list` | – | Vis Exploded Views |
| Show Exploded View | `explode.show` | – | Vis Exploded Visning |

## `expr.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Evaluate Expression | `expr.evaluate` | – | Beregn Udtryk |

## `face.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Delete Face | `face.delete` | SOLID › MODIFY | Slet Flade |

## `library.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Apply Local Library Entry | `library.apply` | – | Anvend Local Bibliotek Entry |
| Export Design Library | `library.export` | – | Eksportér Design Bibliotek |
| Inspect Local Library | `library.inspect` | – | Undersøg Local Bibliotek |

## `material.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Physical Material | `material.assign` | SOLID › MODIFY | Fysisk Materiale |
| New Physical Material | `material.create` | – | Ny Fysisk Materiale |
| Delete Physical Material | `material.delete` | – | Slet Fysisk Materiale |
| Edit Physical Material | `material.edit` | – | Rediger Fysisk Materiale |
| List Materials | `material.list` | – | Vis Materialer |
| Directional Elastic Modulus | `material.properties.directional` | – | Directional Elastic Modulus |
| Validate Physical Material Data | `material.properties.validate` | – | Validate Fysisk Materiale Data |

## `mesh.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Convert Mesh | `mesh.convert` | – | Konvertér Mesh |

## `model.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| List Edges | `model.edges` | – | Vis Edges |
| List Faces | `model.faces` | – | Vis Faces |
| List Threads | `model.threads` | – | Vis Threads |

## `motion.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Delete Motion Study | `motion.delete` | – | Slet Bevægelse Studie |
| Export Motion Study | `motion.export` | – | Eksportér Bevægelse Studie |
| Export Motion Animation | `motion.export_animation` | – | Eksportér Bevægelse Animation |
| Motion Interference | `motion.interference` | – | Bevægelse Overlap |
| List Motion Studies | `motion.list` | – | Vis Bevægelse Studies |
| Play Motion Study | `motion.play` | – | Play Bevægelse Studie |
| Plot Motion Study | `motion.plot` | – | Plot Bevægelse Studie |
| Motion Study | `motion.study` | SOLID › ASSEMBLE | Bevægelse Studie |
| Motion Trace | `motion.trace` | SOLID › ASSEMBLE | Bevægelse Trace |
| Keep Motion Trace | `motion.trace.keep` | – | Keep Bevægelse Trace |

## `occurrence.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Paste (Instance) | `occurrence.copy` | – | Paste (Instans) |
| Ground | `occurrence.ground` | – | Fastlås |
| Move Occurrence | `occurrence.move` | – | Flyt Occurrence |
| Occurrence Physical Properties | `occurrence.properties` | – | Occurrence Fysisk Egenskaber |

## `parameters.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Add User Parameter | `parameters.add` | – | Add User Parameter |
| Change Parameters | `parameters.change` | SOLID › MODIFY | Ændr Parametre |
| Comment Parameter | `parameters.comment` | – | Comment Parameter |
| Complete Expression | `parameters.complete` | – | Complete Udtryk |
| Delete Parameter | `parameters.delete` | – | Slet Parameter |
| Export Parameters | `parameters.export` | – | Eksportér Parametre |
| Favourite Parameter | `parameters.favorite` | – | Favourite Parameter |
| Parameter Graph | `parameters.graph` | – | Parameter Graph |
| Import Parameters | `parameters.import` | – | Importér Parametre |
| List Parameters | `parameters.list` | – | Vis Parametre |
| Move Parameter | `parameters.move` | – | Flyt Parameter |
| Rename Parameter | `parameters.rename` | – | Omdøb Parameter |
| Parameter Users | `parameters.users` | – | Parameter Users |

## `parts.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Insert Fastener | `parts.fastener` | SOLID › INSERT | Indsæt Befæstelsesdel |
| Insert Part | `parts.insert` | – | Indsæt Del |
| Standard Parts | `parts.library` | – | Standard Dele |
| Change Part Size | `parts.resize` | – | Ændr Del Størrelse |

## `render.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Render Backends | `render.backends` | – | Render Backends |
| Cancel Render | `render.cancel` | – | Cancel Render |
| Export Render Image | `render.export` | – | Eksportér Render Image |
| Check Render Request | `render.preflight` | – | Kontrollér Render Request |
| Inspect Render Scene | `render.scene` | RENDER › SCENE | Undersøg Render Scene |
| Apply Render Camera | `render.settings.apply` | – | Anvend Render Kamera |
| Render Settings | `render.settings.get` | – | Render Settings |
| Save Render Settings | `render.settings.set` | – | Gem Render Settings |
| Render Image | `render.start` | RENDER › IMAGE | Render Image |
| Render Progress | `render.status` | – | Render Progress |

## `select.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Clear Selection | `select.clear` | – | Ryd Markering |
| Select | `select.set` | – | Vælg |

## `selection.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Delete | `selection.delete` | – | Slet |

## `view.*`

| Engelsk kommando | ID | Værktøjslinje | Dansk oversættelse |
|---|---|---|---|
| Ambient Occlusion | `view.ambient_occlusion` | – | Ambient Occlusion |
| Restore Named View | `view.apply` | – | Restore Named Visning |
| Delete Named View | `view.delete` | – | Slet Named Visning |
| Graphics Preset | `view.graphics_preset` | – | Graphics Preset |
| List Named Views | `view.list` | – | Vis Named Views |
| Navigate View | `view.navigate` | – | Navigate Visning |
| Rename Named View | `view.rename` | – | Omdøb Named Visning |
| New Named View | `view.save` | – | Ny Named Visning |


## Ordliste – tekniske betegnelser

De originale kommando- og API-navne står uændret. Kolonnen **Dansk oversættelse** giver en oversættelse af menupunktets betydning; det er ikke et nyt kommando-ID.

| Engelsk udtryk | Dansk betydning |
|---|---|
| Command | Kommando |
| ID | Kommandoidentifikator |
| Toolbar | Værktøjslinje |
| CREATE | Opret |
| MODIFY | Rediger |
| CONSTRUCT | Konstruér |
| INSPECT | Undersøg |
| SETUP | Opsætning |
| ASSEMBLE | Saml |
| Feature | Modelleringsfunktion |
| Planned | Planlagt / endnu ikke implementeret |
| Command Registry | Kommandoregister |

<!-- docs-english-updates:start -->

## Engelske ændringer, der mangler oversættelse

> Nedenfor står nye og ændrede afsnit fra den engelske original. Arbejd dem ind i oversættelsen, kontrollér den aktuelle engelske side, og fjern derefter hele denne markerede sektion.

The English original also removes or replaces text. Review the corresponding translated text.

## Sketch

| Command | Id | Toolbar |
|---|---|---|
| Center Point Arc | `sketch.arc.center_point` | SKETCH › CREATE |
| Tangent Arc | `sketch.arc.tangent` | SKETCH › CREATE |
| 3-Point Arc | `sketch.arc.three_point` | SKETCH › CREATE |
| AutoConstrain | `sketch.auto_constrain` | SKETCH › CONSTRAINTS |
| AutoConstrain from datum | `sketch.auto_constrain_from_datum` | SKETCH › CONSTRAINTS |
| Auto-Project Edges on Reference | `sketch.auto_project` | – |
| Blend Curve | `sketch.blend_curve` | SKETCH › MODIFY |
| Break | `sketch.break` | SKETCH › MODIFY |
| Break Link | `sketch.break_link` | – |
| Centerline | `sketch.centerline` | SKETCH › CREATE |
| Chain | `sketch.chain` | – |
| Distance and Angle Chamfer | `sketch.chamfer.distance_angle` | SKETCH › MODIFY |
| Equal Distance Chamfer | `sketch.chamfer.equal_distance` | SKETCH › MODIFY |
| Two Distance Chamfer | `sketch.chamfer.two_distance` | SKETCH › MODIFY |
| Center Diameter Circle (C) | `sketch.circle.center` | SKETCH › CREATE |
| 3-Point Circle | `sketch.circle.three_point` | SKETCH › CREATE |
| 3-Tangent Circle | `sketch.circle.three_tangent` | SKETCH › CREATE |
| 2-Point Circle | `sketch.circle.two_point` | SKETCH › CREATE |
| 2-Tangent Circle | `sketch.circle.two_tangent` | SKETCH › CREATE |
| Conic Curve | `sketch.conic` | SKETCH › CREATE |
| Sketch Constrainer | `sketch.constrainer` | SKETCH › CREATE |
| Coincident | `sketch.constraint.coincident` | SKETCH › CONSTRAINTS |
| Collinear | `sketch.constraint.collinear` | SKETCH › CONSTRAINTS |
| Concentric | `sketch.constraint.concentric` | SKETCH › CONSTRAINTS |
| Curvature | `sketch.constraint.curvature` | SKETCH › CONSTRAINTS |
| Equal | `sketch.constraint.equal` | SKETCH › CONSTRAINTS |
| Fix/UnFix | `sketch.constraint.fix` | SKETCH › CONSTRAINTS |
| Horizontal/Vertical | `sketch.constraint.horizontal_vertical` | SKETCH › CONSTRAINTS |
| MidPoint | `sketch.constraint.midpoint` | SKETCH › CONSTRAINTS |
| Parallel | `sketch.constraint.parallel` | SKETCH › CONSTRAINTS |
| Perpendicular | `sketch.constraint.perpendicular` | SKETCH › CONSTRAINTS |
| Polygon | `sketch.constraint.polygon` | SKETCH › CONSTRAINTS |
| Symmetry | `sketch.constraint.symmetry` | SKETCH › CONSTRAINTS |
| Tangent | `sketch.constraint.tangent` | SKETCH › CONSTRAINTS |
| Normal/Construction (X) | `sketch.construction` | SKETCH › CREATE |
| Copy Sketch Entities | `sketch.copy` | – |
| Create Sketch | `sketch.create` | SOLID › CREATE |
| Cut Sketch Entities | `sketch.cut` | – |
| Delete Sketch Entities | `sketch.delete` | – |
| Sketch Dimension (D) | `sketch.dimension` | SKETCH › CREATE |
| Move Dimension Text | `sketch.dimension_text` | – |
| Drag Sketch Geometry | `sketch.drag` | – |
| End Sketch Drag | `sketch.drag_end` | – |
| Draw Dimensioned Sketch Geometry | `sketch.draw.dimensioned` | – |
| Edit Sketch | `sketch.edit` | – |
| Edit Text | `sketch.edit_text` | – |
| Ellipse | `sketch.ellipse` | SKETCH › CREATE |
| Save As DXF | `sketch.export_dxf` | – |
| Extend | `sketch.extend` | SKETCH › MODIFY |
| Fillet | `sketch.fillet` | SKETCH › MODIFY |
| Finish Sketch | `sketch.finish` | SKETCH › FINISH SKETCH |
| Finish with AutoConstrain | `sketch.finish_auto_constrain` | SKETCH › FINISH SKETCH |
| Fit Curves to Mesh Section | `sketch.fit_curves_to_section` | SKETCH › CREATE |
| Constraint Glyphs | `sketch.glyphs` | – |
| Include 3D Geometry | `sketch.include_3d` | SKETCH › CREATE |
| Infinite Line | `sketch.infinite` | – |
| Insert DXF | `sketch.insert_dxf` | SKETCH › INSERT |
| Insert SVG | `sketch.insert_svg` | SKETCH › INSERT |
| Inspect Sketch | `sketch.inspect` | – |
| Intersect | `sketch.intersect` | SKETCH › CREATE |
| Intersection Curve | `sketch.intersection_curve` | SKETCH › CREATE |
| Isoparametric Curve | `sketch.isoparametric_curve` | SKETCH › CREATE |
| Line (L) | `sketch.line` | SKETCH › CREATE |
| Midpoint Line | `sketch.line.midpoint` | SKETCH › CREATE |
| 3D Line | `sketch.line3d` | – |
| Loose Ends | `sketch.loose_ends` | – |
| Tie Loose Ends | `sketch.loose_ends.tie` | – |
| Mirror | `sketch.mirror` | SKETCH › PATTERN |
| Move/Copy Sketch Entities | `sketch.move` | SKETCH › MODIFY |
| Move 3D Point | `sketch.move3d` | – |
| Drag Sketch Point | `sketch.move_point` | – |
| Offset (O) | `sketch.offset` | SKETCH › MODIFY |
| Break Offset Association | `sketch.offset.break` | – |
| Sketch Palette Options | `sketch.options` | – |
| Paste Sketch Entities | `sketch.paste` | – |
| Circular Pattern | `sketch.pattern.circular` | SKETCH › PATTERN |
| Rectangular Pattern | `sketch.pattern.rectangular` | SKETCH › PATTERN |
| Point | `sketch.point` | SKETCH › CREATE |
| 3D Point | `sketch.point3d` | – |
| Circumscribed Polygon | `sketch.polygon.circumscribed` | SKETCH › CREATE |
| Edge Polygon | `sketch.polygon.edge` | SKETCH › CREATE |
| Inscribed Polygon | `sketch.polygon.inscribed` | SKETCH › CREATE |
| Project (P) | `sketch.project` | SKETCH › CREATE |
| Project To Surface | `sketch.project_to_surface` | SKETCH › CREATE |
| Center Rectangle | `sketch.rectangle.center` | SKETCH › CREATE |
| Rounded Rectangle | `sketch.rectangle.rounded` | SKETCH › CREATE |
| 3-Point Rectangle | `sketch.rectangle.three_point` | SKETCH › CREATE |
| 2-Point Rectangle (R) | `sketch.rectangle.two_point` | SKETCH › CREATE |
| Redefine Sketch Plane | `sketch.redefine` | – |
| Sketch Scale | `sketch.scale` | SKETCH › MODIFY |
| Center Point Arc Slot | `sketch.slot.arc_center` | SKETCH › CREATE |
| Three Point Arc Slot | `sketch.slot.arc_three_point` | SKETCH › CREATE |
| Center Point Slot | `sketch.slot.center_point` | SKETCH › CREATE |
| Center to Center Slot | `sketch.slot.center_to_center` | SKETCH › CREATE |
| Overall Slot | `sketch.slot.overall` | SKETCH › CREATE |
| Snap Inference | `sketch.snap` | – |
| Solve Sketch | `sketch.solve` | – |
| Control Point Spline | `sketch.spline.control_point` | SKETCH › CREATE |
| Control Point Spline (Degree 5) | `sketch.spline.control_point_5` | SKETCH › CREATE |
| Fit Point Spline | `sketch.spline.fit_point` | SKETCH › CREATE |
| 3D Spline | `sketch.spline3d` | – |
| Spun Profile | `sketch.spun_profile` | SKETCH › CREATE |
| Text | `sketch.text` | SKETCH › CREATE |
| Toggle Driven Dimension | `sketch.toggle_driven` | – |
| Trim (T) | `sketch.trim` | SKETCH › MODIFY |
| Trim Along Path | `sketch.trim.path` | – |

## Assembly joints

| Command | Id | Toolbar |
|---|---|---|
| As-Built Joint | `joint.as_built` | SOLID › ASSEMBLE |
| Cam | `joint.cam` | SOLID › ASSEMBLE |
| Joint (J) | `joint.create` | SOLID › ASSEMBLE |
| Delete Joint | `joint.delete` | – |
| Drive Joints | `joint.drive` | SOLID › ASSEMBLE |
| Edit Joint | `joint.edit` | – |
| Gear | `joint.gear` | SOLID › ASSEMBLE |
| Inspect Joint | `joint.inspect` | – |
| Joint Limits | `joint.limits` | – |
| List Joints | `joint.list` | – |
| Motion Link | `joint.motion_link` | SOLID › ASSEMBLE |
| Joint Origin | `joint.origin` | SOLID › CREATE |
| Rack and Pinion | `joint.rack_pinion` | SOLID › ASSEMBLE |
| Reach Point | `joint.reach` | – |
| Rigid Group | `joint.rigid_group` | SOLID › ASSEMBLE |
| Screw | `joint.screw` | SOLID › ASSEMBLE |
| Solve Joints | `joint.solve` | – |
| Sweep Joint | `joint.sweep` | – |

<!-- docs-english-updates:end -->
