# Test Harnesses (testmiljøer)

`cargo xtask ci` kører alle kontroller, som en pull request skal bestå: fmt, `xtask assets`, `xtask licences --check`, `xtask layers`, clippy med `-D warnings`, tests med `ci`-profilen og kontrol af wasm-build.

| Testmiljø | Placering | Hvad det kontrollerer |
|---|---|---|
| Unit- og engine-tests | Ved koden, `*_tests.rs` | Målte resultater: volumen, areal og topologi mod beregnede værdier |
| Ugyldige input | `hostile_params_never_panic`, `crates/engine/src/tests.rs` | Alle kommandoer × dokumenterede parametre × ugyldige værdier: fejl, aldrig panic |
| Sketch fuzz | `crates/engine/src/cmd/sketch_fuzz_tests.rs` | Tilfældige skitser i solver og profilfinder |
| UI-scenarier | `crates/ui-egui/tests/scenarios/*.json` | Den rigtige app i Headless-tilstand: klik, taster, træk og assertions |
| Layouttests | `crates/ui-egui/tests/ui_layout.rs`, `window_widths.rs` | Paneler og dialoger ved forskellige vinduesbredder |
| Designfil-samling | `apps/solidcraft-cli/tests/corpus` | Gamle filversioner åbnes og beregnes stadig korrekt |
| Afbryd under lagring | `apps/solidcraft-cli/tests/kill_mid_save.rs` | En afbrudt lagring efterlader en læsbar fil |
| Referencetests | `cargo xtask oracle` → [docs/oracle.md](oracle.md) | Emner fra `tests/oracle/*/recipe.json` sammenlignes med analytiske eller FreeCAD-beregnede forventninger |

## Sikkerhedsadvarsler for afhængigheder

Planlagte eller manuelt startede CI-kørsler har et separat job med **Dependency Advisories (sikkerhedsadvarsler)**. Nye advarsler får ikke uvedkommende pull requests til at fejle. Kør samme scanning lokalt:

```sh
cargo install cargo-deny --version 0.20.2 --locked
cargo deny --locked check --warn unmaintained advisories bans sources
```

