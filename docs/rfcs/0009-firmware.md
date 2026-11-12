# RFC 0009: Firmware and testability

| | |
|---|---|
| Status | Stub: scope fixed, detailed plan to write |
| Started | 2026-11-12 |
| Apps | SolidCraft |
| Maps to | phase P2 firmware steps of [RFC 0004](0004-product-workflow.md); optional |
| Useful after | [0008 Electronics](0008-electronics.md) |
| Scope detail | [rfc-roadmap](../workflow/rfc-roadmap.md#0009--firmware-and-testability) |

## Summary

A product is not a board plus a shell; it runs code. Integrate firmware rather than build an IDE:
generate board definitions and pin maps from the schematic, keep source in the product, build with
existing toolchains, flash and monitor, and design the board for programming and test.

## Scope

- Board definitions and pin maps generated from the schematic; net and pin names carried through.
- Firmware source in the product; build via PlatformIO, Arduino or Rust embedded; no IDE rebuild.
- Flash, serial monitor and probe/debug integration.
- Test points, programming headers, factory-test access and HIL hooks.
- Firmware revision linked to the board revision and the release manifest.
- Documentation: build, flash, recover, test.

## Four surfaces

Every command ships in the GUI, CLI, MCP server and documentation. The MCP surface matters here:
agents must be able to build and flash a known revision.

## Readiness (optional)

Blink an LED and read a sensor using only pins defined by the schematic; firmware builds and
flashes from the product.

## Non-goals

- An editor, debugger or compiler implementation.
- Vendor SDK replacement.
