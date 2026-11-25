Updates: WR-458

#### WR-458 (update, 2026-10-27). Rechecked against v1.0.1 (3fd5b95)

Unchanged in v1.0.1: `SmOffsetSurface.cpp` is identical to c9c5979, so `sm_ExtendSurface` (`:463`-`:569`) and its calls (`:697`, `:1051`) are where the record says. Rust: `issue_419_offset_extension_does_not_depend_on_evaluation_order` still passes on `main` with the v1.0.1 port (the two copies differ by more than 1e-6 by default, by less than 1e-9 with `offset_extension_from_base`). The default path agrees with the C++ 1.0.1 in difftest (`surface.offset_surface`: 3082 cases, all pass, max_rel_err 0).
