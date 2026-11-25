Updates: WR-442

#### WR-442 (update, 2026-11-15). `revolve_nested_shells_as_cavities` is removed

After a full turn's stitch, a closed shell inside another becomes that solid's cavity (`nest_closed_shells_as_cavities`). That is now the only path, and the flag is gone. `revolve_holed_profile_void` (WR-529) stays a flag of its own. Removed: the field, the goldens runner's arm, SolidCraft's preset line, its entry in `fixes.json`, fixture 451's opt-ins and every test setting of it.

- **Results:** no golden changes.
- **Tests:** `issue_478_full_turn_nested_shell_is_a_cavity` in `primitive::primitive_creation::p3::sweeps` and `fixtures/solidcraft/451-hollow-shell-cut.json`.
