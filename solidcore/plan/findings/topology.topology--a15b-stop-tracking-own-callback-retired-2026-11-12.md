# StopTracking stops only its own callback: 18 golden rows move (2026-11-12)

- Flag retired (tangency-labs/solidcraft#14246, batch A15b): `stop_tracking_checks_own_callback`
  (WR-17). WR-17 has an addendum of 2026-11-12.
- The goldens runners of `topology.topology` and `topology.brep_p5` build their trackers with
  `TrackTopologyChanges::new`, which never read the flag, so the batch survey (flag on in the
  kernel) saw no row move. With the flag gone, `new` takes the corrected path:
  `topology.topology` changed=15 and `topology.brep_p5` changed=3; added, removed and broken 0.
- Old `StopTracking` cleared the slot when the tracker has a B-rep and found the slot empty when
  it last started; new clears it only when the installed callback is the tracker's own
  (`IsTracking`). They differ only at a stop where the slot holds a callback that is not the
  tracker's (old clears it, new keeps it), or holds the tracker's own callback after a second
  `StartTracking` (old keeps it, new clears it).
- Which case takes which branch: the runners ran once more on the fix with a counter at every
  stop (not committed) that prints the case when the two rules disagree on an occupied slot.
  It printed 22 cases: the 18 moved ones and 4 more of `topology.topology` (`gen0182`,
  `gen0183`, `gen0493`, `gen0559`) whose scripts install or clear the slot again before
  anything reads it, so their outputs are equal (the `--digest-debug` outputs before and after
  agree bit for bit). Every moved row has a hit.

## `topology.topology`: a foreign callback stays installed (11 rows)

Each script stops a tracker that found the slot empty while a foreign callback
(`SYS_FOREIGN`) is installed. Old clears it; new keeps it, as WR-17 asks.

| case | where the output first differs | old | new |
|---|---|---|---|
| `track_stop_clears_foreign` | next `NOTIFY`, events recorded | 1 | 2 |
| `gen0061`, `gen0188` | next `NOTIFY`, events recorded | 0 | 1 |
| `gen0172`, `gen0426`, `gen0524` | next `NOTIFY`, events recorded | 1 | 2 |
| `gen0018`, `gen0032`, `gen0294`, `gen0431`, `gen0462` | final state, foreign callback installed | 0 | 1 |

Why right: the foreign callback was installed after the tracker started, so the tracker never
owned it; the extra event is the foreign callback still hearing `Notify`.

## `topology.topology`: another tracker keeps its callback (1 row)

`gen0562`: tracker 0 is made with `start_now` false and never started, tracker 1 starts and
installs its callback, then tracker 0 stops. Old clears tracker 1's callback; new keeps it,
so the later `IsTracking` of tracker 1 (op 26) is 1, not 0. Why right: this is WR-17's
trigger with a tracker in place of the foreign callback.

## `topology.topology`: a tracker started twice stops (3 rows)

The second `StartTracking` finds the tracker's own callback in the slot and remembers the slot
as occupied, so old `StopTracking` leaves the callback installed for good; destroying the
tracker then would leave a dangling callback (upstream; the runner skips that destroy,
`ST_SKIP`, see `topology.topology--green-no-disagreements.md`). New stops it.

| case | where the output first differs | old | new |
|---|---|---|---|
| `track_restart_keeps_callback` | `IsTracking` after the stop (op 4) | 1 | 0 |
| `gen0304` | `IsTracking` of a tracker started after it (op 27) | 0 | 1 |
| `gen0519` | destroying the tracker (op 14), status | -2 (skipped) | 0 |

Why right: a stopped tracker is not tracking, and the next tracker can install its callback.

## `topology.brep_p5`: `tracker-lifetime-k0`, `-k1`, `-k2` (3 rows)

Tracker 1 starts on an empty context, receives two `PostEdit` events for a vertex, then
tracker 2's callback replaces it and tracker 1 stops with its B-rep (`StopTracking` with
`RecordDependencies`). Old clears tracker 2's callback and expands tracker 1's lists to the
vertex's edges and faces; new keeps tracker 2's callback and records no dependencies.

| case | rm and add lists after the stop, old | new | slot empty after the stop, old | new |
|---|---|---|---|---|
| `tracker-lifetime-k0` | vertex, 1 edge | vertex | 1 | 0 |
| `tracker-lifetime-k1` | vertex, 3 edges | vertex | 1 | 0 |
| `tracker-lifetime-k2` | vertex, 2 edges, 1 face | vertex | 1 | 0 |

Why right: tracker 1 was no longer the installed callback, so it was not tracking, and its
stop records nothing. Upstream's `StopTracking` (`SmTrackTopologyChanges.cpp:512`) records
dependencies only in the branch that clears the callback, so a nested tracker that found the
slot occupied records none either; the rule is now the same for every tracker that is not
tracking.

## Commands

- `goldens --unit topology.topology --unit topology.brep_p5 --goldens` at the fix: the 18 rows
  above; at the test commit (main's code) both units pass.
- `--digest-dir D --digest-debug` at both commits for the per-case outputs; `--bless` at the
  fix, then the same `--goldens` run passes.
