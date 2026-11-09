# Original starter appearance recipes

The existing `appearance.library` command lists 40 original illustrative recipes:
34 opaque PBR appearances and six glass recipes that are explicitly unavailable
through scalar appearance assignment. The 20 legacy scalar appearances are
unchanged. The separate six-finish proposal is not included in this constituent.

Use a stable `starter.*` ID with `appearance.assign` to request a recipe. Opaque
presets become owned ordinary Look values; no external file, preset identifier or
new document field is needed to reopen them. Color overrides retain an explicitly
selected recipe response; `pbr: null` deliberately selects scalar shading.

Glass recipes require a genuine certified refraction path. Color, PBR and opacity
overrides do not bypass their refusal. They are not approximated by transparency.

A copied look's `name` is inert data, distinct from the `appearance` selector.
Catalog application uses that inert label and replaces the complete PBR value;
a label matching an unavailable recipe cannot accidentally resolve that recipe.

Recipes are MIT OR Apache-2.0 original optical authoring choices, not measured
engineering grades. Visual edits do not change density, mass or geometry. Wood,
solar and regolith entries are labelled uniform previews; their future texture
metadata does not install textures, grain, displacement or electrical properties.

This change uses existing appearance commands and persistence. It adds no image
pack, runtime network access, new renderer, UI layout or manual format version.

The Appearance dialog omits unavailable recipes from selectable swatches. Its
Apply/OK/drop paths pass the complete owned look and an inert name, so editing a
label that matches a recipe cannot silently re-resolve or change its response.
