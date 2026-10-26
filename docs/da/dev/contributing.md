# Bidrag til SolidCraft

SolidCraft er et open source-program til parametrisk 3D-CAD skrevet i Rust. Du tegner på et plan, tilføjer bindinger, omdanner profiler til solide emner og ændrer parametre for at genberegne tidslinjen. Programmet kan bruges på desktop (Windows og Linux), i browseren (wasm med WebGPU/WebGL2), via kommandolinjen uden grafisk vindue og som MCP-server for AI-agenter.

Alle bidrag er velkomne – fra en lille rettelse til ansvar for et helt område. Denne side forklarer, hvordan du bygger programmet, hvordan kildekoden er organiseret, reglerne for bidrag, og hvor hjælpen er mest efterspurgt.

Se [kommandooversigten](docs/features.md) og [udviklingsplanen](ROADMAP.md), før du vælger en opgave, så du kender de implementerede funktioner og aktuelle prioriteter.

Har du spørgsmål eller idéer? Deltag på [Discord](https://discord.gg/FtU4NQNVDH).

## Kom i gang (cirka 10 minutter på en hurtig computer)

Du skal bruge stabil Rust 1.95 eller nyere (egui/eframe 0.36 kræver det). Til webbygningen kræves desuden `rustup target add wasm32-unknown-unknown` og [`trunk`](https://github.com/trunk-rs/trunk) ([dokumentation](https://trunk-rs.github.io/trunk/)).

```sh
git clone https://github.com/tangency-labs/solidcraft && cd solidcraft
cargo run --release -p solidcraft -- --sample      # desktop app with a sample part
cargo xtask test                                   # unit, engine and UI-scenario tests
cargo xtask ci                                     # everything a PR must pass (see below)
```

SolidCraft er navnet på produkt, repository, Cargo-pakke og eksekverbar fil. Desktopappen bygges med `cargo build --release -p solidcraft`; CLI med `cargo build --release -p solidcraft-cli`. Kør CLI med `cargo run --release -p solidcraft-cli -- commands` eller `target/release/solidcraft-cli commands`. Brug ikke det gamle navn, der kun findes i kompatibilitetslaget beskrevet i [docs/compatibility.md](docs/compatibility.md).

Web: Kør `cd apps/solidcraft-web && trunk serve --release`, og åbn den viste URL. `?sample` indlæser eksempelmodellen, og `?webgl` gennemtvinger WebGL2. Browserens filfunktioner kræver HTTPS eller localhost på grund af Origin Private File System.

Windows-build fra Linux: `cargo xwin build --release --target x86_64-pc-windows-msvc -p solidcraft`.

### NixOS: `nix develop`

NixOS har ikke `/usr/lib`, så et almindeligt `cargo run` kan mangle runtimebiblioteker på `LD_LIBRARY_PATH`: Wayland til winit-vinduet, EGL til wgpu-rendering og libdbus-1 til rfd-fildialoger via xdg-desktop-portal. Mangler sidstnævnte, vises dialogen ikke:

```
ERROR solidcraft: winit EventLoopError: ... The wayland library could not be loaded
ERROR eframe::native::run: Exiting because of error: WGPU error: Failed to create surface for any enabled backend: {}
ERROR rfd::backend::xdg_desktop_portal::portal::libdbus: Can't connect to a portal: libdbus-1.so not found
```

`nix develop` fra [`flake.nix`](flake.nix) i repositoryets rod stiller biblioteker til Wayland, X11, EGL, Vulkan og D-Bus til rådighed samt driveroplysninger fra `/run/opengl-driver/share`. Aktivér `hardware.graphics.enable` i NixOS. Andre Linux-distributioner kan kræve [nixGL](https://github.com/nix-community/nixGL). Standardudviklingsmiljøet bruger det eksisterende Rust-/C-værktøjssæt på `PATH`; `nix develop .#rust` leverer det fastlåste værktøjssæt og bygger i `target/nix`.

Start med `nix run .#solidcraft -- --sample`. Installér terminalkommandoer, skrivebordsgenvej og ikoner med `nix profile install .#solidcraft`. `nix build` opretter kun et buildresultat, ikke en launcher. `solidcraft-cli` installeres også. Se [Nix-installation og fejlfinding](docs/nix.md) om remote flakes og forkert eller gammel eksekverbar fil på `PATH`.

## Kodens opbygning

[Arkitekturkapitlet](docs/architecture.md) viser lagene. Kort fortalt:

- `crates/geom`: Vektorer, planer, mesh og målinger.
- `crates/sketch`: Egen solver for bindinger og søgning efter profiler.
- `crates/kernel`: B-rep-grænseflade omkring [truck](https://github.com/ricosjp/truck). Kun denne crate må bruge truck. Fillet, Chamfer, Shell/Offset, Delete Face, Split Face, STEP/IGES samt fladers ophav er egen kode. Tre rettede truck-crates findes i `vendor/` og er undtaget fra fmt/clippy.
- `crates/doc`: Parametre, udtryk, tidslinje med trinvis genberegning, komponenter, forekomster, ledforbindelser, udseender og vedvarende navngivning.
- `crates/io`: Designfiler og meshformater (3MF, STL, OBJ).
- `crates/engine`: Session og kommandoregister. **Alt, brugeren kan gøre, er en kommando**, også når den startes fra UI, scripts, CLI, kontrolkanal eller MCP.
- `crates/ui-egui`: Udskiftelig desktop- og webbrugerflade (egui + wgpu). Underliggende crates afhænger ikke af egui.
- `crates/mcp`, `apps/solidcraft`, `apps/solidcraft-cli`, `apps/solidcraft-web`.

`cargo xtask layers` afviser builds, hvor et lavere lag afhænger af et højere.

## Ufravigelige regler

1. **Uafhængighed.** SolidCraft udvikles ud fra offentlig dokumentation, almindelige CAD-principper og feedback. Referencetests bruger analytiske værdier og open source-værktøjer. Vi automatiserer ikke kommercielle CAD-programmer og udtrækker ikke data fra dem. Kopiér aldrig et andet produkts kode, ikoner, grafik, hjælpetekst, tooltips, skabeloner eller materialer. Kommando-ID'er er projektets egne (`solid.fillet`).
2. **Ingen GPL/LGPL/AGPL-kode – heller ikke til gennemlæsning.** Det gælder OpenCASCADE, FreeCAD (herunder planegcs), SolveSpace, CadQuery/OCP, LibreCAD og QCAD. Projektet har MIT OR Apache-2.0-licens, og solver og afrundinger er skrevet ud fra matematikken. Et open source-program kan godt køres uafhængigt for at beregne referenceværdier, men kildekoden må ikke læses. Tilladeligt licenserede afhængigheder er i orden; begrund dem i PR'en. `THIRD-PARTY-LICENSES.txt` genereres efter merge. Nightly og manuel CI scanner sikkerhedsadvarsler og afhængighedskilder med cargo-deny; se [testdokumentationen](docs/book/src/dev/testing.md#dependency-advisories).
3. **Alle aktiver skal være originale eller åbent licenserede og opført i [ATTRIBUTION.md](ATTRIBUTION.md).** Ikoner tegnes i kode (`crates/ui-egui/src/icons.rs`), og `cargo xtask assets` håndhæver reglen.
4. **Undgå nedbrud.** Uden for testkode må `unwrap`, `expect`, `panic!` og indeksering på inputafhængige data ikke bruges. `unsafe` er kun tilladt i `apps/solidcraft/src/open_documents.rs` (filåbning på macOS), `apps/solidcraft/src/native_crash.rs` (native Windows-nedbrud) og `crates/threedx-client/src/macos.rs` (3Dconnexion på macOS); compileren kontrollerer dette. Ikke-understøttede situationer skal give en fejl frem for panic. Truck-kald går via `solidcraft_kernel::guard`. Hver nedbrudsrettelse kræver en regressionstest.
5. **Alt er en kommando med en test.** Nye funktioner kræver en `CommandSpec` under `crates/engine/src/cmd/`, engine-tests med målte størrelser (volumen, areal og antal flader, kanter og hjørnepunkter) og UI-scenarier ved ændringer i brugerfladen.
6. **Ét fælles system til kamerastyring.** Alle sider og canvases med 3D-visning bruger `navigation::handle` (`crates/ui-egui/src/navigation.rs`) og `NavCaps` til at angive, om rotation og markering tillades. Det gælder også Render-preview og nye 2D-visninger, hvor Orbit er slået fra. Implementér ikke egne musetræk, hjul- eller knibebevægelser til kameraet. Testen `hand_rolled_camera_input_is_rejected` afviser dem.

Brug samme begreber i labels, parameterværktøjer, meddelelser og dokumentation som i [ordlisten](https://docs.solidcraft.io/reference/glossary.html). Nye begreber skal tilføjes dér.

[AGENTS.md](AGENTS.md) indeholder de detaljerede regler for kodestil og AI-agenter.

## Tests

| Hvad | Hvor | Kør |
|---|---|---|
| Enheds- og engine-tests | Ved kildekoden eller i `*_tests.rs` | `cargo xtask test` (eller `cargo test --workspace --profile ci`) |
| UI-scenarier | `crates/ui-egui/tests/scenarios/*.json` | Del af `cargo test` |
| Referencetests | `cargo xtask oracle` | `tests/oracle/` (se nedenfor) |

Hvis en geometriændring ændrer robuste testreferencer (goldens), skal de genindspilles i samme PR med `ROBUSTNESS_GOLDENS_BLESS="why it changed" cargo test --profile ci -p solidcraft-engine --lib robustness_goldens::` (se [docs/robustness.md](docs/robustness.md)). CI kører alle seeds ved relevante geometriændringer. [Mutationstest-dokumentationen](docs/mutation-testing.md) forklarer, hvordan man finder kernelopførsel, som black-box-tests kan overse.

**UI-scenarier** styrer det rigtige program uden et grafisk vindue: Start et værktøj, klik på et plan eller skitsepunkt, indtast taster, og kontrollér resultatet.

```json
[
  {"start": "sketch.create"},
  {"click": {"plane": "XY"}},
  {"start": "sketch.rectangle.two_point"},
  {"click": {"sketch": [0, 0]}},
  {"click": {"sketch": [40, 30]}},
  {"expect": {"errors": 0}}
]
```

En ny `.json`-fil i scenariemappen registreres automatisk. For at beskrive en fejl, før den er rettet, kan scenariet indeholde `{"pending": "why"}`. Fjern markeringen, når PR'en retter fejlen.

**En fejlet test kan ikke merges alene.** Det ville gøre CI på `main` rød for andre. Medtag rettelsen i samme PR, eller markér testen `#[ignore = "fails until #123 is fixed"]`, så den kan merges og senere aktiveres igen.

**Brug `ci`-profilen.** CI kører med `--profile ci`: let optimering, linjetabeller, ingen LTO og deaktiverede Debug Assertions. Enkelte Debug Assertions i truck afviser gyldig geometri. Profilen bygger hurtigere end `--release`. Oplys i PR'en, hvis en test kun fejler i et normalt Debug-build. `cargo xtask test` bygger med profilen og starter testprogrammer parallelt, mens `cargo test` normalt kører dem efter hinanden. Argumenter efter `--` gives til hvert testprogram, f.eks. `cargo xtask test -- shell`. Tests må ikke være afhængige af virkelig klokketid.

**Pull requests, der er klar, tester det, de ændrer.** CI forbinder ændrede filer til workspace-pakker og kører Clippy, testbuild og tests for pakkerne og deres afhængigheder. [Pakkeselectoren](scripts/ci/affected.py) og [CI-workflowet](.github/workflows/ci.yml) angiver, hvilke ændringer der kræver native kontroller. Rent dokumentations-, pakke-, workflow- eller botscriptarbejde kører kun repositorykontroller på pull requests og `main`. Ændringer på workspace-niveau, grupper i merge queue, nightly og Run workflow tester hele workspacet. Ved push til `main` kan en vellykket merge queue-kørsel med samme commit genbruges. Er ingen kode ændret siden sidste vellykkede push, køres kun repositorykontroller; ellers det fulde testforløb. Dokumentationstests, referencetests og Windows-jobs køres på `main` og nightly.

**CI fordeler testene.** Ét job bygger et cargo-nextest-arkiv. To grupper kører på pull requests, og fire ved fuld validering. Hver test kører i sin egen proces, så den må ikke afhænge af en anden tests tilstand. Nogle tunge tests på 1–12 minutter springes over i almindelige PR-kørsler, men køres ved fuld validering i merge queue, om natten, manuelt og ved pushes til `main`, som køen ikke allerede har testet. Kontrollér resultaterne efter merge. Listen findes i `.config/nextest.toml` og på [testsiden](docs/book/src/dev/testing.md):

- **`solidcraft-engine`:** `tests::hostile_params_never_panic`, tests i `tests::robustness`, seeds fra `tests::robustness_goldens` (`seed_00` … `seed_95`; PR'er med relevant geometri kører alle undtagen de langsomste i `SLOW_SEEDS` i `ci.yml`), `pattern_suppress_tests::enclosure_arc_extinction_keeps_a_real_sharp_inner_lip_after_resize_and_reopen`, `pattern_suppress_tests::enclosure_datums_follow_wall_height_and_board_depth_with_material_proofs`, `cmd::plastic::tests::enclosure_snap_fit_follows_resized_front_wall` og kontrakttests for `atomic_commands`.
- **`solidcraft-kernel`:** `tests::boolean_identity_on_tangent_primitives`, `tspline::tests::hostile_tsplines_never_panic` og property-tests under `boolean_props` (kun det natlige Property Tests-job, da input er tilfældige).
- **`solidcraft-ui-egui`:** Scenariet `acceptance_sensor_enclosure`.

**Handlingsmatrixen** prøver hver almindelig handling på alle flader, kanter, hjørnepunkter og emner i en testfilsamling og sammenligner med en gemt baseline ([docs/action-matrix.md](docs/action-matrix.md)). Når en handling, der tidligere fejlede, nu består, opdateres referencen med `cargo xtask action-matrix --bless`. Hvis en tidligere fungerende handling fejler, fejler CI-testen.

**Referencetests** findes i [`tests/oracle/`](tests/oracle/) med `recipe.json` og tilhørende `expected.json`. SolidCraft bygger modellen og sammenligner antal emner, volumen, areal, tyngdepunkt, Bounding Box og eventuelt overfladen fra `reference.stl`. Forventede tal er enten analytiske (`tools/oracle-analytic/`) eller beregnet ved at bygge opskriften i FreeCAD som separat program (`tools/freecad-oracle/`). Den genererede [referencerapport](docs/oracle.md) viser testene og resultaterne. Ved fuld CI kører `cargo xtask oracle`, mens almindelige PR-kørsler springer det over. En kendt fejl angives i `expected.json` med `"known_failure": "#123"`; ændr aldrig en forventet værdi alene for at passe med SolidCrafts output.

## Ændring af designfilformatet

En designfil angiver formatet med `"format": "solidcraft/N"`. Gemmer en ændring nye felter, nye Feature-typer, nye navnetyper eller en anden betydning af ældre design, kræver det en formatændring. **Tilføj én fil uden at redigere fælles versionsfiler:**

1. Opret `crates/doc/src/format/pending/<what-it-is>.rs` med små bogstaver og bindestreger. Begynd med en `//!`-kommentar om, hvad der gemmes, hvordan ældre design håndteres, og at seneste stabile udgivelse beder om opdatering frem for at forsøge indlæsning.
2. Hvis ældre design skal omskrives ved åbning, tilføjes `pub fn step(v: &mut Value) -> crate::Result<()>` i samme fil. Afventende migrationer kører i filnavnerækkefølge efter de allerede udgivne formattrin.
3. Rør ikke `FORMAT`, `JSON_STEPS`, historikken i `crates/doc/src/format.rs`, andre ændringers filer eller nogen `v<N>/`-mappe. Udgivne versioner ligger fast.

Det er tilstrækkeligt. Bygningen tæller de afventende filer og vælger næste formatnummer, som ingen udgiven version kan læse. Dermed afviser en stabil udgave designet frem for at miste data. Filerne indeholder også en liste over deres afventende ændringer (`format_pending`), så et senere udviklingsbuild kun kører manglende migrationer. To PR'er med hver sin tilføjelse kan merges uden konflikt. Se `crates/doc/src/format/pending/README.md`.

**Ved udgivelse** kører vedligeholderen `scripts/fold-format-pending.sh`, som flytter afventende filer til `crates/doc/src/format/v<N>/` og fastlåser versionsnummeret (se [docs/releasing.md](docs/releasing.md)). Tilføj aldrig en fil til en allerede udgivet `v<N>/`-mappe.

## Lister som mange bidrag ændrer

Visse filer får nye poster i mange samtidige PR'er. De er organiseret, så to tilføjelser kun rammer hinanden, når deres poster står ved siden af hinanden. Det mindsker mergekonflikter og gentagne CI-kørsler:

- **Meddelelseskataloger** (`assets/locales/*.json`): Én meddelelse pr. linje, sorteret efter ID. Tilføj meddelelsen og kør `cargo xtask lists --fix`. `cargo xtask lists` (del af `cargo xtask ci` og Lists-workflowet) fejler på usorterede eller gentagne ID'er.
- **`mod`-linjer:** Én pr. linje i den sorterede gruppe, de hører til (`cargo fmt` holder grupperne sorteret). Tilføj modulet til den eksisterende gruppe, ikke en ny gruppe nederst.
- **Sprog** (`crates/ui-egui/src/i18n.rs`): Én pr. linje, engelsk først og derefter sorteret efter ID; en test kontrollerer dette.
- **Genererede filer:** `THIRD-PARTY-LICENSES.txt` og `docs/features.md` regenereres på `main` efter merge af Generated Files-workflowet (`cargo xtask licences`, `cargo xtask features`). Redigér eller regenerér dem ikke i en PR.
- **Designformat:** Én fil pr. ændring, aldrig en ændring af et fælles versionsnummer.

## Indsend en ændring

1. Arbejdet styres via [GitHub issues](https://github.com/tangency-labs/solidcraft/issues) med områdelabels (`area:kernel`, `area:sketch`, `area:doc`, `area:ui`, `area:infra`). Opret først et issue for større opgaver, der ikke har et. **Reserver opgaven med en draft PR**, så to ikke udvikler samme funktion:
   - Kontrollér, at ingen allerede har den: `gh pr list -R tangency-labs/solidcraft --state open --search "#N"`.
   - Opret en branch fra nyeste `main`, commit `git commit --allow-empty -m "claim: #N"`, push, og opret en **draft PR** med titel `<summary> (#N)`, f.eks. `Fillet: variable radius along a chain (#123)`.
   - **Den åbne PR med det laveste nummer får opgaven.** Hvis din PR har tabt, så luk den eller hjælp på den ældre PR.
   - Botten reserverer ved oprettelse, tildeler dig issuet, tilføjer `in progress` og skriver en kommentar. Den læser `#N` i titlen og `Fixes #N` i brødteksten; ved eksisterende reservation skriver den en besked på din PR.
   - Du kan kommentere `/claim` på issuet før oprettelse af en branch.
   - Push fremskridt mindst dagligt. En draft uden pushes i 72 timer kan overtages; kommentér først, og opret derefter din egen reservation.
   - Markér PR'en klar til review, når den er færdig.
2. Opret branchen fra `main`, og lad PR'en omhandle ét emne.
3. Kør `cargo xtask ci` før push. Det udfører fmt, kontrol af aktiver, licenser og lag, Clippy med `-D warnings`, tests og wasm-build. PR'en skal bestå. Ved almindelige pushes kører format-/repositorykontroller og hurtig compile (`cargo check`). Efter vedligeholderreview tilføjes `approved-for-ci`, som udløser fuld CI (build, tests, goldens, Clippy). Efterfølgende pushes kører det hele igen. Godkendte PR'er merges i grupper efter område, så eventuelle konflikter kun skal løses én gang pr. gruppe.
4. Henvis til issuet (`Fixes #12`). Oplys, hvad du har testet, og medtag både målte og forventede tal ved geometriændringer.

Kommentaren `/claim` tildeler dig opgaven, tilføjer `in progress` og oplyser fristen for automatisk frigivelse. Uden tilknyttet PR frigives den efter 72 timer; med PR efter 14 dage uden pushes. Det er adskilt fra forventningen om daglige fremskridt og 72-timers overtagelsespolitikken. En draft PR, der henviser til issuet, fungerer på samme måde. `/unclaim` frigiver reservationen tidligere (kun bidragyderen eller en vedligeholder). Vedligeholderes egne issues frigives aldrig automatisk.

Hjælp reviewerne med at forstå ændringen. Ved observerbar CAD-, UI- eller modelopførsel bør issue eller PR indeholde faktiske, beskrivende skærmbilleder eller en kort optagelse. Dokumentation kræves for ændringer og fejl, der kan demonstreres i appen, også ved kernel, geometri, samlinger og backend. Kodeagenter bør selv køre et lille reproducerbart eksempel og optage beviset. Vis den faktiske fejl eller det faktiske resultat, helst før og efter, med korte trin og forventet/faktisk opførsel. En pil eller markering kan fremhæve det relevante.

Flowcharts og Mermaid-diagrammer erstatter ikke dokumentation af synlig adfærd. Diagrammer hjælper ved arkitektur, hvor skærmbilleder ikke er dækkende. Ændringer uden relevant observerbar adfærd kræver ingen billed- eller videodokumentation. Brug en lille eksempelmodel uden private data; vedhæft mediet eller link til en tilgængelig optagelse frem for at committe genererede reviewaktiver.

Følg din PR-status, men push ikke alene for at følge `main` (ingen merge af `main`, Update Branch eller rebase kun derfor). Hvert push genstarter CI-køen for alle bag dig, mens `main`-kørslen allerede kontrollerer de mellemliggende ændringer. Ved rigtige ændringer eller konflikter skal du rebase til nyeste `main` og pushe med `--force-with-lease`, eller merge `main` én gang og løse konflikterne. Genstart ikke CI for et uændret commit; reelle fejl forsvinder ikke af gentagelse. Brug kun Re-run Failed Jobs ved runnerproblemer. Følg åbne PR'er gennem statusændringer, og oplys ved overdragelse, hvad der afventer merge.

Store dele af SolidCraft er skrevet af AI-kodeagenter (Claude), der har arbejdet parallelt med hver deres område, mens et menneske har sat retning og rapporteret fejl. Arbejdsnoterne findes i [AGENTS.md](AGENTS.md). Menneskelige bidrag er lige så velkomne. Brug dine foretrukne værktøjer, hvis ændringen følger reglerne.

## Labels og triage

Triage-botten tildeler labels på nye issues og PR'er (`.github/workflows/triage.yml`). Den tilføjer kun labels. Ret dem gerne; den tilføjer dem ikke igen.

- **PR'er** får `area:*` efter filstier (`crates/kernel` og `crates/geom` → `area:kernel`; `crates/ui-egui` og apps → `area:ui`; `.github/`, `scripts/` og `docs/` → `area:infra` osv.).
- **Issues** får `bug` ved fejlrapporter, `enhancement` ved funktionsforslag, `priority:P1` ved nedbrud og et relevant `area:*` ud fra titlen eller kommandoen. Indtil prioritet vælges, har de `needs-triage`; den fjernes, når en prioritet sættes.
- **Vedligeholdere** bruger `/triage area:kernel P1` med et eksisterende label; `P0`–`P3` ændrer prioritet, mens `-label` fjerner en label.
- **Korte fejlrapporter uden reproduktionstrin** får `needs-info` og én kommentar med spørgsmål om trin og eventuelt version. Hvis en appfejlrapport var tom, spørges efter rapporten fra udklipsholderen. Når rapportøren svarer eller redigerer med trin, fjernes labelen. Efter 14 dage i `needs-info` lukkes issuet, men kan genåbnes. Funktionsforslag, komplette apprapporter og vedligeholderes egne issues får ikke dette spørgsmål.
- `needs-triage` er arbejdslisten: [åbne issues med mærket](https://github.com/tangency-labs/solidcraft/issues?q=is%3Aopen+label%3Aneeds-triage).

## Hvor hjælp er mest efterspurgt

**Kernelhastighed (langsomme Fillet, booleske operationer og STEP):** Omskriv ikke den nuværende kernel for at optimere hastigheden. Den erstattes af [solidcore](https://github.com/bherbruck/solidcore) ([fremskridt](https://tangency-labs.github.io/solidcore/)). Rapportér langsomme tilfælde med en fil under labelen `kernel-perf`, og læg kernelarbejdet i solidcore. Se [#1597](https://github.com/tangency-labs/solidcraft/issues/1597).

Se [`help wanted`](https://github.com/tangency-labs/solidcraft/labels/help%20wanted) og [`good first issue`](https://github.com/tangency-labs/solidcraft/labels/good%20first%20issue). Prioriteret efter nytte for brugerne:

1. **Robuste Fillet og overgangsflader.** Fillet er egen kode, da truck ikke har funktionen. Konstant radius, variabel radius og chord-afrunding samt hjørner, hvor forskellige radier mødes, understøttes allerede – også tre afrundinger i samme hjørne. Næste opgaver er setback-hjørner, afrundinger langs buede kanter mellem buede flader og afrundinger, som opsluger naboflader.
2. **Robuste booleske operationer.** Fuzzing af tilfældige design gennemføres i øjeblikket for cirka halvdelen uden kernelfejl. Hvert fejlet seed er en reproducerbar fejl.
3. **Tegninger, CAM og meshværktøjer.** De er uden for den nuværende plan (se [ROADMAP.md](ROADMAP.md)), men en bidragyder, der vil eje et område, er velkommen.

## Opdatering af den genererede Features-oversigt

Når kommandoer tilføjes eller omdøbes, køres `cargo xtask features`. Kommandoregisteret er kilden til `docs/features.md`; redigér ikke de genererede rækker manuelt. Generatoren sorterer efter ID og udelader globale og områdevise tællere, så tilføjelser andre steder ikke omskriver fælles oversigtslinjer.

Hvis branches alligevel konflikter i denne fil, eksempelvis ved tilføjelser tæt på hinanden, så løs først konflikter i kommandoregister og kildekode, kør derefter `cargo xtask features` igen, og tilføj den genererede fil. Brug ikke Gits `union` merge-driver; den kan stiltiende beholde forældede eller duplikerede rækker. Tests for generatoren kontrollerer stabil sortering og at andre rækker ikke ændres. Første overgang til sorteret indhold gav én større diff.

## Licens

Som bidragyder accepterer du, at dit arbejde licenseres under MIT OR Apache-2.0 ligesom resten af SolidCraft. Projektet er ikke tilknyttet nogen CAD-leverandør; produktnavne er deres respektive ejeres varemærker (se NOTICE).

## Ordliste – engelsk til dansk

| Engelsk udtryk | Dansk betydning |
|---|---|
| Contributing | Bidrag til projektet |
| Pull Request (PR) | Forslag til kodeændring |
| Draft PR | Kladde til pull request |
| Maintainer | Vedligeholder |
| Issue | Opgave eller fejlrapport |
| Claim | Reservation af opgave |
| Merge Queue | Kø til fletning af ændringer |
| Regression Test | Regressionstest |
| Reference Test | Referencetest |
| Debug Build | Udviklingsbuild |
| Release Build | Optimeret build |
| Code Review | Kodegennemgang |
| Clean Room | Uafhængig implementering |
| Feature | Modelleringsfunktion |
| Workspace | Arbejdsområde |
| Crate | Rust-pakke |
| Kernel | Geometrikerne |
| Test Harness | Testmiljø |
| Shard | Del af opdelte tests |
| Triage | Kategorisering og prioritering |
| Roadmap | Udviklingsplan |
| Generated Files | Genererede filer |
| Design Format | Designfilformat |
