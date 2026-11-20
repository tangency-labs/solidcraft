# Skills for working on SolidCraft

Skills for contributors and their coding assistants: claiming work, writing and watching a pull request, and the rules for each area of the code. Each folder is one skill in the [Agent Skills](https://agentskills.io) format. `AGENTS.md` routes to them.

Coding assistants that read project skills from `.agents/skills/` find them here. `.claude/skills` is a symlink to this folder. On Windows, enable symlinks (Developer Mode, then `git config core.symlinks true`) or point the tool at this folder.

These are not built into the app. The skills for agents that model through SolidCraft's MCP server are in [`skills/`](../../skills/README.md).

| Skill | Loads when the agent |
|---|---|
| [`solidcraft-contributing`](solidcraft-contributing/SKILL.md) | helps someone set up a fork, choose and claim an issue, and submit a contribution for review; the rules every PR follows |
| [`solidcraft-file-pr`](solidcraft-file-pr/SKILL.md) | opens or updates a pull request: the title, the body, the template lines the checks read, screenshots for UI changes |
| [`solidcraft-babysit-pr`](solidcraft-babysit-pr/SKILL.md) | has pushed a pull request: the local check, waiting for CI and the review bot, reading results, fixing real findings |
| [`solidcraft-full-stop`](solidcraft-full-stop/SKILL.md) | is asked to take a change all the way to a pull request ready for review, with nobody watching; never merges |
| [`solidcraft-takeover`](solidcraft-takeover/SKILL.md) | finishes someone else's pull request or stalled claim and keeps their credit |
| [`solidcraft-modernize`](solidcraft-modernize/SKILL.md) | brings an old pull request up to date and decides whether to keep, rework or close it |
| [`solidcraft-ground-rules`](solidcraft-ground-rules/SKILL.md) | adds a dependency, asset or wording: independence, licences, attribution, the product name |
| [`solidcraft-crash-safety`](solidcraft-crash-safety/SKILL.md) | writes non-test Rust, handles hostile input or investigates a crash report |
| [`solidcraft-commands`](solidcraft-commands/SKILL.md) | adds a command or decides which crate code belongs in |
| [`solidcraft-layers`](solidcraft-layers/SKILL.md) | decides whether code goes in SolidCraft or solidcore (kernel, ops), or works on kernel or geometry code |
| [`solidcraft-ui`](solidcraft-ui/SKILL.md) | adds a dialog, panel, camera input, icon or colour |
| [`solidcraft-shared-controls`](solidcraft-shared-controls/SKILL.md) | changes selection, picking, keys or a tool's start and finish, or wants a new option: shared controls, and options in the tool's panel |
| [`solidcraft-ui-text`](solidcraft-ui-text/SKILL.md) | adds a label, message or translation |
| [`solidcraft-file-formats`](solidcraft-file-formats/SKILL.md) | changes what a design saves, or touches import and export |
| [`solidcraft-preferences`](solidcraft-preferences/SKILL.md) | adds a setting or changes a default |
| [`solidcraft-kernel-rules`](solidcraft-kernel-rules/SKILL.md) | changes geometry, solidcore or a test expectation: one implementation, no fix flags, no new refusals of valid input, test changes with a derivation |
| [`solidcraft-repro-first`](solidcraft-repro-first/SKILL.md) | fixes a bug seen only on some machines (an OS, graphics driver, display scaling, the updater): evidence from a released build, a fix scoped to that condition |
| [`solidcraft-tests`](solidcraft-tests/SKILL.md) | places or runs tests |
| [`solidcraft-performance`](solidcraft-performance/SKILL.md) | works on slow code, the window thread or the wasm build |
| [`solidcraft-packaging`](solidcraft-packaging/SKILL.md) | prepares a release or changes packages |
| [`solidcraft-agent-tools`](solidcraft-agent-tools/SKILL.md) | runs the app, drives the control channel or changes MCP tools |

`solidcraft-file-pr`, `solidcraft-babysit-pr` (with its `pr-wait` watcher), `solidcraft-full-stop`, `solidcraft-takeover` and `solidcraft-modernize` are adapted from [tstack](https://github.com/t3dotgg/tstack) by Theo Browne, MIT licence ([LICENSE-tstack](LICENSE-tstack)).
