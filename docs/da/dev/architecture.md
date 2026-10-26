# Architecture (arkitektur)

SolidCraft er et Cargo-workspace opdelt i lag. En crate må kun afhænge af crates i lavere lag. `cargo xtask layers` får bygningen til at fejle, hvis reglen brydes.

| Lag | Crates | Ansvar |
|---|---|---|
| L0 | `geom` | Vektorer, planer, profiler, mesh og deres mål |
| L1 | `sketch`, `kernel`, `render` | Beregning af bindinger og profiler; grænsefladen til B-rep-kernen (truck); visningsmatematik og beskrivelse af Headless-scenen |
| L2 | `doc` | Parametre, udtryk, Feature-tidslinjen og trinvis genberegning |
| L3 | `io` | Designfiler, STL/OBJ/STEP/3MF-eksport og import-Features til STEP, 3MF, STL, OBJ og glTF |
| L4 | `engine` | Session og kommandoregister (alt udføres som kommandoer) |
| L5 | `gpu`, `ui-egui`, `mcp` | Beregning af visningsmesh med compute shaders, udskiftelig desktopbrugerflade og MCP-server (Headless eller forbundet til appen) |
| Apps | `solidcraft`, `solidcraft-cli` | Desktopprogram og kommandolinje uden grafisk vindue |

Webappen `apps/solidcraft-web` er desktopbrugerfladen kompileret til wasm med [trunk](https://trunkrs.dev). `xtask` indeholder repositoryets egne udviklingsværktøjer: `cargo xtask ci`, `layers`, `assets`, `features`, `oracle` og `book`.

Lagdelingen medfører følgende regler:

- **Kun `crates/kernel` må bruge truck direkte.** Lagene ovenover ser kun SolidCrafts egne typer: emner, flader, kanter, mesh og målinger. Se [Kernel-grænsefladen](kernel.md).
- **Dokumentlaget kender ikke til kommandoer eller brugerfladen.** `crates/doc` indeholder parametre, Feature-tidslinjen og komponenthierarkiet og beregner modellen.
- **Alt, brugeren kan gøre, er en kommando** i `crates/engine`. Se [Kommandosystemet](commands.md).
- **Brugerfladen kan udskiftes.** Ingen lag under `ui-egui` afhænger af egui. CLI og MCP-serveren kører samme engine uden et vindue. Headless-billeder fra `solidcraft-cli snapshot`, MCP-skærmbilleder, `ui.render` og miniaturer tegnes på GPU'en med samme pipelines som Viewport (`crates/gpu`, `capture`). Engine beskriver scenen i `view::scene`, og værtsprogrammet installerer rendereren via `engine::capture`. Niveauerne vælges i rækkefølgen: hardware-GPU, softwareadapter (Mesa lavapipe eller llvmpipe, WARP) og til sidst CPU-rasterizeren i `crates/render`, som ikke tegner overfladeanalyser, fremhævninger eller signaturforklaringer. `SOLIDCRAFT_GPU=auto` forsøger alle tre; `hardware` og `software` bliver på deres niveau og giver fejl, hvis det mangler; `cpu` gennemtvinger rasterizeren. Alle billedsvar angiver `renderer` (`gpu`, `gpu-software`, `cpu`), og CPU-billeder ledsages af en advarsel. `solidcraft-cli gpu` viser tilgængelige adaptere og det niveau, politikken vælger.

## Ordliste – engelsk til dansk

| Engelsk udtryk | Dansk betydning |
|---|---|
| Architecture | Arkitektur |
| Workspace | Arbejdsområde / workspace |
| Crate | Rust-pakke / crate |
| Layer | Lag |
| Dependency | Afhængighed |
| Constraint Solver | Beregner for geometriske bindinger |
| Kernel | Geometrikerne |
| Boundary | Grænseflade |
| Feature Timeline | Tidslinje over modelleringsfunktioner |
| Incremental Evaluation | Trinvis genberegning |
| Command Registry | Kommandoregister |
| Front End | Brugerflade |
| Swappable | Udskiftelig |
| Headless | Uden grafisk vindue |
| Capture | Billedoptagelse |
| Render Pipeline | Renderingspipeline |
| Compute Shader | Beregningsshader |
| Software Adapter | Softwarebaseret grafikadapter |
| CPU Rasterizer | CPU-baseret rasterizer |
| Viewport | Visningsområde |
| B-rep | Grænsefladerepræsentation |
