# Point clouds

A point cloud is scanned points kept in the design as a reference to model from (#5151): a
lidar or SLAM map, a photogrammetry result, a part from a handheld scanner. It is drawn as dots
in the viewport, depth-tested with the bodies, and never becomes a body; nothing in the timeline
depends on it.

## Files

| Extension | What is read |
|-----------|--------------|
| `.ply` | ASCII, binary little-endian and binary big-endian PLY: the `vertex` element's `x`, `y`, `z` (any numeric type) and its `red`, `green`, `blue` when present (bytes, 16-bit or 0…1 floats). Other properties (normals, intensity) and elements are passed over. |
| `.xyz`, `.pts` | Text, a point per line: `x y z`, optionally followed by `r g b` (six numbers) or by an intensity and `r g b` (seven or more, as PTS files write it). Spaces, tabs, commas or semicolons separate the numbers; `#` and `//` start comments; a PTS file's first line (its count) is skipped. |

A PLY file that also has faces can be inserted as a mesh body instead with **Insert Mesh**
(`file.insert_mesh`); **Insert Point Cloud** then mentions it in its warnings.

Point cloud files say nothing about their length unit. `units` (`mm`, `cm`, `m`, `in`, `ft`)
says what it is; the default is millimetres, as for every other import. Lidar and SLAM maps are
usually in metres. The unit can be changed afterwards (Browser › Point Clouds › Edit Point Cloud,
or `pointcloud.edit` with `units`): the points are scaled from the unit they came in as.

## Colours

**Color By** in the Edit Point Cloud card (or `color_by`) says how the points are coloured:

- `file` (the default): the file's colour of each point, grey when it has none.
- `single`: one colour (`color`) for every point.
- `height`: a ramp by design Z, blue low through cyan, green and yellow to red high. It runs
  between two heights (`height_range`, mm). By default they are worked out from the points: all
  but the lowest and highest 2 %, so a few stray points don't flatten the ramp. Turn
  **Automatic Range** off in the card to set them.

## Size

A cloud keeps at most `max_points` points (5,000,000 unless asked, at most 20,000,000); a larger
file is thinned evenly, one point in *n*, and the result says how many were kept. Files up to
1 GiB are read. The points are stored single precision relative to the cloud's first point (kept
double precision), so a scan far from the origin keeps its detail; they travel inside the design
file as a binary part (12 bytes a point, 3 more with colours).

## Commands

- `pointcloud.insert` — `path`; `name?`, `units?`, `max_points?`, `center?` (move the middle of
  its base to the origin), `point_size?` (pixels), `color_by?` (`file`, `single`, `height`),
  `color?` (`[r, g, b]` 0…1; alone it means `single`, `null` means `file`), `height_range?`
  (`[low, high]` mm, or `null` for the automatic range).
- `pointcloud.edit` — `cloud` (id or name); `name?`, `visible?`, `point_size?`, `color_by?`,
  `color?`, `height_range?`, `units?`, `matrix?` (a rigid placement, as `occurrence.move` takes it).
- `pointcloud.delete`, `pointcloud.list`.
- `doc.open` opens a point cloud file as a new design holding it; dropping one on the window
  adds it to the open design.

## Example

```json
{"command": "pointcloud.insert", "params": {"path": "glim-points.ply", "units": "m", "color_by": "height"}}
```
