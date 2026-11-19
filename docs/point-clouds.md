# Point clouds

A point cloud is scanned points kept in the design as a reference to model from (#5151): a
lidar or SLAM map, a photogrammetry result, a part from a handheld scanner. It is drawn as dots
in the viewport, depth-tested with the bodies, and never becomes a body; nothing in the timeline
depends on it.

## Files

| Extension | What is read |
|-----------|--------------|
| `.ply` | ASCII, binary little-endian and binary big-endian PLY: the `vertex` element's `x`, `y`, `z` (any numeric type) and its `red`, `green`, `blue` when present (bytes, 16-bit or 0…1 floats). Every other scalar vertex property (intensity, a class, a time…) is kept, by its name in the file, to colour and filter the cloud by; it is whole-numbered when its type is an integer. The `edge` element (`vertex1`, `vertex2`) and the `face` element (`vertex_indices`, a face as a fan of triangles) become segments and triangles between the points, with their own properties. Other elements are passed over. |
| `.pcd` | The Point Cloud Library's PCD, with its points stored as text (`DATA ascii`), as binary rows (`binary`) or as an LZF-compressed block (`binary_compressed`): the fields `x`, `y`, `z` (any numeric type) and the packed `rgb` or `rgba` field when present (its alpha is passed over). Every other field with one value a point (intensity, a label, a normal's parts…) is kept, by its name in the file, to colour and filter the cloud by; it is whole-numbered when its `TYPE` is `I` or `U`. A field with several values a point (a descriptor) and the padding fields named `_` are passed over. A grid of points (`WIDTH` by `HEIGHT`) is read as its points in order, and the points it marks as missing (`nan`) are left out with a warning. `VIEWPOINT`, where the scanner stood, is not applied: the points keep the file's coordinates. |
| `.xyz`, `.pts`, `.asc` | Text, a point per line: `x y z`, optionally followed by `r g b` (six numbers) or by an intensity and `r g b` (seven or more, as PTS files write it). Spaces, tabs, commas or semicolons separate the numbers; `#` and `//` start comments; a PTS file's first line (its count) is skipped. An `.asc` file that is an ESRI grid of heights (it starts with `ncols`) is refused: it holds rows of heights, not points. |

A PLY file with faces can also be inserted as a mesh body with **Insert Mesh**
(`file.insert_mesh`), which makes it solid geometry rather than a reference.

Point cloud files say nothing about their length unit. `units` (`mm`, `cm`, `m`, `in`, `ft`)
says what it is; the default is millimetres, as for every other import. Lidar and SLAM maps are
usually in metres. The unit can be changed afterwards (Browser › Point Clouds › Edit Point Cloud,
or `pointcloud.edit` with `units`): the points are scaled from the unit they came in as.

## Reloading

A cloud remembers where its file was (`source`, the absolute path it was inserted from), so a
file that changed can be read again: **Reload from file** in the Edit Point Cloud card, or
`pointcloud.reload`. The points, colours, segments, triangles and properties are replaced; the
name, placement, units, point size and colouring are kept, and so are the filters on properties
the file still has. A filter on a set of named values goes when the file no longer names them.
Colouring by a property the file no longer has goes back to the file's colours. The file is read
under the limit the cloud was inserted with (`max_points`), so the same file reads as the very
same points and a file that grew comes in whole unless it exceeds that limit, when it is thinned
as an insert would thin it. The file is read beside the window, with
progress and Cancel. A cloud opened from a design saved before it remembered its file cannot be
reloaded. A file on another computer (`\\host\share`) is treated as a linked design's is:
**Reload from file** asks before it connects to that server, and `pointcloud.reload` is refused
until the server is allowed in Preferences.

## Colours

**Color By** in the Edit Point Cloud card (or `color_by`) says how the points are coloured:

- `file` (the default): the file's colour of each point, grey when it has none.
- `single`: one colour (`color`) for every point.
- `height`: a ramp by design Z, blue low through cyan, green and yellow to red high. It runs
  between two heights (`color_range`, mm). By default they are worked out from the points: all
  but the lowest and highest 2 %, so a few stray points don't flatten the ramp.
