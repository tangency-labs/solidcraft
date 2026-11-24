# ASCII STL with one solid per face

CFD meshers such as OpenFOAM's snappyHexMesh and cfMesh read a multi-solid ASCII STL and make
each `solid <name>` ... `endsolid <name>` block a boundary patch, which the user then gives a
boundary condition by name. SolidCraft writes that file from the same mesh as its plain STL
export: `file.export` with `format: "stla"`, or `file.export_stl` with `ascii: true`, and
`solids: "face"`. The default, `solids: "body"`, is the plain export, byte for byte.

```
solid F1:+x
  facet normal ...
  ...
endsolid F1:+x
solid F1:-x
  ...
```

- **One block per B-rep face** that has triangles, with the triangles the plain export writes
  (the same `quality`, `bodies` and `lay_on_print_faces`), grouped by the face each came from.
  The bodies are welded as for the plain export, so the faces meet at identical vertices and the
  union of the blocks is as closed as the plain file.
- **Block name:** the face's persistent name (the one `model.faces` lists, such as `F1:+x`). Any
  character that is not printable ASCII (white space, control characters, non-ASCII) becomes `_`,
  so a name is one word. With several bodies each name is led by the body's name and `_`.
- **Unique and stable:** a name already used in the file gets `_2`, `_3`, ... after it. The names
  and their order depend only on the design, so they are the same on every export.
- **ASCII only:** `solids: "face"` with binary STL, or with any other format, is refused with an
  error naming `solids`; nothing is written.
