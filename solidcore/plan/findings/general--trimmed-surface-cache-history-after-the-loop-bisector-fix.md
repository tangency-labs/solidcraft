# trimmed surface cache history after the loop bisector fix (2026-10-25)

Follow-up to "trimmed surface cache history: tolerance-growth deferral removed", re-run
after merging main's `SmFace::ClassifyLoops` loop bisector fix (the bisector's `IsSeam` and
classifications now share one snapshot and keep their UV trims and tolerance growth).
- `heal.heal_data_p1` `file_Endcap` and `file_Endcap_rebuild_reverse` (Endcap.smb HealTgts)
  now pass, so their known-mismatch entries are removed.
- `topology.brep_p1` `Endcap_heal` (Endcap.smb HealBrep, healer op 26) still disagrees, now
  in the output length: C++ 245 values, port 256 (previously out[234] and out[238]
  differed). Cause still not found; it stays a known mismatch.
- The 9 torus seam cases (`heal.face_props` gen0646/gen0790/gen1312, `intersect.curve_class_p4`
  gen1570/gen1622/gen1841/gen1876/gen1968/gen2061) and `query.face_p4`'s 2 entries are unchanged.
