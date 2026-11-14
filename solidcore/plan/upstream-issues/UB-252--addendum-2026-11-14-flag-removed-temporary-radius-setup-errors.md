Updates: UB-252

#### UB-252 (update, 2026-11-14). `temporary_radius_setup_errors` is removed

`SmTemporaryRadiusChange`'s constructors and scope helpers report a failed setup before the filleting body runs (#458), instead of ignoring it. The fix is now unconditional and the flag is gone. SolidCraft already ran `new_with_fixes` and `with_temporary_radius_change_with_fixes` with it on; `TemporaryRadiusChange::new` and `with_temporary_radius_change` passed `UpstreamFixes::default()` and so took the upstream path, and now report the setup error too (a failed setup used to build the change and run the body; it now returns the error and skips the body). Nothing in production calls either of them: their only caller is the goldens harness's unit test below. Removed: the field `UpstreamFixes::temporary_radius_setup_errors`, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it.

- **Results:** none change in the goldens or the kernel rig (measured on main a7468ec4f: every goldens unit with this and the other group A flags of the batch on, and the rig's table unchanged).
- **Tests:** the goldens harness's `temporary_radius_setup_error_never_enters_the_filleting_body` (every constructor and helper now returns the setup error).
