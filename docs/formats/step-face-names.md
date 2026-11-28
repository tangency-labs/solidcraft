# Name faces in a STEP export

Read the persistent face names of a body with `model.faces`. Supply the faces you want to label in `file.export_step`. For example, if an extrusion reports `F2:top` and `F2:bottom`, the following command gives both faces the same boundary name.

```json
{
  "command": "file.export_step",
  "params": {
    "path": "air.step",
    "bodies": ["Air"],
    "face_names": {"Air": {"F2:top": "outer", "F2:bottom": "outer"}}
  }
}
```

Each selected face keeps its geometry and receives its own `ADVANCED_FACE` record with the name `outer`. Other faces keep their empty STEP names. An unknown reference stops the export before the destination is written.

For Netgen, list the placed bodies of an assembly in `bodies` to create a flat STEP file. Netgen 6.2.2607 preserved names in flat exports but dropped them in a hierarchical assembly. The names are supplied for this export. Save the command in a modeling script to reuse the same selection after changing dimensions.
