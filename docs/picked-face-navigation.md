# Picked-face navigation

A face selected in the viewport uses its persistent creating feature for **Edit Feature** and **Find in Timeline**. Later holes, blends and trims do not redirect the action to the feature that originally created the whole body.

**Find in Browser** finds the body, whether the menu was opened on a face, an edge or the body itself: it opens the body's folders, selects the body and scrolls its row into view.

**Find in Timeline** selects the feature and opens any collapsed history folders around it. It also opens the **Features** folder that the Browser keeps for each native component with history, in the selected body's actual occurrence, and scrolls the feature's row there into view. Started from a sketch, construction plane or feature row in the Browser, there is no body to tell which copy of a component was meant, so the Browser is left as it is. Sketches appear only in **Sketches**, and a sketch-only component has no Features folder. Double-clicking the row opens its editor. This leaves the timeline layout, active component and history marker unchanged until an edit is requested.

A face with an unavailable creator does not fall back to the body's base feature. Imported source identifiers do not acquire an unrelated editable native history.
