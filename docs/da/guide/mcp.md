# SolidCraft and AI Agents (MCP) – SolidCraft og AI-agenter

SolidCraft understøtter [Model Context Protocol](https://modelcontextprotocol.io) via stdio (JSON-RPC 2.0, én besked pr. linje; protokolversionerne 2025-06-18, 2025-03-26 og 2024-11-05). En AI-agent modellerer med samme kommandoer som brugeren: Værktøjerne udfører engine-kommandoer fra værktøjslinjen, kommandopaletten, scripts og kontrolkanalen uden at åbne dialogbokse.

Vedvarende Headless- og Bridged-sessioner tilbyder asynkron billedrendering gennem værktøjet `execute`: `render.preflight`, `render.start`, `render.status`, `render.cancel` og `render.export`. Bevar det komplette job-ID fra svaret. Billeddata forbliver i sessionen, indtil de eksporteres eksplicit som PNG. Se [render jobs](render-jobs.md).

Se [web-runtime.md](web-runtime.md) om tilsvarende kommandoer i browseren (workers, lagdelt lagring og agenter i fanen) samt [plugins.md](plugins.md) om isolerede plugins og parametriske Timeline-funktioner med `run(command, args)`. Desktop-MCP er fortsat referenceimplementeringen.

## Start MCP-serveren

```sh
# Headless: an in-process design session, no window (screenshots are drawn on the GPU).
solidcraft-cli mcp

# Headless, starting from a design file or a command script.
solidcraft-cli mcp --in part.solidcraft

# Bridged to the running app, so you can watch the agent model:
solidcraft --control 7878 &
solidcraft-cli mcp --connect 127.0.0.1:7878
```

I Bridged-tilstand finder serveren appens kontroltoken i den tokenfil, appen har skrevet for porten (se [control-protocol.md](control-protocol.md#token)). `--token T` eller miljøvariablen `SOLIDCRAFT_CONTROL_TOKEN` kan angive det direkte, når appen kører i et isoleret miljø, hvis datamappe serveren ikke kan læse. Miljøvariablen anbefales, fordi kommandolinjeargumenter kan ses i andre brugeres proceslister.

En Bridged-server kan starte, selv om appen endnu ikke lytter på porten. Derfor bevarer MCP-klienten sine værktøjer, hvis den starter før appen eller efter at appen er lukket. Serverens instruktioner fortæller da, at appen ikke kører. Værktøjskald giver en fejl, som starter med `no SolidCraft app is listening at HOST:PORT`, og forklarer, hvordan du starter den med `solidcraft --control PORT`. Første kald efter appens opstart forbinder automatisk og læser det aktuelle token. Hvis appen lukker eller går ned under sessionen, vises samme fejl ved næste kald. Et kald, som var aktivt da appen forsvandt, kan være gennemført eller ikke være gennemført. Undersøg derfor designet, før du sender ændringen igen. Et `batch`, som slet ikke blev sendt, kan trygt prøves igen.

stdout indeholder kun protokolbeskeder, mens fejl skrives til stderr. Ved Bridged-sessioner skriver appen skærmbilleder til en midlertidig fil, som serveren læser. Begge skal derfor køre på samme computer.

### Kontrol af parametre

Kommandoer med et typet parameterskema (foreløbig `solid.box`, `solid.extrude` og `engine.describe`; `describe_command` viser skemaet) kan valideres ved kald. `--param-check off|warn|strict` indstiller niveauet i en Headless-session, også ved scripts og opskrifter angivet med `--in`. Standard er `off`, der ikke ændrer noget. `warn` udfører kaldet og tilføjer `param_warnings` til svaret, eksempelvis en stavefejl som `operaton` i stedet for `operation`. Advarsler fra et opstartsscript går til stderr. `strict` afviser kaldet uden at ændre designet. Bridged-kald til appen kontrolleres endnu ikke.

### Hvilke filer agenten har adgang til

Værktøjskald må kun læse, skrive og slette filer inden for serverens tilladte mapper:

- Mappen med det åbne design (fra `--in` eller senest åbnet eller gemt af agenten), men kun hvis den ligger **inde i** brugerens hjemmemappe eller den midlertidige mappe og ikke er skjult. Hjemmemappen selv, overordnede mapper (`/`, `/home`, `/Users`, drevets rod), skjulte mapper som `~/.config` og andre placeringer er ikke automatisk tilladt. Skjulte filer og mapper som `.env` og `.git` i designmappen er heller ikke tilgængelige.
- SolidCrafts Recovery-mappe, så `doc.recovery_list`, `doc.recover` og `doc.recovery_discard` fungerer. Discard sletter kun reelle gendannelsesfiler.
- Serverens egen Scratch-mappe (en ny privat `solidcraft-mcp-*`-mappe i den midlertidige mappe), hvor `screenshot` uden `path` gemmes.
- Alle mapper angivet med `--allow-path DIR`. Gentag argumentet for flere mapper. En eksplicit tilladt mappe giver adgang til hele mappen, også skjulte filer.

Serverens aktuelle arbejdsmappe tæller ikke automatisk. Derfor er `~/.ssh`, `~/.aws`, `~/.bashrc` og andre skjulte filer som udgangspunkt ikke tilgængelige. Ved opstart viser stderr de tilladte mapper og oplyser, hvis designmappen fra `--in` blev udeladt. Brug `--allow-path` til at tilføje den eksplicit.

Stier opløses til deres faktiske placering, før de sammenlignes. Derfor afvises `..` og symbolske links, der fører uden for en tilladt mappe. Ved lagring gennem et link tæller linkets mål. Nye filer skal placeres i en eksisterende mappe; en manglende undermappe oprettes ikke automatisk og kræver ikke bredere tilladelse. På Windows skal en sti pege på et lokalt drev (`C:\…` eller `\\?\C:\…`). UNC-, enheds- og NT-stier som `\\server\share`, `//server/share`, `\\?\UNC\…`, `\??\…`, `\\.\…`, drevrelative stier som `C:file`, kolon i filnavne (alternate data streams), reserverede enhedsnavne (`NUL`, `CON`, `COM1.txt`) og korte 8.3-navne (`PROGRA~1`) afvises inden adgang til filsystemet. Den opløste sti kontrolleres igen, så også links og junctions til netværksdrev afvises. Det gælder også skrifttypefiler angivet med sti i `sketch.text` eller gemt i et design; installerede skrifttyper valgt ved familienavn virker stadig. Serveren bruger lange navne til egne Scratch- og Recovery-mapper, så de virker, selv om Windows angiver Temp-stien i kort format. Afviste stier giver en forklarende værktøjsfejl.

Hardlinks i en tilladt mappe kan ikke skelnes fra almindelige filer. Et hardlink til en fil uden for mappen vil derfor kunne læses gennem linket. Undgå sådanne links i mapper, du giver adgang til.

```sh
solidcraft-cli mcp --allow-path ~/designs --allow-path ~/exports
solidcraft-cli mcp --allow-any-path   # no limit (the behaviour before this option existed)
```

Ved Bridged-kald sender serveren sine tilladte mapper med hver forespørgsel (`allow_paths`, se [control-protocol.md](control-protocol.md)). Appen tilføjer sin Recovery-mappe og mappen for det åbne design efter samme regler. Brug absolutte stier, da relative stier fortolkes ud fra appens arbejdsmappe og ikke serverens. Appens egne fildialoger er ikke begrænset på samme måde.

### Værktøjsannotationer

Hvert MCP-værktøj angiver, hvad det kan gøre, så klienter kan spørge om godkendelse eller automatisk godkende på det grundlag. `list_commands`, `describe_command`, `inspect_design`, `measure`, `body_topology` og `crash_reports` er skrivebeskyttede. `set_parameter`, `undo` og `redo` ændrer designet med mulighed for at fortryde. `new_design` erstatter designet og er destruktiv. `execute`, `batch`, `open`, `save`, `export` og `screenshot` (med `path`) kan læse, skrive, overskrive eller slette filer. De er ikke skrivebeskyttede, og `destructiveHint` og `openWorldHint` er true.

### Klientkonfiguration

Claude Code (projektets `.mcp.json` eller kommandoen `claude mcp add solidcraft -- solidcraft-cli mcp`). Konfiguration med tidligere navn fungerer fortsat; se [kompatibilitetsnoterne](https://github.com/bherbruck/solidcraft/blob/main/docs/compatibility.md#mcp):

```json
{
  "mcpServers": {
    "solidcraft": { "command": "solidcraft-cli", "args": ["mcp"] }
  }
}
```

Claude Desktop (`claude_desktop_config.json`) med forbindelse til en kørende app:

```json
{
  "mcpServers": {
    "solidcraft": { "command": "/path/to/solidcraft-cli", "args": ["mcp", "--connect", "127.0.0.1:7878"] }
  }
}
```

### Agent Skills (agentfærdigheder)

Mappen [`skills/`](https://github.com/bherbruck/solidcraft/tree/main/skills) indeholder fire [Agent Skills](https://agentskills.io) til AI-agenter, der modellerer gennem serveren:
- `solidcraft-modeling`: Oprettelse af en ny del.
- `solidcraft-editing`: Redigering af et eksisterende design.
- `solidcraft-checking`: Kontrol af fejl, som SolidCraft ikke selv rapporterer, regressionstest og portering.
- `solidcraft-mechanisms`: Samlinger, Joint Origins i verdenskoordinater, Motion Links, lukkede mekanismer, Contact Sets og Motion Studies med kontrol mod analytiske værdier.

De beskriver arbejdsgange, kommandoformater, verificering med `measure` og kendte faldgruber. Med dem kan agenten oprette et emne med få `batch`-kald og læse langt mindre output. Installér dem til Claude Code ved at kopiere mapperne til `~/.claude/skills/`. Se [skills/README.md](https://github.com/bherbruck/solidcraft/blob/main/skills/README.md).

## Værktøjer

| Værktøj | Argumenter | Funktion |
|---|---|---|
| `list_commands` | `filter?` | Viser kommandokataloget med ID, navn, fane/panel, genvej, parametre og aktuel tilgængelighed. `filter` søger i ID, navn, fane og panel. |
| `describe_command` | `id` | Viser dokumentation for én kommandos parametre (`params`) og JSON Schemas (2020-12) for input og vellykket output, når kommandoen har et typet skema (foreløbig `solid.box`, `solid.extrude`, `engine.describe`). Ellers returneres null. Svarer til `execute` med `engine.describe {command}`. |
| `execute` | `command`, `params?`, `verbose?` (default true) | Kører enhver engine-kommando med JSON-parametre. `list_commands` viser mulighederne. Fejl efterlader designet uændret. Feature-svar indeholder funktionens ID og navn, alle emner samt oprettede, ændrede og fjernede emner. `verbose: false` giver samme kompakte svar som `batch`. |
| `batch` | `commands: [{command, params}]`, `rollback?` (default true), `verbose?` (default true), `timings?`, `profile?` (default false) | Kører kommandoer i rækkefølge, stopper ved første fejl og ruller normalt gennemførte trin tilbage. Svaret viser fejlet trin og tidligere resultater. `verbose: false` forkorter svar for store design; oplysninger om berørte emner og nye skitse-ID'er bevares. Med `engine.batch` sendes hele gruppen som én kontrolforespørgsel til appen. Ældre apps eller apps med synlig agentmarkør udfører trinvis. `timings: true` viser tid pr. trin og samlet tid. `profile: true` oplyser også tid til booleske kernel-operationer, tessellering og øvrigt arbejde. Bridged-tider måles i appen. Uden `engine.batch` kan profilering mangle. |
| `inspect_design` | `measure?` (default true), `sketches?` (default true) | Viser designegenskaber (`metadata`), parametre, tidslinje (typer, deaktivering, fejl, advarsler og tider), emner med mål/topologi, skitser med bindinger og frihedsgrader samt markering og Undo-historik. `removed_later` markerer emner, der senere fjernes i en tilbagetrukket historik. Hjælpegeometri kan skjules med `browser.visibility`; også Joint-symboler kan skjules uden at deaktivere deres funktion. |
| `measure` | `body?` or `items?` | Viser volumen, areal, tyngdepunkt, bounding box og antal topologiske elementer pr. emne og samlet. Med `items` måles valgte flader, kanter, hjørnepunkter, skitsekurver eller profiler. Se forklaringen nedenfor. |
| `body_topology` | `body` | Viser kanter med indeks, midtpunkt, længde og ender samt flader med indeks, navn, areal, centroid, normal og et punkt på fladen. Cylinderflader har desuden `cylinder` med `origin`, `axis` og `radius`. Et kantmidtpunkt kan bruges til Fillet og Chamfer. Fladenavnet kan bruges i de understøttede fladekommandoer. |
| `set_parameter` | `name`, `value` or `expression` (number or expression), `unit?`, `comment?` | Opretter eller ændrer en brugerdefineret parameter med værdi/udtryk, eventuel enhed og kommentar. |
| `screenshot` | `view?` (iso, top, front, back, bottom, left, right, home, fit, or a named view saved with `view.save`), `camera?` (`{target, yaw, pitch, distance, fov?}`, as `view.apply`), `path?`, `width?`, `height?`, `source?` (window, model), `grid?`, `axes?` | Tager et billede af modellen eller det kørende appvindue med valgt visning, kamera, billedstørrelse og filsti. Kan bruges til kontrol og dokumentation. |
| `export` | `path`, `format?` (step, 3mf, 3mf-tagged, stl, stla, obj, urdf), `bodies?`, `mesh_prefix?`, `effort?`, `velocity?` | Eksporterer designets emner eller samling til det valgte format, f.eks. STEP, 3MF, STL, OBJ eller URDF, med relevante eksportindstillinger. |
| `undo`, `redo` | — | Fortryder eller gentager én ændring. |
| `crash_reports` | `id?`, `limit?` | Viser nedbrudsrapporter på computeren med de nyeste først. Med `id` vises den komplette rapport med bl.a. backtrace, kommando og gendannelsesoplysninger. |
| `new_design` | `name?` | Starter et nyt, tomt design. |
| `open` | `path` | Åbner en `.solidcraft`-fil eller importerer f.eks. STEP som et nyt design med importerede emner. Den eksisterende model erstattes. |
| `save` | `path?` | Gemmer designet; uden `path` bruges den aktuelle fil. |

`measure` med `items` returnerer hvert element under `items` med `length_mm`, `area_mm2` eller `position`. Cirkler, buer, huller og cylinderflader giver også `diameter`, `radius`, `center` og `axis`, samt `circumference` ved en hel cirkel eller `arc_length` og `angle_deg` ved en bue. `round` angiver typen (`circle`, `arc`, `cylinder`). En Slot (aflang udskæring) giver `slot` med `width`, `center_distance` og `overall_length`. Ved ét element vises felterne også øverst i svaret. Ved to elementer vises:

- `min` (også `distance_mm`, `from`, `to`, `delta`): Korteste afstand.
- `max` (`max_from`, `max_to`): Største afstand.
- `center_distance` (`center_from`, `center_to`): Afstand mellem centrum eller akser for runde elementer. Parallelle akser måles direkte, ikke-parallelle efter korteste afstand, og der kan også måles mellem akse og plan, linje eller punkt.
- `measure`: Den afstand, der først vises i Measure-panelet (`center` hvis muligt, ellers `min`).
- `angle_deg`: Vinkel mellem to plane flader eller rette kanter.

Ældre felter er uændrede, så eksisterende scripts fortsat fungerer.

For at redigere en Feature kan du læse dens definition med `execute` og `timeline.inspect` (`{feature}` som ID eller navn). Ændr derefter felter med `timeline.edit` (`{feature, set}`), f.eks. `{"extent": {"distance": "30"}}`. Felterne for de enkelte typer findes i kapitlet *Solid Features*.

Ressourcer: `solidcraft://design` (designinspektion som JSON med målinger) og `solidcraft://commands` (kommandokataloget).

Fejl: Ukendte værktøjer og argumenter, der ikke passer til skemaet, giver JSON-RPC-fejl `-32602`. Fejl fra geometrimotoren, som ugyldige parametre eller en fejlet Feature, returneres som værktøjsresultat med `isError: true` og fejlmeddelelsen, som agenten kan læse og rette.

Rollback af `batch` bruger et engine-checkpoint (`edit.checkpoint`) oprettet før første trin. Designet, den aktive skitse og Undo-historikken gendannes nøjagtigt, uanset længden af historikken. Hvis batchen bruger `file.new` eller `doc.open`, gendannes designet, men ikke dets filsti. Ældre appversioner uden checkpoints forsøger i stedet at fortryde det antal trin, batchen tilføjede.

## Kamera og nærbilleder

`camera` i `screenshot` (samme objekt som `view.apply`, `view.save` og `solidcraft-cli snapshot --camera` accepterer) er et Orbit-kamera: `{target, yaw, pitch, distance, fov?}` i millimeter og **radianer**. I modsætning til `angle` i `solid.move` betyder et tal uden enhed ikke grader her. Kameraets position er `target + distance * (-sin(yaw) * cos(pitch), -cos(yaw) * cos(pitch), sin(pitch))` og peger mod `target`, med Z opad på skærmen. `yaw: 0, pitch: 0` er forfra: Kameraet står ved −Y og ser langs +Y med +X mod højre. `yaw` på π/2 ser fra −X, π fra +Y og −π/2 fra +X. `pitch` er højdevinklen fra XY-planet; π/2 ser lodret ned med +X mod højre og +Y opad. `fov` er lodret synsfelt i radianer. `fov: 0` eller udeladt giver ortografisk visning med synlig halvhøjde 0,4 × `distance`. Ved perspektiv er halvhøjden ved målpunktet `distance * tan(fov / 2)`. `inspect.section` fjerner materiale i normalens retning og bevarer punkter `p` med `n · p < d`, hvor `d = n · origin`; `flip: true` beholder den modsatte side. Placér kameraet på den fjernede side for at se ind i snittet.

For at vise en enkelt del af en stor model, eksempelvis en STEP-samling, kan du bruge Headless-session. Trinene ændrer kun designet i hukommelsen:

1. Åbn modellen med `solidcraft-cli mcp --in assembly.step` eller værktøjet `open` (som også accepterer `up` og `names`, se trin 4 og [STEP import](step-import.md)). `inspect_design` viser emner med navne og bounding boxes.
2. Fjern det, der blokerer visningen, med `solid.remove`, f.eks. `{"bodies": ["Cover"]}`.
3. Afgræns det ønskede område med `solid.box`, f.eks. `{"corner": [40, 0, -45], "length": 20, "width": 50, "height": 45, "operation": "intersect", "targets": ["Housing", "Plate"]}`. Hvert mål-emne trimmes til boksen, og emner helt uden for fjernes. Selve boksen bevares ikke.
4. Hvis STEP-modellen ligger på ryggen med +Y opad, kan den åbnes med `up: "y"` via `open`, `doc.open` eller `file.insert_step`, så Y bliver Z ved en kvart drejning om X. For en allerede åben model kan `solid.move` rotere de beholdte emner, f.eks. `{"bodies": ["Housing", "Plate"], "axis": [1, 0, 0], "angle": 90}`. Rotationen er om verdenskoordinaternes origo, og vinklen er i grader.
5. Farv vigtige emner med `appearance.assign`, f.eks. `{"bodies": ["Housing"], "color": "#3a7bd5"}`, og skjul hjælpeskitser eller emner med `browser.visibility`, f.eks. `{"items": ["Plate"], "visible": false}`.
6. Lav et snit med `inspect.section`, f.eks. `{"plane": {"origin": [50, 30, 0], "normal": [0, 1, 0]}}` eller `{"plane": "XY", "at": 5}`.
7. Tag billedet med `screenshot`: Angiv et `camera` med `target` i områdets centrum og kameraet på snittets åbne side, `source: "model"`, eventuelt `grid: false` hvis området krydser Z=0, og den ønskede `path`, `width` og `height`. Et ortografisk billede er 0,8 × `distance` højt, så omkring 1,5 gange områdets størrelse kan indramme det. Kontrollér billedet og justér `yaw`, `pitch` og `distance`. I Headless gengives modellen altid; i Bridged kan `source: "model"` vælges frem for appvinduet.

## Kompakte svar

`execute` og `batch` bruger som standard `verbose: true` og bevarer de komplette kommandosvar. Ved længere modelleringssessioner kan du vælge `verbose: false` på værktøjets argumentniveau:

```json
{"name": "execute", "arguments": {"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}, "verbose": false}}
```

Feature-svar udelader den komplette `bodies`-liste, men bevarer `feature`, `name`, `created_bodies`, `changed_bodies` og `removed_bodies`. En Cut kan derfor rapportere et ændret emne, også hvis ingen emnenavne er tilføjet eller fjernet. Skitsesvar bevarer nye ID'er under `created`, men udelader dubletter af listerne `points`, `curves`, `constraints` og `dimensions` øverst. Andre felter og fejl er uændrede. Inspektionskommandoer returnerer stadig fulde resultater. Komprimeringen bruger motorsvaret direkte uden ekstra designinspektion.

## Demo: opbygning af et kabinet via MCP

`examples/demo/enclosure_mcp.py` er en lille MCP-klient. Den styrer en kørende app (`solidcraft --control PORT`) via `solidcraft-cli mcp --connect` og opbygger et elektronikkabinet med ét `execute`-kald ad gangen (`enclosure_steps.json`): en afrundet boks, udhulet og åben foroven, en USB-udskæring i siden og fire skruefæster. Til sidst ændres parameteren `width`, og Timeline genberegnes. `examples/demo/record.sh` optager forløbet med ffmpeg og brænder trinteksterne ind i videoen.

`examples/demo/enclosure_ui_mcp.py` (v2) bygger samme kabinet via den almindelige brugerflade. Markøren flyttes til knapper og flader og klikker, rigtige dialoger åbnes, skitser tegnes på modellens flader, og kameraet følger med. Parametre, skitsemål knyttet til dem og to Extrude-operationer går gennem MCP. Med `--story v3` (standard) gentages ét skruefæste i et 2 × 2 Rectangular Pattern med parameterstyrede afstande. Derefter ændres bredde og væg i Parameters-dialogen. `record_ui.sh` optager forløbet med markør og tekster.

## Eksempel på en session

```text
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"me","version":"1"}}}
→ {"jsonrpc":"2.0","method":"notifications/initialized"}
→ {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"batch","arguments":{"commands":[
     {"command":"sketch.create","params":{"plane":"XY"}},
     {"command":"sketch.rectangle.two_point","params":{"p0":[0,0],"p1":[40,30]}},
     {"command":"sketch.finish"},
     {"command":"solid.extrude","params":{"distance":20}},
     {"command":"solid.fillet","params":{"edges":[[0,0,10]],"radius":3}},
     {"command":"sketch.create","params":{"plane":"XY"}},
     {"command":"sketch.circle.center","params":{"center":[20,15],"radius":5}},
     {"command":"solid.extrude","params":{"distance":20,"operation":"cut"}}]}}}
← {"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"{\"ok\": true, \"completed\": 8, \"results\": [...]}"}]}}
→ {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"measure","arguments":{}}}
← … "total": {"volume_mm3": 22390.6, …}
→ {"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"screenshot","arguments":{"view":"iso"}}}
← {"jsonrpc":"2.0","id":4,"result":{"content":[{"type":"image","mimeType":"image/png","data":"iVBORw0…"},{"type":"text","text":"{\"source\":\"model\",…}"}]}}
→ {"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"export","arguments":{"path":"/tmp/part.step"}}}
```

Tests i `crates/mcp/src/tests.rs` gennemfører lignende sessioner (Box → Fillet → Hole, parametre, Batch Rollback og ugyldige argumenter) og kontrollerer kun resultaterne gennem MCP.

## Ordliste – MCP og AI-agenter

| Engelsk udtryk | Dansk oversættelse |
|---|---|
| Model Context Protocol (MCP) | Protokol til værktøjsadgang for AI-modeller |
| AI Agent | AI-agent |
| Headless | Uden grafisk vindue |
| Bridged | Forbundet til den kørende app |
| Control Channel | Kontrolkanal |
| stdio | Standardind- og uddata |
| stdout | Standarduddata |
| stderr | Standardfejluddata |
| JSON-RPC | Protokol til kommandokald i JSON |
| Client | Klient |
| Server | Server |
| Token | Adgangstoken |
| Scratch Folder | Midlertidig arbejdsmappe |
| Recovery Folder | Gendannelsesmappe |
| Allowed Path | Tilladt filsti |
| Symlink | Symbolsk link |
| Hard Link | Hårdt link |
| Parameter Checking | Parameterkontrol |
| Schema | Dataskema |
| Strict | Streng validering |
| Warning | Advarsel |
| Tool Annotations | Værktøjsmetadata |
| Read-only | Skrivebeskyttet |
| Destructive | Destruktiv / overskrivende |
| Batch | Gruppe af kommandoer |
| Rollback | Tilbagerulning |
| Checkpoint | Gendannelsespunkt |
| Execute | Udfør |
| Inspect Design | Undersøg design |
| Measure | Mål |
| Camera | Kamera |
| Screenshot | Skærmbillede |
| Orthographic | Ortografisk visning |
| Perspective | Perspektivvisning |
| Field of View | Synsfelt |
| Yaw | Vandret kameravinkel |
| Pitch | Lodret kameravinkel |
| Radians | Radianer |
| Verbose | Detaljeret output |
| Compact Reply | Kompakt svar |
| Agent Skills | Agentfærdigheder |
| Tool | Værktøj |
| Command | Kommando |
| Resource | Ressource |
| Error | Fejl |

### Værktøjsnavne

| Teknisk navn | Dansk betydning |
|---|---|
| `list_commands` | Vis kommandokatalog |
| `describe_command` | Beskriv en kommando |
| `execute` | Udfør kommando |
| `batch` | Udfør flere kommandoer |
| `inspect_design` | Undersøg design |
| `measure` | Mål geometri |
| `body_topology` | Vis emnets topologi |
| `set_parameter` | Indstil parameter |
| `undo` | Fortryd |
| `redo` | Gentag |
| `new_design` | Nyt design |
| `screenshot` | Tag skærmbillede |
| `render.start` | Start rendering |
| `render.status` | Vis renderstatus |
| `render.cancel` | Annullér rendering |
| `render.export` | Eksportér rendering |
| `timeline.inspect` | Undersøg Feature |
| `timeline.edit` | Rediger Feature |
| `edit.checkpoint` | Opret gendannelsespunkt |
| `verbose` | Detaljeret svar |
| `param_warnings` | Parameteradvarsler |

<!-- docs-english-updates:start -->

## Engelske ændringer, der mangler oversættelse

> Nedenfor står nye og ændrede afsnit fra den engelske original. Arbejd dem ind i oversættelsen, kontrollér den aktuelle engelske side, og fjern derefter hele denne markerede sektion.

The English original also removes or replaces text. Review the corresponding translated text.

## Tools

| Tool | Arguments | What it does |
|---|---|---|
| `list_commands` | `filter?` | The command catalog: id, label, tab/panel, shortcut, parameter docs, enabled now. `filter` matches id, label, tab and panel. |
| `describe_command` | `id` | One command's parameter docs (`params`) and, for commands with a typed contract (`solid.box`, `solid.extrude` and `engine.describe` so far), the JSON Schemas (2020-12) of its parameters (`input`) and of a successful result (`output`); `null` for the others. The same as `execute` `engine.describe {command}`. |
| `execute` | `command`, `params?`, `verbose?` (default true) | Run any command with JSON parameters (`list_commands` shows them). Failures leave the design unchanged. A feature command replies with the feature (`feature`, `name`), every body (`bodies`) and the bodies it created, changed and removed (`created_bodies`, `changed_bodies`, `removed_bodies`). `verbose: false` uses the same compact reply as `batch`. |
| `batch` | `commands: [{command, params}]`, `rollback?` (default true), `verbose?` (default true), `timings?`, `profile?` (default false) | Run steps in order, stop at the first error and undo the completed steps. The reply names the failing step (`failed.index`, `failed.command`, `failed.error`) and every result before it. `verbose: false` keeps the reply small in large designs: feature steps leave out the full body list (`created_bodies`, `changed_bodies`, `removed_bodies` stay) and sketch steps the ids they repeat outside `created`. Bridged to an app that has `engine.batch`, the whole batch is one control request (one frame of the app). Older apps, and apps showing the agent cursor, get the steps one by one. `timings: true` adds `timings: [{index, command, ms}]` for every step run, the failing one included, and `total_ms`. `profile: true` also adds each step's `profile: {boolean_ms, tessellate_ms, other_ms}`, its time in kernel booleans, in tessellation and the rest. Steps are timed where they run: in the app when bridged. An app without `engine.batch` is timed step by step, without `profile`. |
| `inspect_design` | `measure?` (default true), `sketches?` (default true), `joints?` (default true) | Design properties (`metadata`: project, author… and other tools' keys; set them with `execute` `document.metadata.set`), parameters, timeline (type, suppressed, rolled back, error, warning, timing), bodies (volume, area, centre of mass, bbox, face/edge/vertex counts, visible; `removed_later: true` while a sketch is being edited on a rolled-back timeline and a later feature deletes the body, which is then not shown), sketches (curves, points, constraints, profiles, DOF, solve status, visible), joints when the design has any (as `joint.inspect` gives them: each origin's occurrence, component and path, the snap as saved, the feature, face, edge or sketch curve (`sketch: {id, name, curve}`) its persistent names refer to, `resolves` and the `error` when it is lost; kind, offset, angle, flip, values, limits, expressions, motion links; `execute` `joint.inspect {joint}` gives one joint), selection, undo depth. Hide helper sketches, bodies or construction planes with `execute` `browser.visibility` (`{items, visible}` or `{folder, visible}`), and joint glyphs with `{joints: [id or name], visible}` or `{folder: "joints", visible}` (a hidden joint still constrains and drives; `joint.list` reports each joint's `visible`). |
| `measure` | `body?` or `items?` | Volume (mm³), area (mm²), centre of mass, bbox and topology counts per body, with totals. With `items` (one or two selections as `execute` takes them: `face`, `edge`, `vertex`, `sketch_curve`, `profile`), the picked geometry instead: see below. |
| `body_topology` | `body` | Edges (index, midpoint, length, ends) and faces (index, name, area, centroid, normal, `point`, and for a cylindrical face its `cylinder`: `origin`, `axis`, `radius`). An edge midpoint is the `[x,y,z]` that picks that edge for fillet and chamfer. A face's `point` lies on it and picks it (its centroid may not: that of a face with a hole is in the hole); a face's `name` picks it for the commands that take faces (`face.delete`, `solid.shell`, `solid.draft`, `solid.offset_face`, `solid.split_face`, `solid.replace_face`, and `face` of `sheet.convert`). |
| `set_parameter` | `name`, `value` or `expression` (number or expression), `unit?`, `comment?` | Create or change a user parameter and recompute. `expression` is another name for `value` (the one `parameters.change` uses): give one of them, or both only when they agree; neither, or two that differ, is an error. Reports `recomputed` (features rebuilt), `restored` (features whose earlier result for the same inputs was reused, e.g. a value set back) and features that now fail. |
| `screenshot` | `view?` (iso, top, front, back, bottom, left, right, home, fit, or a named view saved with `view.save`), `camera?` (`{target, yaw, pitch, distance, fov?}`, as `view.apply`), `path?`, `width?`, `height?`, `source?` (window, model), `grid?`, `axes?` | A PNG image content block. Headless: a render with the viewport's pipelines on a headless GPU device (see `solidcraft-cli gpu`; with no adapter, the CPU rasterizer, without analysis colours), fitted to the model, or from the named view or camera; the reply's `renderer` says which (`gpu`, `gpu-software`, `cpu`, with a `warning` for `cpu`). Bridged: the app window (default) or a model render with the app's camera; a named view or camera moves the app's viewport there first (`execute` `view.apply` does the same without a screenshot). `grid: false` and `axes: false` leave the ground grid (it lies on Z 0 and cuts through a part that straddles it) and the origin's axes out of a model render; axes hidden with `browser.visibility` stay out whatever `axes` says. Bridged, a screenshot that gives `grid` or `axes` is a model render, which follows the app's Grid and Origin switches for what it does not give; `source: "window"` with either is an error. A window that is minimised or on another macOS Space presents no frame: with no `source` the reply is a model render and says so (`fallback`); `source: "window"` is an error saying why, at once. Agents that don't need the app's chrome can ask for `source: "model"`. |
| `export` | `path`, `format?` (step, 3mf, 3mf-tagged, stl, stla, obj, urdf), `bodies?`, `mesh_prefix?`, `effort?`, `velocity?` | STEP, 3MF or mesh export; URDF writes the whole assembly with its meshes next to it (`docs/urdf.md`). |
| `undo`, `redo` | — | Undo / redo one change. |
| `crash_reports` | `id?`, `limit?` | The crash reports on this computer, newest first; with `id`, the whole report (backtrace, command, recent commands, `design_path`/`replay_path`). See [crash-reports.md](crash-reports.md). |
| `new_design` | `name?` | Start an empty design. |
| `open` | `path` | Open a `.solidcraft` design, a STEP file (`.step`/`.stp`) as a new design holding its bodies (an Import base feature), or a 3MF/STL file as mesh bodies. |
| `save` | `path?` | Save the design (to its current file without `path`). |

<!-- docs-english-updates:end -->
