# Pasting a linked design

Save the source design without pending changes, select its root in the Browser and copy it. Switch to the destination document and paste. The result is an ungrounded occurrence linked to the saved source; repeated pastes reuse that component definition. The source design is not modified.

The copied design's origin is placed on the solid surface under the viewport pointer. Over empty space, the view ray is intersected with the active parent component's XY plane. If there is no viewport pointer, or that ray is parallel, behind the camera or invalid, placement uses the parent origin. World points are converted into the parent's coordinates, including a rotated or nested parent. Keyboard and viewport context-menu paste use this same rule.

Initial placement is part of the paste's single undo step. Move/Copy numeric fields adjust pending distances and angles after insertion. Capture All Positions and Revert All Positions retain their existing pending-position behavior. A document's Move/Copy context stays with its own tab.

The pre-existing occurrence arrow input repair is proposed separately in #3015 (issue #3014). Pointer placement does not include or duplicate that shared viewport patch. Combined trials include both changes; this clipboard PR remains independently scoped.
