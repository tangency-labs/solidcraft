# corpus.parity: the ASCII round trip's volume drift is upstream's own (2026-10-23)


- **What.** Nine corpus models (Solids/sphere, sphere, ssi_coincident_spheres,
  ssi_sph_plane1, blade_hub_1/2/3, cliff_1/2) read back after a write (`SmBrepData::FromBrep`,
  `WriteToFile` ASCII, `ReadFromFile`) with a volume 1.2e-5 to 7e-5 relative away from the
  read one, area nearly unchanged; the corpus example's round trip reports them.
- **Evidence (Solids/sphere.smb).** C++ wrote the B-rep its own `ReadFromFile` made
  (`SmBrep::WriteToFile(path, SM_ASCII, TRUE, FALSE, 0.0)`, called in gdb in the
  corpus.parity C++ child); the port's bytes came from the corpus example's
  `BrepData::write_to_file`. The two files are byte-identical (13 571 bytes). Read back,
  C++ measures volume 523.5923997299045 and area 314.160106233197 (read: 523.5990772283121
  and 314.15938308003854); the port's own round trip and the port reading the C++ file
  give the same values bit for bit.
- **Cause.** Upstream's writer prints doubles in fixed format (`%.16f`), which drops
  significant digits of small coordinates and weights; the read-back geometry differs
  slightly and so does the volume integral. The port writes the same bytes.
- **Now.** corpus.parity round-trips every file whose read passes on both sides
  (`sc_corpus_parity_roundtrip`, children `cpp-roundtrip` / `rust-roundtrip`) and compares
  the read-back measures at `10 * ACCURACY`: the drift is expected, a difference between
  the sides is a regression.
