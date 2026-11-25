# Skills for agents that use SolidCraft

Skills for agents that model, edit and check designs through SolidCraft's [MCP server](../docs/mcp.md), and report what went wrong. Each folder is
one skill in the [Agent Skills](https://agentskills.io) format: a `SKILL.md` with a name and a
description that tell the agent when to load it, and an optional supporting reference file.

The modelling documents are built into SolidCraft: its MCP server's `skill_reference` tool returns them, so an
agent without file access (such as the one in SolidCraft's own chat) can still read a skill's reference.
`solidcraft-reporting` is checked against the same tools but not embedded.

The skills for working on SolidCraft's code (contributing, PRs, tests, layers, UI rules) are a different set, in [`.agents/skills/`](../.agents/skills/README.md). `AGENTS.md` routes to both; `cargo xtask skills` fails when its table, these folders or their READMEs disagree, or a file, `cargo xtask` command or skill they name is gone.

| Skill | Loads when the agent |
|---|---|
| [`solidcraft-modeling`](solidcraft-modeling/SKILL.md) | builds a new part (enclosure, bracket, plate, flange, …) |
| [`solidcraft-sheetmetal`](solidcraft-sheetmetal/SKILL.md) | builds or changes a sheet metal part: base, edge and contour flanges, segments, hem, fold, unfold, flat pattern, DXF and bend table, and turns a request in words into flange parameters |
| [`solidcraft-editing`](solidcraft-editing/SKILL.md) | changes, fixes or extends an existing design |
| [`solidcraft-checking`](solidcraft-checking/SKILL.md) | builds, changes or ports a design, next to the two above: parameter ranges, every allowed value, exact STEP comparison, regression tests, designs kept in git |
| [`solidcraft-mechanisms`](solidcraft-mechanisms/SKILL.md) | builds, drives or animates an assembly: components, joints, motion links, closed loops such as actuators, contact sets, motion studies |
| [`solidcraft-reporting`](solidcraft-reporting/SKILL.md) | hears that something went wrong (a crash, a failed command, a wrong shape or number, a hang): reproduces it in a fresh design, shrinks it to a minimal script and writes the bug report |

The modelling skills teach the agent to:
- plan the part as numbers
- build it in a few `batch` calls with parametric positions
- verify every stage with `measure` against values it computed itself
- skip the large replies it doesn't need, such as the unfiltered command catalog and full topology dumps

They also list the traps that otherwise cost failed calls. Examples are the sketch axes of the XZ plane, edge picks that snap to the nearest edge, and cuts that miss but still report success.

The modelling skills are written for SolidCraft 0.8 and later; `solidcraft-mechanisms` for `main` after 0.11.1 (the next release), which counts the degrees of freedom of closed loops; `solidcraft-reporting` for 0.18 and later; `solidcraft-sheetmetal` for the build with `segments` on an edge flange (the `feat/sheet-flange-segments` branch; `unfold_method` on a rule needs PR #9911).


## Install

The modelling skills, for Claude Code in all projects:

```sh
git clone --depth 1 https://github.com/tangency-labs/solidcraft
cp -r solidcraft/skills/solidcraft-{modeling,editing,checking,mechanisms,sheetmetal} ~/.claude/skills/
```

Windows (PowerShell):

```powershell
git clone --depth 1 https://github.com/tangency-labs/solidcraft
foreach ($s in 'modeling','editing','checking','mechanisms','sheetmetal') { Copy-Item -Recurse solidcraft\skills\solidcraft-$s $HOME\.claude\skills\ }
```

- **One project only:** copy the folders into that project's `.claude/skills/` instead.
- **Other agents:** for agents that read the Agent Skills format, copy the folders into the skills directory their documentation names.
- **Updating:** copy the folders again after you update SolidCraft.
- **Contributing:** the contributor skills need no install. Tell your assistant: "I want to contribute to SolidCraft. Read https://github.com/tangency-labs/solidcraft/blob/main/.agents/skills/solidcraft-contributing/SKILL.md and help me get started."

## Maintaining

- The worked example in `solidcraft-modeling/reference.md` runs in CI. `skill_reference_example_builds_as_documented` in `crates/mcp/src/tests.rs` builds it, checks the documented volume, changes a parameter and checks it again.
- The sketch-plane table is covered by `sketch_plane_axes_match_the_skill_reference` in the same file.
- The worked example in `solidcraft-mechanisms/reference.md` runs in CI too: `mechanisms_skill_example_moves_as_documented` builds it, drives the hinge and checks the actuator stroke, the linked panel and the stop against their analytic values.
- Its second worked example, the hinge as a linked subassembly, runs in CI as `mechanisms_skill_linked_subassembly_example_moves_as_documented`: it checks the bill of materials, makes one hinge flexible and then the other, drives each to its own angle and checks both against their hand-computed centres of mass.
- The worked example in `solidcraft-sheetmetal/reference.md` runs in CI: `sheetmetal_skill_example_builds_as_documented` builds the segmented flange, checks the volume against the hand formula and checks the bend table's lengths, angles and flat centre lines.
- `skills_name_only_real_tools_and_commands` checks that every tool and command the skills call exists.
- `solidcraft-checking/scripts/tests` checks that skill's helpers, its traps and conventions, and the facts behind the corrections in `solidcraft-modeling`: `PYTHONDONTWRITEBYTECODE=1 uv run --no-project --with pytest --with build123d pytest -p no:cacheprovider skills/solidcraft-checking/scripts/tests`. The tests run the `solidcraft-cli` on `PATH`, not the workspace build, and CI does not run them.
- When a command, a parameter name or a behaviour that these files describe changes, update them in the same PR.
- Facts that describe current limitations, such as edge picks without a distance limit, should go once the limitation is fixed.
