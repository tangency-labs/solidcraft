# topology.part_file: SmBrepData part reader and the B-rep writer were outside the plan (2026-10-23)

## General: `SmBrepData` part reader and the B-rep writer are missing from the plan (2026-10-23)

`harness/callgraph/units.json` lists only the single-B-rep read side of `SmBrepData`
(`ReadFromFile`, `ReadFromDB`, `ReadCurveFromDB`, `ReadSurfaceFromDB`, `Get/SetHealerVersion`).
Nothing reaches the part functions or the write side from the plan's test entry points,
so `ReadPartFromFile` / `ReadPartFromDB` / `ReadAttributeFromDB` (and
`SmAttribute::CreateAttribute`) were never units. Neither were `WriteToFile` / `WriteToDB`
/ `WritePartToFile` / `WritePartToDB` / `WriteCurveToDB` / `WriteSurfaceToDB` /
`WriteAttributeToDB`, the `SmDatabaseIO` `Write*` side or `FromBrep`. "Every unit green or
partial" therefore does not mean the port can read upstream's 74 `.smp` test parts (plus
`SmallBox.smb` and `Platform.smb`, which are part files too) or write any B-rep back.
`topology::part_file` (fix/part-reader) ports the read side outside the plan:
`read_part_from_file`, `read_part_from_db`, `read_attribute_from_db` and `create_attribute`,
plus `InputStream::getline` and `DatabaseReader::read_characters`. The difftest unit `topology.part_file` (shim `topology_part_file.cpp`) compares it with
upstream's `ReadPartFromFile`; the corpus runner (`examples/corpus`) exercises them on the real
files. The writer (`FromBrep` plus the `Write*` functions) is `topology::brep_data_write` (fix/brep-writer, salvaged onto swarm/b1 2026-10-25).
