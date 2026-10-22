{{#include ../../../mcp.md}}

## Naming bodies in scripts

Automatic names such as `Body1` and `Body2` are given by position and are not stable references for scripts.
When creating a body, pass `body_name` to commands
such as `solid.extrude` or `solid.box`, then use that name in later commands. For example,
use `"body_name": "Mounting plate"` and refer to it as `"body": "Mounting plate"`.
See [solid features](features.md) for body creation tools.
