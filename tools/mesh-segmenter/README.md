# Mesh segmenter

`mesh_to_cad` (docs/mcp.md) reads a mesh body as the surfaces it stands for. It groups the
triangles itself, by growing a patch across edges while the normals stay close, and that is enough
for a mesh that came from CAD. On a scan, where a face is noisy and an edge is rounded, a learned
segmentation often groups the triangles better than any angle threshold does.

This program is that second opinion. It reads an STL, asks an open-source network which triangles
belong to one face, and writes the answer as the `groups` argument of `mesh_to_cad`:

```sh
python segment.py --fetch                                 # once: download the model
python segment.py --mesh scan.stl --out groups.json
```

```json
{"triangles": 292084, "points": 8192, "model": "point2cyl-without-sketch", "groups": [0, 0, 3, ...]}
```

Then call the tool with that array:

```json
{"name": "mesh_to_cad", "arguments": {"body": "Scan", "groups": [0, 0, 3, "..."], "tolerance": "0.2 mm"}}
```

The STL has to be the one SolidCraft wrote for that body (`execute file.save_mesh
{path, bodies: ["Scan"], format: "stl"}`), because the groups are matched to the body's triangles
by position in the list. The export walks a mesh body's triangles in the order the body holds
them, so the two line up; `mesh_to_cad` refuses an array of the wrong length rather than
misapplying it, and drops a proposal it cannot line up with the triangles welding kept.

## The proposal never decides the geometry

SolidCraft fits every surface by least squares and then certifies it against the mesh itself, and
it splits any group whose fit will not certify within the tolerance. A good proposal therefore
buys recognition -- more of the part named, fewer faces broken into pieces -- and a bad one costs
nothing but the time. Nothing this program says is taken on trust, which is why it can be an
external, optional step at all.

## The model

[Point2Cyl](https://github.com/mikacuy/point2cyl) (Uy, Chang, Sung, Goel, Lambourne, Birdal,
Guibas; CVPR 2022), **MIT licence**. It takes a point cloud and labels each point with the
extrusion it belongs to, together with whether that point is on the extrusion's base or its
barrel -- which is exactly the decomposition a parametric design wants, rather than a per-point
primitive type. `--fetch` downloads three files from its repository and checks each against a
SHA-256 pinned in `segment.py`:

| File | SHA-256 | Bytes |
|---|---|---|
| `models/pointnet_extrusion.py` | `59660ecd…3ceef4` | 2,641 |
| `models/pointnet_util.py` | `b8a090bc…c498e0` | 11,315 |
| `results/Point2Cyl_without_sketch/model.pth` | `11d4092a…96785d` | 5,689,993 |

The checkpoint was trained on the Fusion 360 Gallery and DeepCAD datasets, at 8192 points and at
most 8 extrusions, on shapes normalized to about unit size, which is what `--points`,
`--normalize` and the 8 groups here follow.

### Why this model and not another

- **Point2CAD** (ETH Zurich, CVPR 2024) fits a B-rep straight from a point cloud and would be the
  obvious choice, but its code and weights are released under **CC BY-NC 4.0**: research and
  personal use only. SolidCraft is MIT OR Apache-2.0 and cannot build on it.
- **CAD-Recode** and **cadrille** generate CadQuery programs. CadQuery is one of the projects
  SolidCraft's ground rules keep it clear of (AGENTS.md), so neither its code nor its output
  format can come anywhere near this repository.
- **ComplexGen** (MIT) and **SPFN** (MIT) are both usable licence-wise; they predict a B-rep chain
  complex and per-point primitive types respectively. Point2Cyl was chosen over them because
  sketch-plus-extrusion is what SolidCraft's timeline is made of, so its output maps onto real
  features instead of needing another reconstruction step.

## It is a separate program, not a dependency

Nothing here is part of SolidCraft's build, its tests or its binaries, and SolidCraft ships none
of it. The model files `--fetch` downloads stay in `tools/mesh-segmenter/model/`, which is not
packaged and not committed. The application stays pure Rust and builds for the browser, as
AGENTS.md requires; this is a Python program a person runs when they want it, like
`tools/freecad-oracle/`.

## Setting it up

PyTorch on the CPU is enough -- the network is 1.4 M parameters and one run takes seconds.

```sh
python -m venv env
env/bin/pip install -r requirements.txt      # Windows: env\Scripts\pip
python segment.py --fetch
```

`requirements.txt` asks for the CPU build of PyTorch. For a GPU build, install PyTorch however
you normally would instead; this program does not care which it gets.

## What is checked

`python segment.py --self-test` covers the parts that need no model: reading binary and ASCII
STLs, spreading the sample points over the surface by area while keeping every triangle's own
middle (so no triangle can go unlabelled), the majority vote that turns per-point labels back
into per-triangle groups, and the normalization. It needs only numpy.

The inference path itself -- loading the checkpoint and running it -- has **not** been run here;
the environment this was written in would not execute the downloaded model. Run `--self-test`,
then `--fetch`, then one real mesh, and treat the first run as the test. If the checkpoint turns
out to want `--normalize sphere` rather than the default box, say so here.
