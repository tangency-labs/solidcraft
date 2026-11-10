# 2D Timeline

The 2D Timeline is an optional view of the timeline. It shows each feature in the row of the
component it changes. Columns are timeline steps, from left to right in the order SolidCraft
computes them. Rows are the root of the design and its components. In an assembly with many
components, you can see which component owns each step and which steps depend on each other.

![The SolidCraft window with the 2D Timeline enabled](../generated/timeline-2d-overview.png)

## Turning on the 2D Timeline

You can turn on the 2D Timeline in two ways:

1. **Preferences**: Open **Preferences › Layout** and select **2D Timeline (Bottom only)**
   (`prefs.timeline_2d`). The 2D Timeline replaces the timeline at the bottom of the window only.
   The other places for the history (Left, Under browser, In browser) keep the 1D timeline.
2. **Control channel or agent tools**: Send the preference through the control channel. The first
   request on a connection also needs a `"token"` parameter:
   ```json
   {"id": 1, "method": "ui.prefs", "params": {"timeline_2d": true}}
   ```

The 2D Timeline then takes the place of the timeline at the bottom of the window. You can change
its height with the rail along its top edge.

### Changing the height

Drag the rail at the top of the 2D Timeline (`timeline:top_rail`) to change its height.
Double-click the rail to go back to the default height.

- **One row**: Drag the rail down as far as it goes. The 2D Timeline then shows one component row
  (116 px) and a vertical scrollbar on the right. The 3D view keeps as much height as possible.

![2D Timeline vertically resized to a single visible component track](../generated/timeline-2d-resize-1row.png)

- **All rows**: Drag the rail up until every component row shows, with no empty space under the
  last row. The 2D Timeline grows to at most three quarters of the window height. When the rows
  still don't fit, drag an empty area of the rows to pan them (grab and pan). You can also drag with
  the middle mouse button anywhere on the timeline, turn the mouse wheel, or use the vertical
  scrollbar on the right.

![2D Timeline vertically resized to fit all five component tracks](../generated/timeline-2d-overview.png)

### Modeling and Assembly tracks

When the design has joints, the 2D Timeline has two parts side by side: **Modeling Features**
(blue) and **Assembly & Joints** (purple). A vertical splitter separates them.

- **Both tracks**: This is the default. Drag the grip in the middle of the splitter to change how
  much width each track gets.

![Horizontally split track view showing Modeling Features and Assembly & Joints side-by-side](../generated/timeline-2d-corridor-selection.png)

- **Modeling track only**: Click the lower, purple button on the splitter (`>|`). The Assembly track
  closes into a narrow tab at the right, and the Modeling track takes the full width. The button
  then shows `◫`; click it to show both tracks again.

![Fully expanded Modeling Features track view with Assembly contracted into the right dock tab](../generated/timeline-2d-view-modeling-full.png)

- **Assembly track only**: Click the upper, blue button on the splitter (`|<`). The Modeling track
  closes into a narrow tab at the left, and the Assembly track takes the full width. The button
  then shows `◫`; click it to show both tracks again.

![Fully expanded Assembly & Joints track view with Modeling contracted into the left dock tab](../generated/timeline-2d-view-assembly-full.png)

- **Joints**: Each joint shows a chip in the row of each component it connects, with its type
  under its step number. Its right-click menu has the joint's actions. When you select a joint, the
  2D Timeline highlights the other joints that share a component row with it. This shows shared
  rows only; it does not follow how the joints move each other.

![Split Assembly & Joints track with active joint selection and kinematic dependencies](../generated/timeline-2d-assembly.png)

## Rows and columns

- **Rows**: The top row is the root of the design. Each component follows in the order of the
  browser, active or not. Each row starts with a round **activation** indicator (`○` inactive, `⊙`
  active), as in the browser. While **Activate components from timeline**
  (`timeline_2d_component_activation`) is on, which is the default, a click on a row's label makes
  that component active (`component.activate`). While it is off, the indicator only shows which
  component is active.
- **Active row**: The row of the active component has a light accent background. You can see at a
  glance which component a new feature goes into.
- **Columns**: Each column is one timeline step, in the order SolidCraft computes the steps. After a
  reorder, this can differ from the order in which the features were made. An empty cell shows a
  small dot (`·`), so you can follow a column across all rows.
- **Features that change several components**: A feature that changes bodies in several components,
  such as a `solid.combine` join or cut, or a joint, shows a chip in each of those rows. A dotted
  vertical line connects the chips.

## The header

The header has two rows:

