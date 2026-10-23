# Draft reply to bherbruck on PR #5132

Not posted. For Simon to send.

---

Thanks — the size concern is fair. Keeping a snapshot of every save should not be the default, and a file should not pick up that extra bulk unless someone asks for it.

Revision history is now opt-in and **off by default**. A save still records the revision count. It stores a restorable model only when the preference is on (Preferences → General → Autosave and recovery).

Two limits are applied whenever snapshots are pruned:

- **Maximum size**, configurable, **default 128 MB** (the ceiling you cited). The setting runs from 1 MB to 128 MB.
- **Maximum revisions**, configurable, **default 16** (the newest 16 keep a snapshot). The setting runs from 1 to 64.

Unchanged STEP, mesh and canvas bytes are stored once, so the size cap counts payload that is actually new. Plain JSON counts the encoded snapshot. Turning the preference off stops new snapshots; a later save still prunes what is already in the file down to those limits.

This is rebased onto current `main`.
