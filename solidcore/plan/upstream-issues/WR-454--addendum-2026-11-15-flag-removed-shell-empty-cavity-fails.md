Updates: WR-454

#### WR-454 (update, 2026-11-15). `shell_empty_cavity_fails` is removed; an inward shell that leaves no cavity is always refused

`ShellBrep` (`offset/offset_executive/p2.rs`) always refuses an inward shell of a solid whose offset is empty
(`SM_ERR_INVALID_INPUT`, 1007), before the shell's no-closed-solid guard (WR-578, 1001). The opt-in described in the
original record no longer exists. Removed: the field, SolidCraft's preset line, its `fixes.json` entry, the
flag-off branch, and the flag settings in the shell tests.

- **Defaults:** SolidCraft's preset had the flag on, so the app's results don't change. `Kernel::default()` changes.
- **Goldens:** no row of any unit moves (the survey's 17 rows were all refused on main already by the WR-578 guard).
- **Tests:** `issue_380_shell_with_no_cavity_fails_with_the_fix` asserts the 1007 refusal for walls of 5 and 6 on a
  10 box and that a wall of 1 still shells; its flag-off half (1001) is removed.
