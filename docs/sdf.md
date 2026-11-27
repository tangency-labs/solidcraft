# SDF model export

A design exports as a Gazebo model: a folder holding `model.config`, `model.sdf` (SDFormat) and
the files they name. Put the folder in a folder on `GZ_SIM_RESOURCE_PATH` (or a ROS package's
`models` folder) and Gazebo Sim lists it, or include it in a world with
`<include><uri>model://<name></uri></include>`. Images on the model (**Insert → Canvas** or
**Insert → Decal**, PNG, JPEG or SVG) are written as textured visuals, so an ArUco marker, a label or a logo shows in the simulation where SolidCraft
shows it.

- **Desktop:** **File → Export SDF Model…**, then a name and a place for the model's folder
- **Commands:** `file.export_sdf {"path": "models/Panel"}`, or `file.export {"path":
  "models/Panel.sdf"}` (or `"format": "sdf"`)
- **MCP:** the `export` tool with a `.sdf` path or `format: "sdf"`
- **CLI:** `solidcraft-cli run script.json --out models/Panel.sdf`

## The folder

`path` names the model's folder; a `.sdf` ending (the save picker adds one) is dropped, so
`models/Panel.sdf` writes the folder `models/Panel`. The folder is made if it is missing. A
file of the model already in it is replaced; any other file is left alone. A regular file where
the folder goes is refused.

```
Panel/
  model.config        the model's name, version and SDF file
  model.sdf           one static model, one link
  Body1.stl           each body, binary STL in metres
  Aruco13.png         each decal's image, as it is in the design (an SVG is already a PNG)
  Aruco13.dae         each decal's quad, a COLLADA mesh with texture coordinates
```

File names are the body and decal names with every character but `A-Z a-z 0-9 _ -` replaced by
`_`, kept apart without regard to case. Every path is checked, including against the folders an
agent may use (`allow_paths`), and the whole model is made before anything is written. The
meshes and images go first, `model.config` and then `model.sdf` last; if a write fails part way,
the error names the files already written and the one that failed.

## `model.sdf`

- **SDFormat 1.9**, which Gazebo Fortress and later read; newer versions convert it as they
  load it.
- **One model, one link** (`link`), in the design's frame: Z up, metres. Bodies are where the
  design places them. An exploded view, or a component moved but not captured, is shown only:
  it is not exported, with a warning.
- **Each body** is a `<visual>` with its STL and its colour as the material's ambient and
  diffuse (opacity below 1 as the visual's `<transparency>`), and a `<collision>` with the same
  STL when the body is a closed solid. An open surface is drawn with no collision, with a
  warning.
- **`static`** (default true): scenery Gazebo never moves, as a landmark or a panel is.
  `static: false` gives the link the closed bodies' mass, centre of mass and inertia, from
  each body's material density; with none, a token mass of 1 mg and a warning.
- **File names** are relative to `model.sdf`, which Gazebo Sim resolves against the file's
  folder. `model_uris: true` writes `model://<folder>/<file>` instead, for tools that resolve
  only model paths.
- **`name`** (default: the folder's name) is the model's name in both files.
- **`bodies`** (default: every body) and **`quality`** (`standard`, `fine`, `finest`: how
  closely the meshes follow curved faces) are as for the other mesh exports.

## Decals

Each shown image, canvas or decal, becomes one more `<visual>` of the link:

```xml
<visual name="Aruco13_visual">
  <geometry><mesh><uri>Aruco13.dae</uri></mesh></geometry>
  <material>
    <ambient>1 1 1 1</ambient>
    <diffuse>1 1 1 1</diffuse>
    <specular>0 0 0 1</specular>
    <emissive>0 0 0 1</emissive>
    <pbr>
      <metal>
        <albedo_map>Aruco13.png</albedo_map>
        <metalness>0</metalness>
        <roughness>1</roughness>
      </metal>
    </pbr>
  </material>
  <cast_shadows>false</cast_shadows>
</visual>
```

- The quad is the decal's rectangle as SolidCraft shows it, mirrored when the decal is, and
  where its sketch puts it when it is anchored (`canvas.frame_sketch`). Its texture
  coordinates put the image's bottom-left corner on the rectangle's bottom-left, so a marker
  keeps its turn and handedness, and with them its id.
- It is written on both sides, 0.2 mm off the face each way, so the face never hides it and one
  side faces out whichever way the face points.
- The albedo map is the image, as Gazebo Sim's PBR material takes it; the COLLADA file names
  the image as its own texture too, for readers without PBR materials. Gazebo Sim cuts out the
  image's clear pixels; the COLLADA material is opaque.
- Every shown image is written, whether it was placed with **Insert → Canvas** or **Insert →
  Decal**: hide an image you traced from (`canvas.edit {visible: false}`, or Show/Hide on it)
  to keep it out. One whose image is damaged, or neither PNG nor JPEG, is left out with a
  warning. An image made partly transparent in SolidCraft (`opacity` below 1) is written
  opaque, with a warning.

## Not covered

- Joints and moving parts: the model is one link. Export a URDF (docs/urdf.md) for a robot.
- A decal on a curved face: a decal lies on a plane.
- Body textures other than decals (render setups' images and optics).
