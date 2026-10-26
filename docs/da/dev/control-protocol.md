# Control Protocol (kontrolprotokol)

`solidcraft --control PORT` (eller `SOLIDCRAFT_CONTROL_PORT=PORT`) lytter på `127.0.0.1:PORT` efter JSON-linjer. Hver forespørgsel er én linje `{"id": …, "method": "…", "params": {…}}`; svaret er én linje `{"id": …, "ok": true, "result": …}` eller `{"id": …, "ok": false, "error": "…"}`. Forespørgsler udføres på UI-tråden mellem frames. Mus og tastatur sendes som rigtige egui-hændelser med én markørhændelse pr. frame.

### Token

Hver forbindelse skal godkendes. Ved opstart opretter appen et tilfældigt 256-bit-token, som gælder for denne kørsel. Det skrives som én linje i hexadecimalformat til en fil, som kun brugeren må læse:

| System | Tokenfil |
|---|---|
| Linux | `$XDG_DATA_HOME/solidcraft/control/PORT.token` (`~/.local/share/solidcraft/control/PORT.token`) |
| macOS | `~/Library/Application Support/SolidCraft/control/PORT.token` |
| Windows | `%LOCALAPPDATA%\SolidCraft\control\PORT.token` |

På Unix har mappen rettighederne `0700` og filen `0600`; på Windows erstattes de nedarvede filrettigheder med én tilladelse til brugeren. `SOLIDCRAFT_CONTROL_DIR` flytter mappen. Filen slettes, når appen lukkes, og et nyt token oprettes ved næste start. Stien til tokenfilen skrives til stderr ved opstart.

Første forespørgsel på en forbindelse skal indeholde tokenet i feltet `"token"` øverst (`{"id": 1, "method": "ui.inspect", "params": {}, "token": "…"}`). Senere forespørgsler på samme forbindelse kan udelade det. Er tokenet forkert eller mangler, returneres `{"ok": false, "error": "unauthorized: …"}`, og forbindelsen lukkes uden udførelse. Tokenet sammenlignes på konstant tid.

`solidcraft-cli mcp --connect 127.0.0.1:PORT` læser selv tokenfilen for porten. Du kan angive det direkte med `--token T` eller `SOLIDCRAFT_CONTROL_TOKEN`. Til et testmiljø eller en sandbox, som ikke kan se appens datamappe, kan appen startes med `SOLIDCRAFT_CONTROL_TOKEN` sat til mindst 32 hextegn (f.eks. `openssl rand -hex 32`). Så bruges dette token, men filen oprettes stadig. Her er en enkel Python-klient:

```python
token = (Path.home() / ".local/share/solidcraft/control/7878.token").read_text().strip()
sock.sendall((json.dumps({"id": 1, "method": "ui.inspect", "params": {}, "token": token}) + "\n").encode())
```

### Grænser

Kontrolkanalen skal behandles som upålideligt input. En forespørgselslinje må højst være 16 MiB. Større linjer får svaret `{"ok": false, "error": "request too large"}`, og forbindelsen lukkes. Det samme gælder efter otte linjer, der ikke indeholder gyldig JSON. Højst 16 forbindelser håndteres samtidig; øvrige får `{"ok": false, "error": "too many connections"}`. Der udføres højst 64 kølagte forespørgsler pr. frame, så mange kald ikke blokerer tegning. En forespørgsel, som UI-tråden ikke har besvaret inden 60 sekunder (3600 sekunder for `engine.batch`), får `{"ok": false, "running": true, "error": "timeout: … is still running …"}`. MCP-serveren bruger samme grænse på 16 MiB pr. linje.

