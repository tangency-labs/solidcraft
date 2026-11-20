Updates: WR-604 (`WR-604--fillet-geom-trim-drops-failed-statuses.md`)

#### WR-604 (update, 2026-11-17). `fillet_trim_status` is removed

`SmFilletGeom::Trim` (`crates/solidcore/src/blend/fillet_geom/p1.rs`) now always returns the first failed rail, other-rail, center-line, interval or surface trim, as the fork does (tangency-labs/smlib 9b7cd48, `SER`). Propagation is the only path, and the flag is gone: the field, the ignore branches, SolidCraft's preset line, its entry in `fixes.json` and every test setting of it are removed (tangency-labs/solidcraft#14246, group G1). The successful prefix edits and the parameter snapping are kept, as in either mode before.

The goldens rows this moves are those of `blend.fillet_geom_p1`, counted with `fillet_statuses_pass_on` in the WR-661 addendum of the same date (the two flags guarded the same `Trim` sites and are retired in one commit). Test: `goldens` `blend_fillet_geom_p1::tests::wr661_moved_trims_ask_for_a_trim_that_does_not_exist`.
