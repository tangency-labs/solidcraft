# General: WR-541 addendum: the fork's own tray test does not diverge either (2026-10-31)

Updates: plan/findings/general--flag-reachability-wr541-ball-centre-not-reproduced.md

The fork's capi test (bc6ddf9) runs the tray read back through the flat arrays, with its bottom rounds as cylinders. It passes on the fork's replay/1.0.1 (0f7b209) with bc6ddf9's hunk reverted. That read-back tray, exported from the fork and read into the port, rounds to the closed form (2.9e-12) with and without the flag.
