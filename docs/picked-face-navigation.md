# Picked-face navigation

A face selected in the viewport uses its persistent creating feature for **Edit Feature** and **Find in Timeline**. Later holes, blends and trims do not redirect the action to the feature that originally created the whole body.

**Find in Browser** finds the body, whether the menu was opened on a face, an edge or the body itself: it opens the body's folders, selects the body and scrolls its row into view.

With **Preferences › Interface › Browser rows start closed** on (off by default), placements and folders start closed and Find in Browser opens only the rows down to the body: the placements that hold it, its Bodies folder and any group around it. The next Find in Browser closes the rows this one opened, except rows opened or closed by hand in between, and the History's Find in Browser does the same for the feature's first body. A pick in the 3D view then leaves the Browser as it is, and only the body Find in Browser found has the rows above it marked, until the selection changes.

**Find in Timeline** selects the feature and opens any collapsed history folders around it. It also opens the **Features** folder that the Browser keeps for each native component with history, in the selected body's actual occurrence, and scrolls the feature's row there into view. Started from a sketch, construction plane or feature row in the Browser, there is no body to tell which copy of a component was meant, so the Browser is left as it is. Sketches appear only in **Sketches**, and a sketch-only component has no Features folder. Double-clicking the row opens its editor. This leaves the timeline layout, active component and history marker unchanged until an edit is requested.

A face with an unavailable creator does not fall back to the body's base feature. Imported source identifiers do not acquire an unrelated editable native history.