En forespørgsel, der har overskredet tidsgrænsen, er **ikke nødvendigvis fejlet**. Den fortsætter i appen og kan stadig ændre designet, f.eks. efter en langsom Fillet. Send den derfor ikke blot igen. Forbindelsen beholder sit svar (#1618). Hent status med `control.pending`: Mens kaldet kører, returneres `{"pending": <id>, "method": …, "done": false}`; derefter `{"pending": <id>, "method": …, "done": true, "reply": {…}}` én gang, eller `{"pending": null}`, hvis intet afventer. Indtil kaldet afsluttes, afvises alle øvrige forespørgsler på forbindelsen straks med `{"ok": false, "busy": true, "error": "busy: …"}`. Dermed ligger der ingen skjulte kommandoer i kø.

Alle forespørgsler kan have `allow_paths: ["/abs/folder", …]` i `params`. Hver fil, som forespørgslen læser, skriver eller sletter (f.eks. `file.export` med `path`, `doc.recovery_*` med `dir`, `ui.render` eller `ui.screenshot`), skal da ligge inden for de tilladte mapper, efter at `..` og symbolske links er opløst. Ellers afvises kaldet med `… outside the folders this agent may use …`. Appen tilføjer sin gendannelsesmappe og mappen med det åbne design, hvis den ligger sikkert under brugerens hjemmemappe og ikke er skjult. Skjulte filer i designmappen er stadig utilgængelige. MCP-serveren, der er koblet til appen, sender sine mapper ved alle kald ([mcp.md](mcp.md)). Uden `allow_paths` begrænses filstierne ikke: En klient med tokenet kan foretage de samme filhandlinger som brugeren.

## Engine (kommandosystem)

| Metode | Parametre | Resultat |
|---|---|---|
| `engine.execute` | `command`: kommando-ID, `params`: JSON-objekt | Kommandoens resultat |
| `engine.script` | `commands`: `[{command, params}, …]` | Resultater; stopper ved første fejl |
| `engine.batch` | `commands`: `[{command, params}, …]` (højst 10.000), `rollback?`: bool (standard true), `timings?`, `profile?`: bool (standard false) | `{completed, results, undone}` samt `failed: {index, command, error}` ved fejl. Med `timings` tilføjes `timings: [{index, command, ms}]` (inklusive fejlet trin) og `total_ms`; `profile` tilføjer pr. trin `profile: {boolean_ms, tessellate_ms, other_ms}`. Alle trin udføres i én forespørgsel og samme frame, efter reglerne for `engine.execute`. Første fejl stopper batchen, og `rollback` gendanner designet (`undone` angiver de fortrudte trin). Kaldet afvises før udførelse med `engine.batch unavailable`, når agentmarkøren vises, eller et trin åbner en fil (`doc.open`, `file.insert_step`, `file.insert_mesh`). Send disse trin enkeltvis. Svaret ventes i op til 3600 sekunder |
| `engine.commands` | — | Alle kommandoers ID, navn, fane, panel, ikon, genvej, parameterdokumentation og tilgængelighed |
| `document.inspect` | `measure?`: bool | Parametre, tidslinje (fejl og tider), emner, skitser og markering |

Kommandoer åbner aldrig dialogbokse, når de køres på denne måde. `solidcraft-cli commands` viser samme kommandoregister.

Billeder af modellen bruger `render.preflight/start/status/cancel/export` gennem `engine.execute`. Bevar job-ID'et fra svaret til senere kald; det gælder kun for sessionen. Statuskaldet blokerer ikke og udfører ikke rendering. Se [render jobs](render-jobs.md) om billedoptagelse, annullering og eksport.

Når en almindelig filvælger er åben, viser `ui.inspect` værdien `file_picker_busy: true`. Engine-kommandoer, `ui.file` og `ui.home` venter på dens svar. Ændring af vinduesstørrelse, inspektion og skærmbilleder virker stadig.

Alle `sketch.*`-kommandoer, som ændrer en skitse, returnerer desuden `created: {points, curves, constraints, dimensions}` (og `wires` ved 3D-kurver): ID'erne for oprettede elementer i oprettelsesrækkefølge sammen med andre felter (`curves`, `sides`, `corners` osv.). Skitse-ID'er er stabile og genbruges aldrig efter sletning. Efter `l1` og `l2` bliver næste ID `l3`, selv om `l2` er slettet; kun Undo kan rulle tælleren tilbage med designet. Derfor kan ID'erne bruges direkte i senere kommandoer:

```text
{"id":7,"method":"engine.execute","params":{"command":"sketch.line","params":{"points":[[0,0],[40,0]]}}}
→ {"curves":["l1"],"created":{"points":["l1.start","l1.end"],"curves":["l1"],"constraints":["k1"],"dimensions":[]},…}
{"id":8,"method":"engine.execute","params":{"command":"sketch.dimension","params":{"entities":["l1"],"value":50}}}
```

Et tal uden enhed i `value` til `sketch.dimension` (fra kontrolkanalen, scripts eller MCP) fortolkes i designets enheder, som om værdien var indtastet i målfeltet. I et design med tommer giver `"value": 2` en linje på 2 in (50,8 mm). Punktkoordinater er stadig i millimeter. Brug et udtryk med eksplicit enhed for at være uafhængig af designets enheder: `"value": "50 mm"` eller `"value": "0.5 in"`. Vinkler angives i grader, medmindre udtrykket angiver andet.

Viewport viser også en målestreg opgjort efter X, Y og Z mellem endepunkterne for den aktuelle afstand (centrum, minimum eller maksimum). Hver verdensakseafstand vises separat som en absolut værdi, også nul. Visningen bruger designets enheder og præcision uafhængigt af kamera og skitseplan. **Preferences → Graphics → Show X/Y/Z distances** styrer de ekstra linjer og etiketter (slået til som standard og gemmes). Med `ui.prefs {"measure_axis_distances": false}` skjules de, men den samlede afstand og dialogens ΔX/ΔY/ΔZ bevares. Målestregen vises, indtil Measure lukkes. Overlay-identifikatorerne `measure:label:x`, `measure:label:y`, `measure:label:z` identificerer etiketterne, og `prefs:measure_axis_distances` identificerer afkrydsningsfeltet.

## UI (brugerflade)

| Metode | Parametre | Tekniske noter (original) | Dansk forklaring |
|---|---|---|---|
| `ui.inspect` | — | UI state, pending file picker (`file_picker_busy`; compatibility alias `file_dialog_open`, no path), what is hidden (`visibility`: bodies, origin and construction items, sketches, joints (`hidden_joints`, joint ids); set it with the `browser.visibility` command), viewport rect, camera, active tool/dialog, the status line (`status`: text and whether it is an error), hover, renderer, timings, live preview (`active`, `busy`, `error`, `ms`, bodies it stands in for) | Læs UI-tilstand, aktiv dialog og værktøj, synlighed, markering, kamera, statusmeddelelser, renderer, tider og forhåndsvisning. |
| `ui.set` | any `UiState` field (`tab`, `perspective`, `showGrid`, `dark`, …), and `hiddenBodies`, `hiddenOrigin`, `hiddenSketches`, `shownSketches`, `hiddenJoints`, `hiddenFolders` (whole lists; `browser.visibility` changes single items) | `dark: false` switches to the light theme | Indstil felter i brugerfladens tilstand; `dark: false` skifter til lyst tema. |
| `ui.view` | `view`: `front`, `back`, `top`, `bottom`, `left`, `right`, `iso`, `home`, `fit`; `animate` (bool, default false: snap) | | Vælg standardkameraretning eller tilpas udsnittet; animation er som udgangspunkt slået fra. |
| `ui.camera` | `target?`: `[x, y, z]` (mm), `yaw?`, `pitch?`, `roll?` (radians), `distance?` (mm), `fov?`, `up?`; `fit?`: `true`, `false` or `[occurrence id or name…]` (`root`: the bodies outside every component) | the viewport's orbit camera: it looks at `target` from `distance` away; `yaw` 0 looks along +Y (from the front), `pitch` is the elevation (±π/2: straight down or up), `roll` turns the image. Fields given are set first (the others keep their values), then `fit` frames the listed occurrences (or everything), keeping the direction. The reply, passed back, sets the same view again, so a framing can be stored and replayed. `fov` and `up` follow the perspective setting (`ui.set`) and the up axis preference (`ui.prefs`) and can't change here; a refused field changes nothing | Læs eller indstil kameraets målpunkt, vandrette og lodrette vinkler, rulning og afstand. `fit` indrammer hele modellen eller valgte instanser; svaret kan gemmes og genbruges. |
| `ui.start` | `command`, `params?` | like a toolbar click: starts the sketch tool or dialog for the command. `params` sets what the dialog opens on, and nothing runs until OK: `joint.drive` takes `joint` (id or name; without it, the joint picked in the browser, else the first joint that moves) and `values` (numbers in degrees or millimetres, or expressions); `motion.study` takes `study` (id or name), which opens that study to edit and chart. A command that takes no params is refused before it starts; a param the dialog doesn't take is refused and the dialog closes | Start et værktøj eller åbn dets dialog som ved et klik på værktøjslinjen. Parametre forudfylder dialogen, men udføres først ved OK. |
| `ui.click` | `x`, `y` (screen points), `button?`: left/right/middle, `shift?`, `ctrl?`, `double?` | real pointer input | Send et ægte museklik til angivet skærmposition med valgfri knap og modifikationstaster. |
| `ui.move` | `x`, `y`, `shift?`, `alt?`, `ctrl?`, `cmd?` | pointer movement without a button; modifiers apply until the next synthetic move/click; a move with omitted modifiers releases them. With `nav: "touchpad"`, Shift pans and Alt/Option orbits. | Flyt markøren uden museknap. Shift kan panorere og Alt/Option rotere med touchpad-profilen. |
| `ui.drag` | `x0`, `y0`, `x1`, `y1`, `button?`, `shift?`, `ctrl?`, `steps?`, `hold?`, `hold_end?` | press, move, release; a left drag on the model is a box selection (left to right: window, right to left: crossing); with OpenSCAD navigation, plain left-drag on empty space orbits, Shift+left-drag box-selects, and drags starting on sketch geometry edit it; `hold` keeps the button down that many frames first (a right press-and-hold opens the marking menu, releasing over a direction picks it), `hold_end` that many frames at the end (a sketch drag shows its snap) | Tryk, træk og slip en museknap; kan bruges til markeringsrammer, skitseændringer og radialmenu med holdetid. |
| `ui.menu` | `x?`, `y?`, `target?`, `close?` | with a point: open a context menu there as a right-click would (the viewport's marking menu, or a browser menu with `target: {"type": "body", "name"} \| {"type": "sketch", "id"} \| {"type": "component", "id"}`); without: the open menu (`radial` and `items`: id, label, shortcut, enabled) | Åbn kontekstmenuen ved en position, eller læs den aktuelt åbne menus punkter. |
| `ui.menuPick` | `item`: id or label | runs an item of the open menu, as clicking it does | Vælg et menupunkt efter ID eller synligt navn. |
| `ui.selectOther` | `x?`, `y?`, `pick?`, `add?`, `hover?`, `close?` | Select Other: with a point, list everything pickable there (as holding the left button does); `pick`: select row `pick` (`add`: as Shift-click; with a dialog open, into its active input); `hover`: light a row up; without: the open list (`open`, `at`, `total`: everything under the cursor, `rows`: label, icon, sel, at most 20 per kind, nearest first) | Vis overlappende objekter under markøren, fremhæv et valg, eller vælg elementet i model eller dialog. |
| `ui.rename` | `text?`, `commit?` (default true) | finishes the rename box a Rename item opened | Afslut eller bekræft omdøbning af et markeret element. |
| `ui.selection` | | the selection, the open dialog's inputs, the hovered item, and the active sketch's drawn dimensions (`dimensions`: id and text centre, `dimension_selected`, `dimension_editing`) | Læs markeringer, aktive dialogfelter, element under markøren og mål i den aktuelle skitse. |
| `ui.worldToScreen` | `points`: `[[x, y, z]…]` world points | their screen points | Omsæt 3D-verdenskoordinater til skærmpunkter. |
| `ui.triad` | | screen points of the 3D sketch move triad's handles (`x`, `y`, `z`, `plane`) | Læs skærmpositionen for flytteaksens X-, Y-, Z- og planhåndtag. |
| `ui.sketchToScreen` | `points`: `[[x, y]…]` in active-sketch coordinates | their screen points (null when behind the camera) | Omsæt punkter i den aktive skitses koordinater til skærmpunkter. |
| `ui.dialogInput` | `index` | what clicking an input's Select chip does: picks go to that input | Vælg hvilket inputfelt i dialogen der skal modtage den næste markering. |
| `ui.at` | `world: [x,y,z]` \| `sketch: [x,y]` \| `plane: "XY"` \| `axis: "Z"` \| `dimension: "d1"` (its value text) \| `handle: "arrow"` (a manipulator handle, see `scenario::publish_handle`) | the screen point (a scenario step's target; see `crates/ui-egui/src/scenario.rs`) | Find skærmpositionen for et navngivet geometrisk punkt, plan, mål eller manipulatorhåndtag. |
| `ui.handles` | `prefix?` | the handles drawn last frame, `[{name, x, y}]`: toolbar buttons and drop-down items `toolbar:<command id>` (the button alone: `button:<command id>`; a drop-down row: `dropdown:<command id>`, highlighted `dropdown_hot:<command id>`, its pin `dropdown_pin:<command id>`; Customize Toolbar's check boxes `customize:<command id>`), panel labels `panel:<tab>:<panel>`, browser rows `row:<label>` and fold arrows `fold:<label>`, manipulator handles; signed-axis tips `axis_view:z`, `axis_view:-y`; optional cube facets `navigation_cube:top`, `navigation_cube:top_front_right`, controls `navigation_cube:control:home` (and other actions), plus signed-axis aliases for visible cube faces; Preferences navigator picker `prefs:navigator`, options `prefs:navigator:Signed axes` and `prefs:navigator:Cube`; in the Motion Study dialog, where the chart puts its first and last step `motion:chart_start` and `motion:chart_end`, the Check collisions button `motion:check` and each collision found `motion:collision:<n>` | Læs synlige håndtag og knappers skærmpositioner, så agenten kan klikke præcist. |
| `ui.editFeature` | `feature` (id or name) | what double-clicking a timeline item does: rolls back to it and opens its dialog filled in (sketches open in sketch mode) | Rul tidslinjen tilbage til en Feature, og åbn dens redigeringsdialog. |
| `ui.scroll` | `x`, `y`, `delta?` | wheel zoom at the cursor | Simulér musehjulets zoom ved markøren. |
| `ui.key` | `key` (egui key name, `Enter`, `Escape`…), `cmd?`, `shift?` | | Send tastetryk med eventuelle genvejsmodifikationer. |
| `ui.text` | `text` | typed text | Indtast tekst gennem brugerfladen. |
| `ui.screenshot` | `path?` | PNG of the window (needs a presented frame: a minimised or fully covered window answers at once with `hidden: true` and an error; use `ui.render`) | Gem PNG af programvinduet; et skjult vindue kræver i stedet `ui.render`. |
| `ui.render` | `path`, `width?`, `height?`, `grid?`, `axes?` | Render of the model with the current camera (no window needed), drawn on the app's GPU device with the viewport's pipelines (the reply's `renderer`: `gpu`, or `gpu-software` on a software adapter), in the window's theme (light or dark; `solidcraft-cli snapshot` keeps the light colours); cut by the section the viewport shows (`inspect.section`, an open Section Analysis dialog's preview, a sketch's Slice), with the cut face capped. The ground grid and the origin's axes are drawn as the window shows them (`ui.set` `showGrid` and `showOrigin`, `hiddenOrigin`); `grid` and `axes` (booleans) say otherwise for this render | Render modellen fra det aktuelle kamera uden at vinduet skal være synligt. |
| `ui.confirm` | `accept?`: bool | answers the Delete confirmation (shown when a delete takes other features with it or makes some fail); without `accept`, what it lists | Bekræft eller afvis sletning og vis konsekvenser for afhængige funktioner. |
| `ui.messages` | `clear?`: `true` (empty the list), `open?`: bool (show or hide the Messages window) | every status message of this run, oldest first (the last 200; a message repeated in a row is one entry with a `count`): `messages` `[{text, error, at, count}]` (`at` in ms since the Unix epoch), and whether the window is `open` (#2763) | Læs eller ryd sessionens status- og fejlmeddelelser og vis eller skjul meddelelsesvinduet. |
| `ui.documents` | `action?`: `new`, `switch`, `close`, `quit`; `index?`; `force?` (close without asking); `answer?`: `save`, `dont_save`, `cancel` (answers "save changes?") | the open designs (tabs): names, unsaved flags, the active one, a pending "save changes?" prompt, whether a quit is asking | Opret, skift, luk eller afslut designfaner; håndter forespørgsler om ugemte ændringer. |
| `ui.file` | `path`; `insert?`: bool | starts an interactive background Open or Insert; poll `ui.inspect.file_busy` for completion; the window stays responsive | Start en interaktiv Open/Insert-filvælger i baggrunden. |
| `ui.home` | `open?`: bool; `sample?`: index | the start page: show or hide it, open a built-in sample (once built); returns `open` and the recent designs | Vis startsiden, seneste design eller en indbygget eksempelmodel. |
| `ui.shortcut` | `command`, `key?`, `replace?` | a command's key, or rebinds it (`""` clears; a key another command uses is refused unless `replace`) | Læs eller ændr genvejstasten for en kommando. |
| `ui.help` | `item?`: `about`, `shortcuts` (read only), `report` (opens a report draft for review), `crashes` (the Crash Reports window) | the version, commit, build date and diagnostic text | Åbn hjælpeindhold, genvejstaster, fejlrapport eller nedbrudsrapporter. |
| `ui.prefs` | any of `default_units`, `nav` (`fusion`, `solidworks`, `inventor`, `alias`, `blender`, `tinkercad`, `openscad`, `touchpad`), `zoom_reverse`, `pan_reverse_x` (default false: horizontal scrolling pans the other way), `zoom_to_cursor`, `smooth_wheel_zoom` (default true), `trackpad_pan` (default true: two-finger scroll pans, except Touchpad always zooms; pinch zooms), `orbit_around` (`view_centre` (default: what Pan moves and Fit frames), `origin` (the world origin stays still on screen), `cursor` (the model point under the pointer when the drag began); the older `orbit_cursor` boolean still sets `cursor` when true), `look_at_anim`, `look_at_speed`, `middle_double_click` (`home` (default), `fit`, `off`), `msaa`, `show_frame_rate` (default false: frames per second in the view's bottom-right corner; the view redraws every frame while it is on), `length_decimals`, `angle_decimals`, `theme` (`dark`, `light`, `system` or a theme file's id: a built-in one such as `gruvbox-dark`, or an imported one; the reply's `theme_ids` lists them), `theme_import` (a `solidcraft-theme/1` file's JSON or text: adds the theme and turns it on; the reply's `theme_imported` is its id), `theme_remove` (an imported theme's id), `theme_export: true` (the reply's `theme_file` is the theme on now as a complete file; see [colour themes](book/src/guide/ui-tour.md#colour-themes)), `language` (`en`, `ja`, `de`, `fr`, `ko`, `ms`, `ru`, `uk`, `zh`; see [localization](localization.md)), `ui_scale`, `ui_font` (an installed font family or a font file for the interface; empty for the built-in font), `up_axis` (`z`, `y`), `offer_stand_up`, `color_vision` (`off` (default), `protan`, `deutan`, `tritan`: colour vision modes, #778), `check_updates` (default true; [updates](book/src/guide/updates.md)); `open` (a page: General, Navigation, Keyboard Shortcuts, Design, Graphics, Sketch, Selection, AI) opens the Preferences dialog there; `button`: `ok`, `cancel`, `apply`, `defaults`; `stand_up: true` stands up a just-opened Y-up file, `stand_up: false` leaves it as it is ("Leave it": the offer closes and `offer_stand_up` stays on); the offer's buttons are the handles `stand_up:stand`, `stand_up:leave` and `stand_up:never` | sets preferences (kept between runs), returns them all plus `dialog` (the open page or null), `dark` and `stand_up_offer` | Læs eller ændr indstillinger som enheder, navigation, farver, zoom, snapping og kamerastyring. |
| `ui.capture` | `action?`: `capture`, `revert`, `cancel` | answers the Capture Position question (asked when a command starts or the design is saved while components were moved without capturing); without `action`, what waits | Gem, gendan eller annullér en ikke-gemt flytning af komponenter. |
| `ui.window` | `action`: minimize, maximize, restore, toggle, close | what the title bar's caption buttons do (on Windows and Linux the application bar is the title bar); close asks about unsaved designs first | Minimér, maksimér, gendan eller luk programvinduet. |
| `ui.agent_cursor` | `show` (bool), `speed`: a number from 1 to 5 (times the normal speed) or `"instant"` (no animation: the command runs at once and the pointer jumps to where it acted), `follow_camera` (bool); all optional | the settings. While the cursor is shown, `engine.execute` first glides to the command's toolbar button and to each pick, then runs and replies; later requests wait their turn. `"animate": false` on a call skips it | Vis og styr agentens markør, hastighed og eventuel kamerafølgning. |
| `ui.agent_chat` | `features?`, `toolbar_button?`, `permissions?` (`ask`, `edits`, `all`), `add_agent?` ({`name`, `kind`: `acp` or `api`, `command` or `base_url` + `model`}; no keys), `default_agent?`, `agent?` (the conversation's agent, by id), `mode?` (the agent's own mode), `model?` (one of its models), `open?`, `side?` (`left`, `right`), `minimized?`, `popped?` (in its own window), `width?`, `picker?`, `refresh?`, `connect?`, `stop?`, `attach?`: a file's path (a picture, a text file or a CAD file, for the next message; CAD files are imported at once), `remove_file?` (index of a file on the next message), `send?`: text (files alone make a message too), `attach_view?`, `permission?`: an option id or `"cancel"` (the newest question of the agent's own), `approve?`: true, false or `"always"` (the oldest request held for Allow/Deny), `cancel?` (Stop: the queue stays), `send_now?` (stop the running step, then send the queued messages), `remove_pending?` (index), `new_chat?` (clear the chat, new session), `undo?` (the last message's changes) | the chat's state: `features`, `agents` (found and added), `currentAgent`, `state` (off, starting, ready, busy, auth, exited), `popped`, `window` (its place and size), `agent`, `modes`, `mode`, `models`, `model`, `authMethods`, `toolsPort`, `pid`, `waiting`, `turn`, `pending` (queued messages, with their `files`), `attachments` (the files on the next message, by name), `loading` (pictures still being read for it, by name), `stopped`, `detection` and the conversation (`items`). With `features` off only the settings apply. Desktop only; see [Agent chat](agent-chat.md) | Læs eller ændr Agent Chat-indstillinger, tilslutning, modeller og tilladelser. |
| `ui.resize` | `width`, `height` | | Ændr vinduets bredde og højde. |
| `app.quit` | `force?` (quit without asking about unsaved designs) | `quitting` (the window closes), `asking` (a "save changes?" prompt is open) | Afslut programmet med eller uden spørgsmål om ugemte ændringer. |

`ui.workspace {workspace?: "design" | "render"}` læser eller ændrer det aktive arbejdsområde. Når du går til Render, skal den aktuelle modelleringsoperation afsluttes; når du vender tilbage, gendannes Design-kameraet. Se [det integrerede Render-arbejdsområde](render-workspace.md).

### Kontinuerlig kamerabevægelse

Kontinuerlig kamerabevægelse er også tilgængelig gennem `engine.execute` med kommandoen `view.navigate`. Angiv `camera` (fra `ui.inspect`) og `motion: "pan" | "orbit" | "zoom"`. Panorering og rotation bruger `delta: [dx,dy]` i skærmpunkter (højst ±10000 pr. akse), og panorering kræver desuden `viewport: [width,height]` med positive dimensioner. Zoom bruger en ændring af zoomniveauet. Brug de dokumenterede parametre og kontrollér svarets kamera.

## MCP

`solidcraft-cli mcp --connect 127.0.0.1:PORT` forbinder en MCP-klient (en AI-agent) til denne kontrolkanal. Se [mcp.md](mcp.md).

## Eksempel

```text
{"id":1,"method":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}}
{"id":2,"method":"engine.execute","params":{"command":"sketch.rectangle.two_point","params":{"p0":[0,0],"p1":[40,30]}}}
{"id":3,"method":"engine.execute","params":{"command":"solid.extrude","params":{"distance":20}}}
{"id":4,"method":"engine.execute","params":{"command":"solid.fillet","params":{"edges":[[0,0,10]],"radius":3}}}
{"id":5,"method":"ui.screenshot","params":{"path":"/tmp/part.png"}}
```

Under Preferences → Navigation → View navigator kan du vælge **Signed axes (akser med fortegn)** (oprindelig standard) eller **Cube (visningskube)**. Valget gemmes uafhængigt af museprofilen. Kontrolkanalen ændrer det med `ui.prefs {"navigator_style":"axes"}` eller `ui.prefs {"navigator_style":"cube"}`. Tidligere indstillinger uden dette felt bruger aksevisningen.

Hold Shift nede under rotation for at snappe til nærmeste standard- eller diagonalvisning. Det gælder højre træk, Orbit-værktøjet og træk i begge visningsnavigatorer. I standardmuseprofilen roterer Shift + midterknap også med snapping, mens midterknappen alene panorerer. Kubens flader vælger hovedvisninger, og hjørnerne vælger isometriske visninger.

Kuben er nedtonet, når den ikke bruges. Hold markøren over den for at fremhæve kube og navigationslinje og vise tilstødende kontroller. Den flade eller det hjørne, du peger på, får lyseblå udfyldning og blå kant. Når markøren flyttes væk, dæmpes visningen igen. Navigationsknapper fremhæves også ved hover og under navigation.

Kuben har seks fladeområder og otte hjørneområder, men ingen særskilte kantområder. Hjørnernes valgområder ligger på de seks plane flader, så kuben bevarer skarpe hjørner uden permanente opdelingslinjer. Når et hjørne fremhæves, fremhæves dets synlige dele på alle naboflader samlet.

Kuben vises i et kompakt kvadrat på 112 skærmpunkter i hjørnet af Viewport. En flade set direkte forfra er cirka 55 punkter bred. Tegning og hit-testing skaleres samlet. Tekstetiketterne strækkes lodret. Ved klik animeres skiftet mellem kameraretninger, hvis animation af visningsskift er slået til.

Kuben bruger `view.navigate` gennem kommandosystemet. Programmatiske kald sender `camera` og enten `direction: [x,y,z]` i kameraets opadaksekoordinater eller `action` (`left`, `right`, `up`, `down`, `clockwise`, `counterclockwise`, `home`, `fit`). Svaret indeholder `camera` og `fit`. UI-kald anvender samme kommando og animerer efter de valgte indstillinger.

Shift-Orbit-snapping inkluderer kantvisninger ved 45 grader og isometriske hjørnevisninger med en let dæmpet overgang og en lille margin, så visningen ikke flimrer.

## Ordliste – kontrolprotokollens engelske udtryk

De tekniske navne i kode og JSON er uændrede.

| Engelsk udtryk | Dansk betydning |
|---|---|
| Control Protocol | Kontrolprotokol |
| Request | Forespørgsel |
| Reply / Response | Svar |
| Token | Adgangstoken |
| Authenticate | Godkend forbindelse |
| Timeout | Tidsgrænse overskredet |
| Pending | Afventende |
| Busy | Optaget |
| Untrusted Input | Upålideligt input |
| Allowed Paths | Tilladte filstier |
| Engine | Kommandosystem |
| Batch | Gruppe af kommandoer |
| Rollback | Tilbagerulning |
| UI | Brugerflade |
| Viewport | Visningsområde |
| Pointer | Musemarkør |
| Screenshot | Skærmbillede |
| Render | Render / tegn model |
| Camera | Kamera |
| Pan | Panorér |
| Orbit | Rotér visning |
| Zoom | Zoom |
| Yaw | Vandret kameravinkel |
| Pitch | Lodret kameravinkel |
| Roll | Kamerarotation om synsaksen |
| Fit | Tilpas udsnittet |
| Selection | Markering |
| Tooltip | Hjælpetekst |
| Marking Menu | Radialmenu |
| Navigator Cube | Visningskube |
| Signed Axes | Akser med fortegn |
| Snap | Fastgør visningsretning |
| Frame | Tegnet skærmbillede |

<!-- docs-english-updates:start -->

## Engelske ændringer, der mangler oversættelse

> Nedenfor står nye og ændrede afsnit fra den engelske original. Arbejd dem ind i oversættelsen, kontrollér den aktuelle engelske side, og fjern derefter hele denne markerede sektion.

The English original also removes or replaces text. Review the corresponding translated text.

## Engine

| Method | Params | Result |
|---|---|---|
| `engine.execute` | `command`: command id, `params`: JSON object | the command's result |
| `engine.script` | `commands`: `[{command, params}, …]` | results, stops at the first error |
| `engine.batch` | `commands`: `[{command, params}, …]` (at most 10 000), `rollback?`: bool (default true), `timings?`, `profile?`: bool (default false) | `{completed, results, undone}`, plus `failed: {index, command, error}` when a step fails, and with `timings` `timings: [{index, command, ms}]` (the failing step included) and `total_ms`; `profile` adds each step's `profile: {boolean_ms, tessellate_ms, other_ms}`. Runs the steps in one request, so within one frame, each as `engine.execute` runs it. The first failure stops the batch, and with `rollback` the design goes back to how it was before it (`undone`: the steps taken back). Refused before anything runs, with an error starting `engine.batch unavailable`, while the agent cursor is shown or when a step loads a file (`doc.open`, `file.insert_step`, `file.insert_mesh`). Send such steps one by one. Answered when done, for up to 3600 s |
| `engine.commands` | — | every command: id, label, tab, panel, icon, shortcut, params doc, enabled |
| `document.inspect` | `measure?`: bool | parameters, timeline (errors, timings), bodies, sketches, selection (assembly joints: `engine.execute` `joint.inspect {joint?}`, which reads only: each joint's two origins with the occurrence, the snap as saved, the feature, face, edge or sketch curve its names refer to, and whether it resolves or why not; kind, offset, angle, flip, values, limits, expressions and motion links) |

<!-- docs-english-updates:end -->