1. **Step row**:
   - Shows the step numbers (`01`, `02`, `03`, ...), tick marks and status marks.
   - Holds the **rollback marker**, a round pin. Drag the pin, or click a step number, to roll the
     design back to just before that step. A click on a feature chip selects the feature and does
     not roll back. In the Assembly & Joints track, a click on a joint step moves the assembly
     marker. It does not roll back the design or change the joints.
2. **Group row**:
   - Shows a bracket and a name for each feature group.
   - Shows the **movable corridor** (`[ ... ]`) while a feature is selected: the steps the feature
     can move to.

## Dependencies and reordering

A feature depends on the features it uses. An extrusion uses its sketch, and a fillet uses the body
it rounds, so it must come after them. When you select a feature, the 2D Timeline shows these
relations:

- **Selection**: A selected feature chip has a white outline.
- **Prerequisites**: The steps the selected feature depends on, directly or through other steps,
  have an amber outline. The feature cannot move before the last of them.
- **Dependents**: The steps that depend on the selected feature have a blue outline. The feature
  cannot move after the first of them.
- **Movable corridor (`[ ... ]`)**: Green brackets in the group row and across the grid show the
  steps the selected feature can move to without breaking the design.
- **Reordering**: Drag a feature chip sideways within its corridor. The features between its old and
  new step move one step the other way. A drop on the other side of the rollback marker also makes
  the feature active or inactive to match where it lands, so its geometry can disappear or come
  back. You cannot drag a feature into another component's row.
- **Corridor at the rollback marker**: When a corridor bracket and the rollback marker are at the
  same step boundary, the bracket moves 3 px into the corridor. Both stay visible, in the Modeling
  and the Assembly track. Only the marker line reacts to the pointer.

![Topological dependency highlighting and Movable Corridor selection](../generated/timeline-2d-corridor-selection.png)

![Coincident rollback marker with Smart Inset inward bracket offset](../generated/timeline-2d-corridor-smart-inset.png)

## Groups and folding

You can put features in named groups:

1. **Select several features**:
   - `Shift + Click` selects a range of features.
   - `Cmd + Click` (macOS) or `Ctrl + Click` (Windows, Linux) adds a feature to the selection or
     takes it out.
2. **Make a group**: Right-click a selection of neighbouring features and choose **Group
   Selection**.
3. **Fold and open**:
   - An open group shows a bracket with its name over its steps in the group row. When the name is
     too long for the bracket, it ends in an ellipsis (`...`). Hover over it to see the full name
     and the number of features.
   - Click the group's bracket once to fold the group into a single **group card**. Don't
     double-click it: the second click lands on the folded card and opens the group again.
   - A group card has a header, the number of features in it (for example `4`), an arrow to open
     it, and a background that spans all the rows its features are in.
4. **Group menu**: Right-click a group card for **Open/Close Group**, **Rename Group**, **Delete Group
   but expand content** (the features stay in the timeline) and **Delete Group and content**.

![Open Feature Groups with Tier 2 banners spanning member columns](../generated/timeline-2d-group.png)

![Folded Virtual Group Cards spanning tracks with integrated header caps and member count badges](../generated/timeline-2d-dock-closeup.png)

## Suppressing features

A suppressed feature stays in the timeline, but SolidCraft does not compute it.

- **How it looks**: A suppressed chip has a dashed outline and a crossed-out eye
  (`assets/icons/ui/design/svg/flat/suppress.svg`) at 50% opacity over its icon, so you can still
  see which feature it is.
- **Dependents follow**: When you suppress a feature, the features that depend on it are suppressed
  too, because they need its geometry.
- **Unsuppressing**: When you unsuppress a feature, its suppressed prerequisites come back too. What
  happens to its dependents depends on **Preferences › Layout › 2D Timeline › DAG unsuppress**:
  - **Restore State** (default): Only the dependents that its suppression suppressed come back. A
    dependent you had suppressed on its own in the 2D Timeline before stays suppressed. The 2D
    Timeline remembers this only while the design is open, and only for suppressions made in the
    2D Timeline. Undo and redo of such a suppression can also make it forget.

    The prerequisites that come back can bring back other features too. Say B and C both depend on
    A, and suppressing A suppressed all three. Unsuppressing B brings back A, its prerequisite.
    C then comes back with A, although C does not depend on B.
  - **All Features**: All dependents come back, also those you had suppressed on their own.

## Rollback and inactive steps

The blue rollback marker divides the timeline:

- SolidCraft computes the unsuppressed steps to the left of the marker. It does not compute the
  suppressed ones.
- Steps to the right of the marker are inactive. Their chips are grey, at full opacity and with the
  same brightness, so the icons and labels stay readable and look different from active steps.

![Timeline dock with folded virtual group cards, combine connector, and inactive steps](../generated/timeline-2d-dock-closeup.png)
