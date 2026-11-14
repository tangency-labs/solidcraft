# Native selection fields for command plugins

| | |
|---|---|
| Status | Draft proposal; no runtime support is added by this document |
| Tracks | [#16274](https://github.com/tangency-labs/solidcraft/issues/16274) |
| Builds on | [RFC 0002](rfcs/0002-plugins.md), [design forms #15100](https://github.com/tangency-labs/solidcraft/issues/15100), [RFC 0015](rfcs/0015-plugin-ui-workspaces.md) |

A plugin tool should let a person select geometry while its form stays open,
using the same controls as Extrude's Profiles and Chamfer's Edges. Preselection
can fill a field initially; it is not the only way to supply that field.

This proposal defines the first delivery: shared interactive selection for
command-entry forms. Reproducing the complete native Extrude workflow remains
the benchmark. The parity table below identifies the additional work rather
than calling a selection widget a complete tool API.

## Current boundary

`craft-plugin::validate_list_inputs` requires `selection`, `target`, `targets`,
`bodies`, and `body_ids` to be arrays. It does not define their items.
`plugin_menu::form` accepts only scalar fields; an array disables the whole form.
That refusal prevents the form from silently ignoring input constraints.

Native tools already have the interaction we need:

- `selection::SelInput` stores accepted hit kinds, multiplicity and ordered picks.
- `dialogs::input_rows_range` renders the field, count, clear action, item list,
  individual removal and hover highlighting.
- `viewport` routes hits and box selection to the active native `Dialog` input.
- `Dialog::candidate_on`, `pick` and command construction also contain
  operation-specific rules. Extrude can take planar faces and restrict profiles
  to one sketch. Chamfer expands face selections and optionally tangent chains.

Plugin forms have a separate `EntryDialog` and do not participate in that routing.
Allowing arrays in the parser, or drawing the same button, is insufficient.

## Declaration proposed for review

Use a SolidCraft-owned annotation on an existing array property. The property
key binds the field to its command argument. JSON Schema remains the source of
requiredness, item constraints, bounds, titles and descriptions.

The following is a proposed declaration, not a manifest that works today:

```json
{
  "type": "object",
  "required": ["profiles"],
  "properties": {
    "profiles": {
      "type": "array",
      "title": "Profiles",
      "minItems": 1,
      "items": {
        "type": "object",
        "required": ["type", "sketch", "index"],
        "properties": {
          "type": {"const": "profile"},
          "sketch": {"type": "integer", "minimum": 0},
          "index": {"type": "integer", "minimum": 0}
        },
        "additionalProperties": false
      },
      "x-solidcraft-selection": {
        "version": 1,
        "accept": ["profile"]
      }
    }
  }
}
```

`accept` names hit categories, not raw Rust bit values. The first supported set
should cover profiles, edges, faces, bodies, sketch points and sketch curves.
Planarity, circle/arc filtering and operation-specific expansion are semantic
policies, not guessed from a field name. Their declaration must be agreed before
the relevant native-parity example is accepted.

`maxItems: 1` selects single-pick replacement behavior; otherwise picks toggle
in a multi-select field. The host enforces `minItems` and `maxItems`, retaining
pick order and the native definition of duplicate identity. No second `multi`,
`required`, label or default is introduced in the annotation.

An optional, empty selection field is omitted; a required field is emitted as
an array, with `minItems` deciding whether empty is valid. Thus `required` alone
does not mean at least one pick. Clearing an optional field restores omission.
Selection defaults are not accepted in this first contract: preselection is an
explicit UI initialization rule, not a default reference to another document.

The host must validate both the annotation and the item schema. A recognizable
annotation does not authorize skipping constraints. Unsupported combinations,
unknown versions or categories produce a field-specific explanation and keep
Run disabled. Unannotated arrays retain their existing behavior.

Factor the supported item schemas alongside the engine's canonical selection
parser, with coverage against that parser. The current schema is hand-written;
there is no derived `Sel` schema to reuse unchanged. The expanded profile schema
above illustrates the proposed strict subset. Do not maintain a second list of
selection identities in the plugin UI. This does not imply general support for
arbitrary nested forms or schema combinators.

### One shared form model

The annotation is an adapter into the selection field type shared with #15100
and RFC 0015. Native declarations, command-plugin schemas and future WIT forms
must normalize into one selection-field description and controller. They do not
get independent implementations of pick rules, validation or presentation.

The exact annotation spelling and its version negotiation are review decisions.
A shared `x-ui` annotation with `kind: "selection"` and named host roles is also
an option. Per-field roles and policies driven by other fields must use one host
descriptor shared with #15100 and RFC 0015, without a plugin-specific policy enum.
`craft-plugin` continues to carry app-specific schemas as data. It must not
import SolidCraft geometry or egui types; SolidCraft's adapter interprets this
annotation and exposes it through `plugin.list` and `engine.describe`.

## Values and reference lifetime

Each populated field supplies an ordered array of explicit selection identities.
Reuse the engine's tagged `Sel` identities where they apply. Require complete
identities and a pure validator that resolves them against exact geometry.
Factor that validator with native selection callers where possible.
The existing `select.set` handler mutates selection and does not fully validate
every exact identity; calling it is not validation of a plugin draft. Its partial
index/point resolution is not this contract. Do not pass all of
`selection.get(detail: true)` as an input format:
its optional geometry details are display/query data and have a detail limit.

Normalize sketch points and curves to identities with an explicit source sketch,
using the existing external-sketch variants. A plugin must not need the active
sketch to interpret a point id. A profile includes its sketch and region index;
a body uses its world name. Face and edge identities retain the body, index and
picked point used by the native selection code. Preserve existing assembly
identity through world-qualified body names and `Occurrence.path` where that
variant applies. Do not add an `occurrence_path` member to face/edge identities;
that member belongs to enriched query output today.

These are references for one live command invocation, not a new persistent
design format. Raw topology indices are not stable across arbitrary model edits.
Retain the owning document and a model identity with the field's picks. Reject
stale references when the model changes; never silently pick a nearby substitute.
Selection and camera changes alone must not invalidate picks. Recheck the owner
and model at dispatch, before the plugin runs, and retain the existing worker's
guard before adopting a computed result.

The same normalization and validation is available to programmatic calls. UI
calls capture the model guard with their draft; programmatic calls validate
explicit inputs against the executing session. The form adapter never opens a
picker for programmatic calls or fills declared fields from global selection.
Plugins may still explicitly call `selection.get` as part of their own behavior.

Body-name arrays remain distinct from geometry-item arrays. This delivery does
not reinterpret existing string arrays based on the names `bodies` or `targets`.
Selected geometry is input data, not an extra grant of write permission.

## Shared native interaction

Extract the reusable input/controller behavior from native `Dialog` and use it
from both forms. Keep a single active viewport-pick owner. Opening a plugin form
must follow the existing active-tool transition policy, so it cannot steal picks
from another uncommitted editor or silently cancel that editor.

The shared controller owns active field, ordered picks, selected-list hover,
pick focus/refusal and document binding. Native static labels and plugin-owned
labels must use an owned-or-borrowed representation; do not leak manifest strings
to satisfy `SelInput`'s current `&'static str` fields.

Rendering uses the existing selection control and native row behavior. Viewport
hover, click, Select Other, box selection and highlight paths ask the active
controller for its input instead of adding a parallel plugin hit-testing path.
Camera input continues through `navigation::handle`.

Preselection fills only the initial active field with compatible items. Do not
copy one global selection into every field. A tool with Body to modify and
Position keeps those drafts independent. Subsequent viewport picks update only
the active field, and clear/remove actions affect only that field.

Picking and editing fields execute no guest code. Run serializes the validated
draft and uses the existing approved `plugin.run` path. Cancel releases the pick
owner and transient highlights without changing the design or adding undo steps.
The established document-switch, cancellation and failed-run behavior remains
part of that path.

Operation-specific rules must be extracted with their native callers. Share
Extrude's profile/face mapping and Chamfer's face-to-edge and tangent-chain
behavior where exposed; do not duplicate their geometry logic in a plugin picker.
The public policy bindings needed for these rules are prerequisites for the
native selection parity delivery. Agree on them during interface review,
including rules driven by another field such as thin mode or chain. An accepted
hit-category list alone does not provide native tool semantics.

## Delivery and acceptance

| Delivery | Required evidence |
|---|---|
| Contract and shared controller | Schema/identity validation, independent fields, document ownership, native interaction regression tests |
| Command-plugin integration | Open without preselection; activate, hover, pick, toggle, clear and remove; pass explicit fields to Run; cancel and undo |
| Native selection parity | External example plugins wrapping Extrude and Chamfer; compare profiles/planar faces, edges/faces, applicable expansion and command results with native tools |
| Full tool parity follow-ups | Live previews, unit expressions, conditional controls, manipulators and edit/reopen behavior |

Use a two-field placement example as well: choosing Position cannot change Body
to modify. Cover sketch-local ids that repeat in two sketches, assembly context,
invalid counts, a model changed while the form is open, and a queued run whose
owner has changed. A rejected field must identify the problem before any guest
code runs. Check plugin input values and resulting geometry, not just the count
shown on the button.

Keep tests in the existing selection, dialog, plugin-menu and engine plugin test
modules. Native tools must exercise the extracted controller too. Capture the
actual interaction at 1280×720 and a small window for the implementation PR.
Example plugins live outside the app repository; small fixtures remain in its
existing test infrastructure.

Preview is a separate execution contract. The current native preview path reads
`app.dialog` and evaluates command plans on scratch sessions. A plugin preview
must explicitly prevent external side effects and reject obsolete results; do
not repeatedly invoke an ordinary effectful plugin entry as the user picks.
Persistent generator editing also needs its own saved-reference and lifecycle
contract. These are tracked follow-ups, not capabilities implied by selection.

## Coordination and review decisions

- #15100 and RFC 0015: agree on the annotation-to-shared-field mapping and version
  handling before implementation freezes an author-facing format.
- [#15609](https://github.com/tangency-labs/solidcraft/pull/15609): preserve its
  plugin inventory, grants and entry placement. It overlaps `plugin_menu.rs` but
  does not supply interactive selection fields.
- [#11314](https://github.com/tangency-labs/solidcraft/issues/11314): keep the
  selection controller with its editor owner, independently of panel placement.
  This work does not implement docking or native-window movement.
- [#1221](https://github.com/tangency-labs/solidcraft/issues/1221): retain the
  app-agnostic runtime, command surface and review of interfaces before later
  implementation. This design does not add a parallel plugin UI capability.

The implementation review must settle: the annotation/version shape; the exact
supported identity subset and generated schema; the shared model guard; and the
public bindings for native operation-specific selection policies. Each decision
needs one implementation used by the UI and programmatic validation.

**Compatibility:** this document changes no runtime behavior. The proposed
annotation is experimental. Incompatible contract revisions must be versioned
and documented, without compatibility aliases or silent fallback to global
selection. An unsupported host must report the required version.