Projektets `deny.toml` afviser sårbarheder samt ukendte registries og Git-kilder. Ikke-vedligeholdte crates, tilbagetrukne versioner og dubletter giver advarsler. Licenser kontrolleres fortsat af `cargo xtask licences --check`. Gennemgå fund og opdater afhængigheder frem for at ignorere brede kategorier. Den første scanning førte til en opdatering af quick-xml til 0.41.0 for [RUSTSEC-2026-0194](https://rustsec.org/advisories/RUSTSEC-2026-0194) og [RUSTSEC-2026-0195](https://rustsec.org/advisories/RUSTSEC-2026-0195).

Scanningen omfatter ikke versionsmatchning af lokale patches i `vendor/`: `truck-shapeops`, `truck-stepio`, `truck-meshalgo`, `matext4cgmath` og `egui-wgpu`. Koden og dens patches skal stadig sammenlignes med de oprindelige projekter. En vellykket scanning er ikke en sikkerhedsgaranti og indarbejder ikke upstream-rettelser automatisk.

## UI-scenarier

Et scenario er en JSON-liste med trin, der køres mod den rigtige app i en Headless egui-kontekst:

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

Nye filer i `crates/ui-egui/tests/scenarios/` findes automatisk. For at registrere en fejl før rettelsen kan du tilføje et trin `{"pending": "why"}`. Testen springes over med den angivne begrundelse, indtil rettelsen fjerner trinnet.

## CI-profil

CI kører tests med `--profile ci`: let optimering, linjetabeller, ingen LTO og deaktiverede Debug Assertions. Nogle Debug Assertions i truck fejler på gyldig geometri; `ci`-profilen deaktiverer dem og bygger samtidig hurtigere end `--release`. Skriv det i din PR, hvis en test kun fejler i et almindeligt Debug-build.

`cargo xtask test` bygger med denne profil og kører testprogrammer parallelt sammen med dokumentationstests. Argumenter efter `--` videresendes til hvert testprogram, f.eks. `cargo xtask test -- shell`. Brug `cargo test --workspace --profile ci`, hvis du vil køre testprogrammerne sekventielt.

## Fordeling af CI-tests og tunge tests

På GitHub bygger ét job et [cargo-nextest](https://nexte.st/)-arkiv, hvorefter fire jobs kører hver sin fjerdedel af testene (to halvdele på en pull request, `--partition slice:N/4`, indstillinger i `.config/nextest.toml`). De langsomste tests køres først, og dokumentationstests har deres eget job.

Pull requests springer nogle tunge tests over, som hver tager 1–12 minutter. De køres ved alle pushes til `main`, om natten og med `cargo xtask test`:

- **`solidcraft-engine`:** `tests::hostile_params_never_panic`, alle `tests::robustness` og seeds fra `tests::robustness_goldens` (`seed_00` … `seed_95`; relevante pull requests kører alle på nær de langsomste i `SLOW_SEEDS` i `ci.yml`), `pattern_suppress_tests::enclosure_arc_extinction_keeps_a_real_sharp_inner_lip_after_resize_and_reopen`, `pattern_suppress_tests::enclosure_datums_follow_wall_height_and_board_depth_with_material_proofs`, `cmd::plastic::tests::enclosure_snap_fit_follows_resized_front_wall` og kontrakttestene for `atomic_commands`.
- **`solidcraft-kernel`:** `tests::boolean_identity_on_tangent_primitives`, `tspline::tests::hostile_tsplines_never_panic` og property-tests under `boolean_props`. Sidstnævnte køres kun i det natlige \"Property tests\"-job på grund af tilfældige input.
- **`solidcraft-ui-egui`:** UI-scenariet `acceptance_sensor_enclosure`.

**Action Matrix (handlingsmatrixen)** i `apps/solidcraft-cli/tests/action_matrix.rs` forsøger alle almindelige handlinger (skitsering, forskudt plan, Press Pull, Hole, Draft, Shell, Fillet, Chamfer, konstruktionsgeometri, Move, Mirror og Pattern) på alle flader, kanter, hjørnepunkter og emner i en samling af design og sammenligner med en gemt baseline. En handling, som tidligere lykkedes, men nu fejler, får testen til at fejle. Almindelige kørsler tager en fast delmængde, mens det natlige \"Action matrix\"-job kører alle kombinationer. Efter en rettelse kan baseline opdateres med `cargo xtask action-matrix --bless`. Resultater og årsager er dokumenteret på [siden om Action Matrix](https://github.com/bherbruck/solidcraft/blob/main/docs/action-matrix.md).

Kør præcis pull request-testene med `cargo nextest run --cargo-profile ci --profile ci`; tilføj `--ignore-default-filter` for alle tests. En ny test, der tager mere end et minut i CI, kan føjes til `default-filter` i `.config/nextest.toml`, hvis den tester robusthed frem for almindelig funktionalitet.

## Ordliste – engelsk til dansk

| Engelsk udtryk | Dansk betydning |
|---|---|
| Test Harness | Testmiljø |
| Unit Test | Enhedstest |
| Engine Test | Motortest |
| Hostile Input | Ugyldige / fjendtlige input |
| Sketch Fuzz | Tilfældige skitsetests |
| UI Scenario | Brugerfladescenario |
| Layout Test | Layouttest |
| Corpus | Testfilsamling |
| Reference Test | Referencetest |
| Dependency Advisory | Sikkerhedsadvarsel for afhængighed |
| CI | Kontinuerlig integration |
| Sharding | Opdeling af tests på flere jobs |
| Heavy Test | Ressourcekrævende test |
| Regression | Regression / fejl, der genopstår |
| Baseline | Referencegrundlag |
| Assertion | Kontrolbetingelse |
| Checkpoint | Gendannelsespunkt |
| Property Test | Egenskabsbaseret test |
| Pull Request | Forslag til kodeændring |