- any property's name (`x`, `y`, `z` or one of the file's): the same ramp over the property's
  values or, for a whole-numbered property, a palette of ten colours (its value modulo ten;
  negative values grey). By default the ramp runs exactly over the values the property takes on
  the points every filter keeps, so filtering re-spreads the colours over what is left.

A ramp's range is a [range track](#range-tracks) in the card; touching it sets the range and
turns **Automatic Range** off, and ticking the box (or a double click beside the range) gives
it back. `color_range: null` does the same from a command.

A whole-numbered property may name its values with a PLY comment, `property: 0 name, 1 other
name, -1 none` (for example `comment role: 0 outer wall, 1 inner wall`). Coloured by it, the
card shows a legend, a swatch and a name per value; clicking a name shows or hides that value.

## Edges and faces

A cloud read from a PLY file with edges or faces draws them between its points: triangles
filled, segments as lines or, when they have `width` and `height` properties, as beads of that
stadium cross-section (a tube when the two are equal), meeting mitred where one runs into the
next. Points no segment or triangle ends on stay dots, of the cloud's point size. A point that
can't be read (not a finite number, or out of range) is left out with a warning, and so are the
segments and triangles that end on it.

A property on the points varies along the segments and across the triangles that meet there;
one on the segments or triangles is constant on each. Colours follow the same rule, and a range
filter cuts an element exactly where its value crosses an end of the range, so a filter on a
time property replays a path as it was drawn. A range track weighs each element by its measure,
each kind over its own total: a dot counts 1, a segment its length, a triangle its area.

## Filters

A filter keeps the points whose property lies in a range (both ends included) or, for a
property with named values, is one of a set of values. Only what every filter keeps is shown; a
cloud has at most 8 filters, and the view applies the first 4. **Filter by** in the card adds
one over a property's whole span (or with all its named values); × removes it. Filters change
what is drawn, never the points the design holds.

## Range tracks

Each range filter, and a ramp's range, is edited on a track, as a range is in an audio editor.
The track draws how much of the cloud lies at each value: a histogram of the property over what
the *other* filters keep, stacked by colour, so it is also the colour legend. A thin strip above
it shows the property's whole span, with the part the track shows framed.

- Drag an end or the band between them to move them; drag elsewhere to draw a new range.
  Shift-click brings the nearer end to the pointer.
- Double-click the band to fit the track to it; double-click beside it for the whole span.
  The wheel or a pinch zooms around the pointer, a sideways scroll pans, and dragging in the
  strip moves the framed part.
- An end snaps to a value where the colour changes when no other change is near at this zoom;
  a whole-numbered property's ends snap to whole numbers. ⌥ moves ten times slower, without
  snapping.
- Once the track is touched, ← and → bring the part touched last to the previous or next colour
  change (one pixel with ⌥, or where there is none).
- **Start**, **End** and **Length** are fields too; Length keeps the start. A value typed with a
  sign is relative: `+30` moves it by 30.
- **▶ Play** (or Space) moves the part touched last across the track in 20 seconds: an upper
  end grows the range, the band slides it, a lower end wipes it. The track follows it, and an
  automatic colour range follows it too. Touching the range stops it.

## Size

A cloud keeps at most `max_points` points (5,000,000 unless asked, at most 20,000,000); a larger
file is thinned evenly, one point in *n*, and the result says how many were kept. Thinning would
cut a file's edges and faces, so a thinned file leaves them out, with a warning; raise `max_points`
to keep them. A cloud holds at most 20,000,000 segments and as many triangles. Files up to
1 GiB are read; a larger one is read when the limit for STEP and import files is raised
(Preferences › General › Largest file to open, or `--max-file-size`), and a file over the limit is
refused before it is read, saying how to raise it. The points are stored single precision relative
to the cloud's first point (kept double precision), so a scan far from the origin keeps its
detail; they travel inside the design file as a binary part (12 bytes a point, 3 more with
colours).

## Deviation from a body

`pointcloud.deviation` measures how far each point of a cloud lies from a body. It reads the
cloud and the body where they are placed now and changes nothing: no placement, no colour, no undo
step. It does not align the cloud, scale it or leave points out as outliers.

