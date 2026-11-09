# Raspberry Pi 5 visual reconstruction

The electronics work uses original SolidCraft geometry to replace the source board's generic port and GPIO envelopes with recognizable packages, connectors and contacts. It is a dimension-informed visual model for close inspection and rendering. It is not a complete electrical BOM, netlist, PCB fabrication file or manufacturer-certified enclosure model.

## Reference geometry

The [Raspberry Pi 5 mechanical drawing](https://datasheets.raspberrypi.com/rpi5/raspberry-pi-5-mechanical-drawing.pdf) gives an approximately **85 × 56 mm** board outline, **58 × 49 mm** mounting-hole spacing, **3.5 mm** offsets to the nearby edges and **2.7 mm** mounting-hole diameter. The manufacturer explicitly describes its dimensions as approximate and leaves some components out of the drawing. These are visual reference dimensions, not manufacturing tolerances.

The existing SELENE PCB uses a **1.6 mm** modeled thickness. Its final placement must be read from the evaluated source: the source timeline contains later translations, so the early sketch plane is not the assembled board elevation. The component generator preserves the original mounting position and documents its exact world-coordinate origin.

The [official GPIO documentation](https://www.raspberrypi.com/documentation/computers/raspberry-pi.html#gpio-and-the-40-pin-header) establishes a **2.54 mm** pitch. The visual header therefore uses two rows of 20 individually modeled pins, with a separate dark carrier and visible solder/pad details.

## Component identity and connector inventory

The manufacturer's [Pi 5 introduction](https://www.raspberrypi.com/news/introducing-raspberry-pi-5/) identifies the BCM2712 application processor, LPDDR4X memory, RP1 I/O controller, DA9091 power-management IC, CYW43455 wireless chip and BCM54213 Ethernet PHY. The PHY is visibly rotated 45 degrees. These identities guide named geometry rather than copied chip artwork or logos.

The [current official board documentation](https://www.raspberrypi.com/documentation/computers/sbcs.html#raspberry-pi-5-ports) provides the connector inventory:

| Interface | Visual modeling target |
|---|---|
| USB-A | Two stacked USB 2.0 ports with black tongues, two USB 3.0 ports with blue tongues; hollow metal shells and visible contacts. |
| Ethernet | RJ45 cavity, eight contact representations and two LED windows. |
| GPIO | 40 pins in two rows of 20. |
| Display output | Two micro-HDMI connector shells and contact representations. |
| Power | USB-C shell and tongue. |
| Camera/display | Two 22-contact MIPI FFC connectors at 0.5 mm pitch. |
| PCIe | One 16-contact FFC connector at 0.5 mm pitch. |
| Auxiliary headers | Four-pin fan, three-pin UART, two-pin RTC and four-pin PoE. |
| Storage | Underside microSD cage and contact representations. |
| Board details | Power button, status LED, shields, crystals, inductors, representative passives and plated holes. |

The [manufacturer's PCIe connector specification](https://datasheets.raspberrypi.com/pcie/pcie-connector-standard.pdf) independently confirms the 16-contact, 0.5 mm-pitch interface. Detailed solder terminations, internal contact spring shapes and production-specific component populations remain simplified.

## Rendering and assembly contract

A separate component groups the new electronics and attaches rigidly to the original avionics assembly. Ports have openings, tongues and contacts; separate materials distinguish solder mask, package epoxy, ceramic, connector polymer, shields, heat spreaders, solder and gold-colored plating. Appearance values are illustrative PBR controls. A bulk Brass material on a gold-colored contact is a density proxy, not a claim that the contact is solid gold.

Exact body counts, removed placeholder names, generator hash and native validation results belong in the generated build evidence. No count or completed validation result is inferred from this reference document. Before shipping, check the evaluated component position, mounting-hole and header spacing, representative connector cavities, the rigid attachment and a close-up native render.

All added geometry and documentation are original. No Raspberry Pi trademark artwork, manufacturer CAD, PCB texture or photograph is distributed with the model.
