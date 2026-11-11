# General: WR-527 addendum: the fork's own test measures time (2026-10-31)

Updates: plan/findings/general--flag-pin-wr527-faces-apart-is-speed-only.md

The fork's capi test (60b1c6a) times the perforated plate: 256 holes must take under 8 times as long as 64. It checks the area, which is exact either way. A deterministic port would need the fork's work counters (`smlib_work_counters`, the face pairs examined), which solidcore does not expose. The hunk also does not reverse cleanly on replay/1.0.1.
