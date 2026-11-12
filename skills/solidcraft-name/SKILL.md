---
name: solidcraft-name
description: Use when naming a crate, binary, file format, environment variable, setting or deployment, when a new idea needs a glossary term, or when units or the up axis appear in code, docs or messages.
---

# The name, terms and units

These rules moved here from `solidcraft-ground-rules` when that skill reached its
size budget. They are equally binding.

## The name
- SolidCraft everywhere: crates `solidcraft-*`, binaries `solidcraft` and `solidcraft-cli`, `.sc` designs (`.solidcraft` still opens), the `solidcraft/N` format tag, `SOLIDCRAFT_*` environment variables, settings and recovery folders, the MCP server name, docs, CI and packaging.
- The old name survives only in the compatibility shims listed in `docs/compatibility.md`. Do not add new uses of it.
- Public docs, web and Flatpak deployments use project Pages `/solidcraft/`. Do not add permanent old-name Pages aliases.
- Labels, parameter docs, messages and docs use the terms in the glossary (`docs/book/src/reference/glossary.md`): one term for each idea. A change that adds a new idea adds its term there.
- Units are millimetres and radians internally; Z is up.
