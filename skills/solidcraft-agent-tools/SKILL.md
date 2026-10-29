---
name: solidcraft-agent-tools
description: "Use when running SolidCraft to see a change working, driving it through the control channel or CLI, or adding or changing an MCP tool, control method or bundled agent skill."
---

# Running the app, the control channel and MCP tools

## Run it and look at it
- `cargo run --release -p solidcraft -- --sample --control PORT` starts the sample design with the control channel. Pick a free port. With no display, run it under `Xvfb :99` with `DISPLAY=:99`.
- Talk JSON lines to `127.0.0.1:PORT` (`docs/control-protocol.md`). The first request on a connection carries the launch token as `"token"`: read it from `~/.local/share/solidcraft/control/PORT.token`, or start the app with `SOLIDCRAFT_CONTROL_TOKEN=$(openssl rand -hex 32)` and send that. Never paste the token into a PR or issue.
  - `{"id":1,"method":"engine.execute","params":{"command":"solid.extrude","params":{"distance":20}}}`
  - `{"id":2,"method":"ui.start","params":{"command":"sketch.create"}}`, then `ui.click {x, y}`
  - `{"id":3,"method":"ui.screenshot","params":{"path":"/tmp/shot.png"}}`, then read the PNG
- For UI work, look at the result. Without a window use `ui.render` or `solidcraft-cli snapshot`.
- Headless: `solidcraft-cli run script.json --out part.step`, `solidcraft-cli eval design.sc`, `solidcraft-cli snapshot script.json --out shot.png`, `solidcraft-cli commands`.
- Build with `CARGO_TARGET_DIR=<repo>/target`.

## MCP tools
The server is `solidcraft-cli mcp` (`crates/mcp/src/`, `docs/mcp.md`). Tools are defined in `crates/mcp/src/tools.rs`; keep `docs/mcp.md` in step, and keep tool annotations (read-only, destructive) honest.
- Anything the user can do is a command; add a command, not a new tool, unless the tool is about the session itself (open, save, screenshot, batch).
- Files: tools reach only the allowed folders (the design's folder, the recovery folder, the scratch folder, `--allow-path`). Do not widen this casually; paths are resolved before comparing, so `..` and links leaving the allowed set are refused, and on Windows UNC and device paths are refused.
- Replies stay small: `verbose: false`, no unfiltered command catalogue or topology dumps.
- The control channel is a local-only interface with a token. A new method needs an entry in `docs/control-protocol.md`; long work follows `skills/solidcraft-performance/SKILL.md`.

## Bundled agent skills
The modelling skills (`skills/solidcraft-modeling`, `-editing`, `-checking`, `-mechanisms`) are built into the MCP server for its `skill_reference` tool (`crates/mcp/src/skill_reference.rs`). The contributor skills are not.
- When a command, parameter name or behaviour those skills describe changes, update them in the same PR. `skills_name_only_real_tools_and_commands` in `crates/mcp/src/tests.rs` fails when a skill names a tool or command that is gone.
- Adding a bundled file means adding it to `skill_reference.rs` and to that test.
- `skills/README.md` explains the layout and the other tests; `cargo xtask skills` checks that AGENTS.md and every SKILL.md cite real paths, commands and skills.
