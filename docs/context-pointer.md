# Context Pointer

Context Pointer attaches a note to faces, edges or vertices in the model. A note can
refer to several selected targets. Each target displays the same numbered pin, so
reviewers can see exactly which geometry the note concerns.

## Add a note

1. Select one or more model faces, edges or vertices.
2. Right-click a selected target and choose **Add context pointer**.
3. Enter the note and choose **Open**, **Ready for review** or **Resolved**.
4. Click **Save note**, then use the normal **Save** command to save the design to disk.

For a note on several targets, right-click one of the targets already selected.
Right-clicking a different target starts a note for that target instead.

You can also open **Context Pointer** from **Inspect**, select geometry, and click
**Add context pointer** in its window. Pins appear while the window is open.
Closing the window does not delete the notes.

## Review and edit

Click a numbered pin or a note in the list to open it. Opening a saved note recalls
its saved view. Edit the text or status and click **Save note**. **Recall saved view**
returns to the view recorded when the note was created.

**Delete note** removes the note and its pins. Note edits and deletions use normal
undo and redo. Save the design after making changes you want to retain.

## When geometry changes

A note follows the recorded component occurrence when that occurrence moves rigidly.
If its geometry or reference no longer matches, the affected target becomes **stale**
and its pin is hidden. Context Pointer does not move the pin to nearby geometry.
The note and its other targets remain saved. Restoring the original geometry or
occurrence, including through undo, can make the original pin visible again.

## Storage and limits

Notes are stored inside the design's existing metadata extension point. No external
service or browser is required. The tool keeps its existing `document.review`
command and `com.tutorintelligence.review` metadata key so earlier saved notes remain
readable. Earlier face-only notes are upgraded when a note edit is saved; opening a
design alone does not rewrite its metadata.

Each design supports up to 128 notes. Each note can reference 1–32 geometry targets
and contain up to 4096 UTF-8 bytes of text. The full notebook must fit within the
metadata value limit of 64 KiB. Unsupported or malformed note data is retained,
and editing is disabled until the data can be read safely.

Programmatic `document.review` calls return the stored metadata without opening the
window. The UI saves changes through `document.metadata.set`.