- **Exact geometry.** Each distance is the kernel's closest-point solve on the body's faces as they
  are trimmed, with their edges and vertices. A point past a face's edge is measured to that edge,
  not to the face's unbounded surface. The display mesh plays no part, so a finer or coarser view
  gives the same result. A mesh body is refused.
- **Sign.** For a body that encloses a volume, measured on all its faces, each distance has a sign:
  positive outside the material, negative inside it, zero on the boundary within the body's own
  tolerance. The kernel's exact point classification decides the side. A face set (`faces`) or an
  open body has no inside, so its distances are unsigned and `signed: true` is refused.
- **Sampling.** At most `max_samples` points are measured (100,000 unless asked). A larger cloud is
  sampled at every *n*-th point, from the first point, with *n* the smallest whole number that keeps
  the count within the limit. The result gives `cloud_points`, `sample_step`, `sampled`, `measured`
  and `failed`, and `sampled` = `measured` + `failed`. A point the kernel cannot measure is listed in
  `failures` and left out of the statistics; it never counts as 0.
- **Statistics.** `mean_abs_mm`, `rms_mm`, `max_abs_mm`, `mean_signed_mm` (signed only) and
  `within_tolerance` count each measured point once; they are not weighted by area.
  `within_tolerance_fraction` is `within_tolerance` / `measured`. With no measured point, each is
  `null`, not 0. `worst` lists the farthest points with their closest point on the body, and
  `per_point: true` adds every distance in sampling order.
- **Filters and visibility** change only what the view shows. Every point of the cloud is a
  candidate to measure.

What the numbers do and do not say:

- **Numerical accuracy.** The kernel solves each distance on the exact geometry;
  `body_tolerance_mm` is the body's own tolerance. The cloud stores its coordinates as 32-bit floats,
  and `point_precision_mm` says how far a stored point may lie from the value in its file.
- **Sampling error.** `max_abs_mm` is the largest distance among the measured points, not a
  certified maximum. A point the cloud does not have, or that sampling skipped, may lie farther.
- **Coverage.** The distances go from the points to the body only. A part of the body with no
  points near it does not show up, so a missing feature or a gap in the scan can go unseen.
- **Scanner uncertainty.** The scanner's own noise and errors are in the points, and the command
  cannot tell them apart from a real difference in shape. The result is a sampled geometric
  comparison, not a check of conformity to a specification.

## Commands

- `pointcloud.insert` — `path`; `name?`, `units?`, `max_points?`, `center?` (move the middle of
  its base to the origin), `point_size?` (pixels), `color_by?` (`file`, `single`, `height` or a
  property's name), `color?` (`[r, g, b]` 0…1; alone it means `single`, `null` means `file`),
  `color_range?` (`[low, high]`, or `null` for the automatic range; `height_range` is the same),
  `filters?` (`[{"property": …, "range": [low, high]}` or `{"property": …, "values": [whole
  numbers]}]`, or `null` for none).
- `pointcloud.edit` — `cloud` (id or name); `name?`, `visible?`, `point_size?`, `color_by?`,
  `color?`, `color_range?`, `filters?`, `units?`, `matrix?` (a rigid placement, as
  `occurrence.move` takes it).
- `pointcloud.reload` — `cloud`; reads its file again (see [Reloading](#reloading)).
- `pointcloud.delete`.
- `pointcloud.deviation` — `cloud`, `body`; `faces?`, `tolerance?` (mm, default 0.1),
  `max_samples?`, `signed?`, `worst?`, `per_point?` (see [Deviation from a body](#deviation-from-a-body)).
- `pointcloud.list` — each cloud with its `source`, its segment and triangle counts, its properties (name, whole-numbered or not, span, named
  values), its filters and its ramp's range as drawn (`color_range`).
- `doc.open` opens a point cloud file as a new design holding it; dropping one on the window
  adds it to the open design.

## Example

```json
{"command": "pointcloud.insert", "params": {"path": "glim-points.ply", "units": "m", "color_by": "height"}}
{"command": "pointcloud.edit", "params": {"cloud": "glim-points", "color_by": "intensity", "filters": [{"property": "z", "range": [0, 2500]}]}}
```
