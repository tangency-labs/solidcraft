# Sources and provenance ledger

This ledger was first reviewed on 2026-10-21. Native-source admission was updated on 2026-10-22 in [SOURCE.md](../../assets/render/selene/SOURCE.md) and the [source lock](../../assets/render/selene/source-lock.json). These references support specific facts or existing contracts; catalog names, recipes, implementation plans and prose are independently authored. Referenced shader implementations, textures and HDR maps are not bundled.

| ID | Source | Use and boundary |
|---|---|---|
| SRC-SOLIDCRAFT-BASE | [Personal integration branch, pinned commit](https://github.com/0xlillith/solidcraft/tree/93a2922c5ee272fb605703ae84c05825b4a74607) | Repository rules and current documented contracts. This package does not infer support from future planning entries. |
| SRC-SETTINGS | [Render settings contract](https://github.com/0xlillith/solidcraft/blob/93a2922c5ee272fb605703ae84c05825b4a74607/docs/render-settings.md) | Fields and bounds for the six baseline setup examples. |
| SRC-SELENE-V7 | [SELENE model repository at the inspected revision](https://github.com/0xlillith/solidcraft-rover/tree/b32437915372d32b71419dc8f5143fe22ff1e869) | Historical v7 identity, published statistics and inspected native assembly inventory; excluded as the active showcase source. The legacy v7 file and its demonstration media are not bundled. |
| SRC-SELENE-ATTRIBUTION | [SELENE attribution](https://github.com/0xlillith/solidcraft-rover/blob/b32437915372d32b71419dc8f5143fe22ff1e869/ATTRIBUTION.md) | Original rover, emblem and modeled-lettering attribution; the native package retains the Ubuntu Font Licence notice. |
| SRC-SELENE-SHOWCASE | [Author's improved mechanism demonstration](https://github.com/tangency-labs/solidcraft/pull/1540#issuecomment-6205857321) | Documented improved mechanisms. The public demonstration is reference material; the owner-supplied native file is admitted separately in the source lock. |
| SRC-SELENE-EXPORT | [Motion animation export PR](https://github.com/tangency-labs/solidcraft/pull/1680) | Names `SELENE-showcase.solidcraft` and reports author-run exports; not this contribution's runtime evidence. |
| SRC-SELENE-ANALYSIS | [Motion analysis PR](https://github.com/tangency-labs/solidcraft/pull/1712) | Existing kinematic rate/interference tools and their sampled-check limitations. |
| SRC-SELENE-MECHANISMS | [Pinned project mechanism reference](https://github.com/tangency-labs/solidcraft/blob/463cea0615a22be2f7d6c30512211f9eda97ff28/skills/solidcraft-mechanisms/reference.md) | Public project command conventions, actuator geometry and loop-stop examples. |
| SRC-SUN-FACTSHEET | [NASA Sun Fact Sheet](https://nssdc.gsfc.nasa.gov/planetary/factsheet/sunfact.html) | Apparent solar diameter context for the approximate authored disk size; no site ephemeris or irradiance calibration. |
| SRC-MOON-FACTSHEET | [NASA Moon Fact Sheet](https://nssdc.gsfc.nasa.gov/planetary/factsheet/moonfact.html) | Reference surface gravity of 1.62 m/s². Other tabulated global quantities are not treated as local shader parameters. |
| SRC-MOON-FACTS | [NASA Moon Facts](https://science.nasa.gov/moon/facts/) | Broad surface/regolith and tenuous-exosphere context. No surface imagery is copied. |
| SRC-POLAR-LIGHT | [NASA SVS: Lunar South Pole Illumination with Earth and Sun](https://svs.gsfc.nasa.gov/5228/) | Low-Sun and terrain-occlusion context. No animation, DEM or ephemeris data is copied. |
| SRC-LUNAR-DUST | [NASA NTRS: PELT investigation record](https://ntrs.nasa.gov/citations/20090022233) | Vacuum ballistic particle-motion context; no claim that all lunar dust forces are modeled. |
| SRC-OPENPBR | [ASWF OpenPBR Surface specification](https://academysoftwarefoundation.github.io/OpenPBR/) | Public surface-model specification, observed v1.1.1 dated 2026-04-17. Conceptual reference only; no code, artwork or specification text is copied. |

The use of a public specification does not authorize a prohibited implementation dependency. The source repository's clean-room rules continue to apply. See [CLEAN_ROOM.md](CLEAN_ROOM.md) for the complete admission and authorship policy.

## Implemented UI design reference

The UI continuation inspected upstream main at **`728f753b8094e1a54c5cc8b7e453982afa028023` (0.18.4)** on 2026-10-21. This is a read-only design reference; the personal integration base remains `93a2922c5ee272fb605703ae84c05825b4a74607` (0.18.2).

[ui-integration.json](ui-integration.json) records 21 exact path/blob comparisons and their source URLs. The primary references are the [workspace/ribbon structure](https://github.com/tangency-labs/solidcraft/blob/728f753b8094e1a54c5cc8b7e453982afa028023/crates/ui-egui/src/workspace.rs), [current UI kit](https://github.com/tangency-labs/solidcraft/blob/728f753b8094e1a54c5cc8b7e453982afa028023/docs/ui-kit.md), [theme](https://github.com/tangency-labs/solidcraft/blob/728f753b8094e1a54c5cc8b7e453982afa028023/crates/ui-egui/src/theme.rs) and [binding icon style guide](https://github.com/tangency-labs/solidcraft/blob/728f753b8094e1a54c5cc8b7e453982afa028023/assets/icons/ui/STYLE_GUIDE.md). Current code takes precedence over historical picture proposals. Existing icon masters were previewed for review only; no UI artwork or screenshot is added to the shipped package.
