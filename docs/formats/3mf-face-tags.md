# 3MF with B-rep face tags

Version 1 (2026-10). Namespace `https://github.com/bherbruck/solidcraft/formats/3mf-face-tags/2026/10`,
prefix `ft`.

A tessellated body loses its faces: a plain mesh says where the surface is, not which B-rep face
each triangle came from. Downstream tools that mesh or analyse a part need that back. They apply
loads and boundary conditions to faces, keep face boundaries as feature edges, and group
results by face. SolidCraft therefore exports a 3MF in which every triangle carries the index of
the B-rep face it lies on (**File → Export 3MF…**, then the format **3MF with face tags**, or `file.export` with
`format: "3mf-tagged"`).

## Why 3MF

The file is an ordinary **3MF package** (3MF Consortium, *3D Manufacturing Format Core
Specification* 1.3). The tags are one attribute in a namespace of SolidCraft's. The Core
specification allows that attribute on `<triangle>` and `<object>`, and every consumer ignores it
because the namespace is not listed in `requiredextensions`. Slicers, viewers and SolidCraft's
own mesh import open the file as plain 3MF. A tool that knows the namespace gets the faces too.
A purpose-built binary format would have needed its own specification and readers, and would
open nowhere else. Decimal coordinates lose nothing: written in shortest round-trip form, every
`f64` reads back bit for bit.

## Example

```xml
<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xml:lang="en-US"
       xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02"
       xmlns:ft="https://github.com/bherbruck/solidcraft/formats/3mf-face-tags/2026/10">
 <metadata name="Application">SolidCraft</metadata>
 <resources>
  <object id="2" type="model" name="Body1" ft:faces="7">
   <mesh>
    <vertices>
     <vertex x="0" y="0" z="0"/>
     ...
    </vertices>
    <triangles>
     <triangle v1="0" v2="2" v3="1" ft:face="0"/>
     ...
    </triangles>
   </mesh>
  </object>
 </resources>
 <build>
  <item objectid="2"/>
 </build>
</model>
```

## Attributes

| Attribute | On | Value | Meaning |
|---|---|---|---|
| `ft:faces` | `<object>` | integer ≥ 1 | Number of B-rep faces of the body. Every `ft:face` of the object is below it. Faces without triangles still count. |
| `ft:face` | `<triangle>` | integer ≥ 0 | The B-rep face (0-based, in the body's own face order) the triangle lies on. |

An object's triangles are either all tagged or none. A reader treats an object without tags as a
single face (tag 0).

## What SolidCraft writes

- `unit="millimeter"`, Z up. There is one `<object type="model">` with a `<mesh>` per body,
  named after the body, and one `<item>` per object, with no transform and no components.
- The tessellation is the export mesh (chord tolerance 2·10⁻⁴ of the body size). It is welded
  watertight: vertices within 10⁻⁶ of the body size are merged and T-junctions between faces are
  split. Each piece of a split triangle keeps its face. Triangles are counter-clockwise seen from
  outside, and degenerate triangles are dropped.
- Coordinates are the tessellation's own `f64` values in shortest round-trip form, and vertices
  are shared by exact value.
- Colours use 3MF's own `basematerials` (`pid`/`p1`), as in the plain 3MF export, independent of
  the tags.
- The bytes are deterministic: fixed part order, fixed zip timestamps, no hash-ordered output.

## Versions

A later version gets a new namespace (`…/3mf-face-tags/<year>/<month>`). A reader accepts the
versions it knows and treats an unknown face-tag namespace as untagged.
