# Kom godt i gang

Skrivebordsprogrammet hedder `solidcraft`, kommandolinjeværktøjet (CLI) hedder `solidcraft-cli`, og designfiler gemmes som `.solidcraft`. Designfiler, indstillinger og opsætninger fra versioner udgivet under programmets tidligere navn fungerer fortsat (se [kompatibilitetsnoterne](https://github.com/bherbruck/solidcraft/blob/main/docs/compatibility.md)).

## Skrivebordsprogram

Hent installationsfilen til dit system fra [downloadsektionen i README](https://github.com/bherbruck/solidcraft#download) eller [udgivelsessiden](https://github.com/bherbruck/solidcraft/releases). Du kan også selv bygge programmet med Rust stable (1.95 eller nyere):

```sh
git clone https://github.com/bherbruck/solidcraft && cd solidcraft
cargo run --release -p solidcraft -- --sample   # opens with a sample part
```

`solidcraft part.step` åbner en STEP-, IGES-, 3MF-, STL- eller OBJ-fil som et nyt design, mens `solidcraft design.solidcraft` åbner et gemt design. Windows-versioner kan krydskompileres fra Linux med `cargo xwin build --release --target x86_64-pc-windows-msvc -p solidcraft`.

### macOS

Åbn udgivelsens `.dmg`-fil, og træk `SolidCraft.app` til Programmer (Applications).

Hvis du opdaterer fra en version udgivet under det tidligere produktnavn, skal du følge [opgraderingsvejledningen til macOS](https://github.com/bherbruck/solidcraft/blob/main/docs/compatibility.md#macos), så den eksisterende installation erstattes i stedet for at installere en ekstra kopi.

### Linux AppImage

Første gang du åbner AppImage-filen, opretter SolidCraft en menugenvej og ikoner under `~/.local/share` (`applications/ai.storyteller.solidcraft.desktop`, `icons/hicolor`), så programmet vises med korrekt navn og ikon. Der tilføjes ikke noget, hvis [appimaged](https://github.com/probonopd/go-appimage) allerede har oprettet en genvej, eller hvis en launcher som Gear Lever eller AppImageLauncher starter filen. I sidstnævnte tilfælde fjernes en genvej, som SolidCraft tidligere har oprettet.

Du kan deaktivere integrationen permanent ved at oprette en tom fil på `~/.local/share/solidcraft/no-desktop-integration` eller starte AppImage med `SOLIDCRAFT_NO_DESKTOP_INTEGRATION=1`.

### Linux Flatpak

Installér Flatpak via din distributions pakkehåndtering. For at modtage opdateringer skal du tilføje Flathub (til den nødvendige Freedesktop-runtime) og SolidCrafts Flatpak-kilde til samme brugerinstallation:

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak remote-add --user --if-not-exists solidcraft https://docs.solidcraft.io/flatpak/solidcraft.flatpakrepo
flatpak install --user solidcraft ai.storyteller.solidcraft
flatpak run ai.storyteller.solidcraft
```

Hvis du tilføjede SolidCrafts Flatpak-kilde før flytningen fra `bherbruck.github.io` til `docs.solidcraft.io`, finder den ikke længere opdateringer. Ret adressen med `flatpak remote-modify --user --url=https://docs.solidcraft.io/flatpak/repo/ solidcraft`, og kør derefter `flatpak update --user`.

Versioner til og med 0.7 brugte program-id'et `ai.storyteller.solvecraft`. Hvis du installerede en af disse versioner fra SolidCrafts Flatpak-kilde, vil `flatpak update --user` meddele, at versionen er udgået, og tilbyde at erstatte den med `ai.storyteller.solidcraft`. Acceptér, så indstillinger og automatisk gemte filer følger med. Hvis du installerede en `.flatpak`-pakke direkte, skal du installere den nye pakke, starte den én gang (så indstillingerne kopieres) og derefter køre `flatpak uninstall --user ai.storyteller.solvecraft` (se [kompatibilitetsnoterne](https://github.com/bherbruck/solidcraft/blob/main/docs/compatibility.md#linux-app-id)).

Du kan også hente udgivelsens `.flatpak`-fil til din arkitektur og køre `flatpak install --user ./solidcraft-VERSION-linux-ARCH.flatpak`. Pakken indeholder adressen til Flathubs runtime-kilde, men installationen kræver stadig internet for at hente manglende runtimes. Start med `flatpak run ai.storyteller.solidcraft` i stedet for at lede efter en `solidcraft`-fil på værten. Filer valgt via Åbn/Gem håndteres gennem skrivebordets filvælgerportal. Kommandolinjestier uden for Dokumenter kan kræve ekstra filsystemrettigheder.

### Linux Snap

På Ubuntu og andre distributioner med snapd:

```sh
sudo snap install solidcraft
solidcraft                # or open SolidCraft from the app menu
solidcraft.cli --version  # the CLI, for scripts and `solidcraft.cli mcp`
```

Snap-versionen kan åbne designfiler i din hjemmemappe (dog ikke skjulte mapper) og andre steder via filvælgeren. Giv adgang til USB-drev og andre monterede diske ved at køre `sudo snap connect solidcraft:removable-media` én gang. En udgivelses `.snap`-fil kan installeres uden Snap Store med `sudo snap install --dangerous ./solidcraft-VERSION-linux-ARCH.snap`; den opdateres så ikke automatisk.

## Web

Samme program kan bruges i browseren: [Åbn SolidCraft på nettet](../app/). Parameteren `?sample` åbner et eksempel-emne, og `?webgl` tvinger WebGL2, hvis WebGPU mangler eller giver problemer. Browseren husker valget indtil `?webgpu` bruges; det gælder også en app installeret fra Chrome eller Edge. Designfiler gemmes i browserens private fillager, som kræver HTTPS eller localhost.

Den planlagte integration med workers og lagdelt datalagring er beskrevet i [web-runtime.md](https://github.com/bherbruck/solidcraft/blob/main/docs/web-runtime.md). Den er endnu ikke integreret i den udgivne wasm-version.

Efter det første besøg fungerer webappen offline, fordi browseren gemmer en kopi. Når du er online, henter browseren den nyeste udgivelse ved opstart, hvis den er ændret. Hvis du er offline, netværket ikke svarer inden for få sekunder, eller en ny version ikke kan downloades helt, starter den gemte kopi. Downloaden fortsætter til næste opstart.

Browsere, der understøtter installation af webapps, viser **Install app… (Installér app)** i menuen File (Filer). Det installerer SolidCraft med eget vindue og programikon og giver samme offlinefunktion som websiden. Chrome, Edge og andre Chromium-browsere viser browserens installationsdialog; når appen er installeret, forsvinder menupunktet. Safari har ikke denne dialog, så menupunktet viser i stedet vejledningen: Share › Add to Home Screen (Del › Føj til hjemmeskærm) på iPhone og iPad eller File › Add to Dock (Arkiv › Føj til Dock) på Mac med Safari 17 eller nyere.

Byg selv webappen:

```sh
rustup target add wasm32-unknown-unknown
cd apps/solidcraft-web && trunk serve --release --no-autoreload
```

Offlinebrug kræver `--no-autoreload`: Trunks automatiske genindlæsning ændrer den serverede side, så den ikke længere svarer til den version, browseren skal gemme. Debug-builds med `trunk serve` gemmer aldrig en offlinekopi.

### Selvhosting med Docker

Webappen findes også som containerimage, `ghcr.io/tangency-labs/solidcraft-web`, til Linux på x86_64 og ARM64. `latest` er den nyeste udgivelse, `0.14.0` eller `0.14` vælger en udgivelse, og `edge` følger den nyeste `main`. nginx serverer appen på port 8080. Serveren gemmer ikke designfiler; de bliver i hver besøgendes browser.

```sh
docker run -d --name solidcraft -p 8080:8080 --restart unless-stopped ghcr.io/tangency-labs/solidcraft-web:latest
```

Du kan også bruge Docker Compose med [`packaging/docker/compose.yaml`](https://github.com/tangency-labs/solidcraft/blob/main/packaging/docker/compose.yaml). `SOLIDCRAFT_PORT` vælger en anden port:

```sh
docker compose -f packaging/docker/compose.yaml up -d      # start
docker compose -f packaging/docker/compose.yaml pull && docker compose -f packaging/docker/compose.yaml up -d   # update
docker compose -f packaging/docker/compose.yaml up -d --build   # build the image from your checkout instead
```

Åbn derefter `http://localhost:8080/`. Fra en anden computer gemmer browseren kun designfiler over HTTPS. Brug derfor en reverse proxy med certifikat, eksempelvis Caddy, Traefik eller nginx. Appen fungerer under enhver URL-sti, f.eks. `https://example.com/cad/`. Du kan bygge imaget uden Compose fra repositoryets rodmappe med `docker build -f packaging/docker/Dockerfile -t solidcraft-web .`.

### 3D-mus i browseren

Webappen understøtter 3Dconnexion SpaceMouse og SpaceNavigator via browserens Gamepad API, med samme aktivering og hastighed under Preferences › Navigation (Indstillinger › Navigation) som i skrivebordsprogrammet. Edge og Chrome registrerer enheden direkte. Browseren viser først en gamepad til en side, når den er blevet bevæget. Hvis intet sker, skal du bevæge knoppen tydeligt én gang.

3DxWare kan også omsætte knappens bevægelse til zoom med musehjulet. Når knoppen bevæges, ignorerer webappen musehjulet og læser knoppen direkte. Før browseren har registreret den, kan den første bevægelse derfor give en lille zoomændring.

Firefox registrerer ikke 3D-mus direkte ([Mozilla bug 1226575](https://bugzilla.mozilla.org/show_bug.cgi?id=1226575)). På Windows kan Firefox dog registrere 3DxWares virtuelle joystick, som kan overføre knappens bevægelser. Gem følgende som `%APPDATA%\3Dconnexion\3DxWare\Cfg\Firefox.xml`, og genstart Firefox (og 3DxWare, hvis knoppen stadig kun zoomer). Ændringen gælder kun Firefox: Her sendes knappens seks akser til joysticket i stedet for musehjulet. Slet filen for at fortryde.

```xml
<?xml version="1.0" encoding="UTF-8"?>
<AppCfg Default="false" xmlns="" CfgFormatVersion="1.3" ThisFileVersion="1.1">
  <AppInfo>
    <Signature>
      <Name>STR_FIREFOX</Name>
      <ExecutableName>FIREFOX.EXE</ExecutableName>
      <Transport>KMJ</Transport>
    </Signature>
    <Options/>
  </AppInfo>
  <CfgProperties>
    <ID>ID_STR_FIREFOX</ID>
    <Name>STR_FIREFOX</Name>
    <InheritsFromID>ID_STR_FIREFOX</InheritsFromID>
  </CfgProperties>
  <Devices>
    <Device>
      <ID>ID_Standard_3D_Mouse</ID>
      <AxisBank>
        <Name>STR_DEFAULT_BANK</Name>
        <ID>Default</ID>
        <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_X</ActionID><Min>-512</Min><Max>511</Max><Deadband>0</Deadband></Input><Output><ActionID>HIDJoystick_X</ActionID></Output></Axis>
        <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Y</ActionID><Min>-512</Min><Max>511</Max><Deadband>0</Deadband></Input><Output><ActionID>HIDJoystick_Y</ActionID></Output></Axis>
        <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Z</ActionID><Min>-512</Min><Max>511</Max><Deadband>0</Deadband></Input><Output><ActionID>HIDJoystick_Z</ActionID></Output></Axis>
        <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Rx</ActionID><Min>-512</Min><Max>511</Max><Deadband>0</Deadband></Input><Output><ActionID>HIDJoystick_Rx</ActionID></Output></Axis>
        <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Ry</ActionID><Min>-512</Min><Max>511</Max><Deadband>0</Deadband></Input><Output><ActionID>HIDJoystick_Ry</ActionID></Output></Axis>
        <Axis><Enabled>true</Enabled><Input><ActionID>HIDMultiAxis_Rz</ActionID><Min>-512</Min><Max>511</Max><Deadband>0</Deadband></Input><Output><ActionID>HIDJoystick_Rz</ActionID></Output></Axis>
      </AxisBank>
    </Device>
  </Devices>
</AppCfg>
```

## CLI

`solidcraft-cli` udfører de samme kommandoer uden at åbne et vindue:

```sh
cargo run --release -p solidcraft-cli -- commands                       # every command, as JSON
cargo run --release -p solidcraft-cli -- run examples/bracket.json --out bracket.step
cargo run --release -p solidcraft-cli -- eval examples/bracket.json     # volume, area, faces…
cargo run --release -p solidcraft-cli -- snapshot examples/bracket.json --out bracket.png
cargo run --release -p solidcraft-cli -- exec solid.box '{"length": 20, "width": 10, "height": 5}'
cargo run --release -p solidcraft-cli -- run mechanism.json --in arm.solidcraft  # with a motion.export_animation step: a GIF
```

Et script er en liste over kommandoer:

```json
{"commands": [
  {"command": "sketch.create", "params": {"plane": "XY"}},
  {"command": "sketch.rectangle.two_point", "params": {"p0": [0, 0], "p1": [40, 30]}},
  {"command": "sketch.finish", "params": {}},
  {"command": "solid.extrude", "params": {"distance": 20}},
  {"command": "solid.fillet", "params": {"edges": [[0, 0, 10]], "radius": 3}}
]}
```

![Eksempel på beslag, renderet med solidcraft-cli snapshot](../generated/bracket.png)

`snapshot` viser som standard modellen isometrisk (**Isometric View**, isometrisk visning). `--view NAME` vælger en anden standardvisning (`front`, `back`, `top`, `bottom`, `left`, `right`) eller en visning gemt med `view.save`. `--camera JSON` angiver en vilkårlig kameraposition, ligesom MCP-værktøjet `screenshot` (se [MCP-vejledningen](mcp.md#camera-and-close-up-recipe)). Et script, der bruger `view.apply`, indstiller også kameraet.

Grundgitteret ligger i Z = 0 og kan derfor skære gennem et emne, der strækker sig på begge sider af planet. `--no-grid` skjuler gitteret, og `--no-axes` skjuler koordinatakserne. Billedet renderes på GPU med de samme pipelines som programmets visningsområde og viser derfor også overfladeanalyser som `inspect.zebra`. Uden en brugbar fysisk GPU kan SolidCraft bruge en softwarebaseret GPU (Mesa lavapipe på Linux eller WARP på Windows). Hvis heller ikke den er tilgængelig, bruges CPU-rendering uden farver fra overfladeanalyser; resultatet angiver `renderer: cpu` med en advarsel. `SOLIDCRAFT_GPU=auto|hardware|software|cpu` vælger rendering. `hardware` og `software` fejler i stedet for at falde tilbage til CPU. `solidcraft-cli gpu` viser tilgængelige enheder og den renderingsklasse, et snapshot vil bruge.

```sh
solidcraft-cli snapshot part.solidcraft --out photo.png --view Photo
solidcraft-cli snapshot part.solidcraft --out close-up.png --no-grid --no-axes --camera '{"target": [30, -20, 0], "yaw": 0.6, "pitch": 0.25, "distance": 60}'
```

### Se, hvad Cut eller Join vil gøre

Et billede kan hurtigt vise, om et værktøjsemne går gennem materialet og er placeret korrekt, før du udfører **Cut (udskæring)** eller **Join (sammenføjning)**. `--body NAME:opacity=0.3` gør emnet gennemsigtigt, så dets indre bliver synligt. `--body NAME:color=#cc3333` giver det en farve. Hvis du kun angiver én egenskab, bevares den anden fra designet. Indstillingerne svarer til `appearance.assign`, som bruges i Appearance-dialogen. Snapshot viser derfor samme udseende som programvinduet.

`--tool NAME` viser det valgte værktøjsemne solidt i værktøjsfarven, mens de øvrige emner er gennemsigtige. `--section z=27` laver et **Section (snit)** gennem modellen, så indersiden bliver synlig, ligesom `inspect.section`. Angiv en akse og en koordinat, tilføj `,flip` for at beholde den modsatte side, vælg `XY`, `XZ` eller `YZ` for et plan gennem origo, eller brug kommandoens JSON-format.

![En tap inde i en gennemsigtig plade, gennemskåret af et snitplan](../generated/tool-preview.png)

```sh
# The pin the cut would remove, inside the plate it stops 6 mm short of.
solidcraft-cli snapshot plate.solidcraft --out preview.png --tool Pin
solidcraft-cli snapshot plate.solidcraft --out cut.png --tool Pin --section z=12
solidcraft-cli snapshot plate.solidcraft --out glass.png --body "Plate:opacity=0.25" --body "Pin:color=#1a7f4b"
```

Et emne med gennemsigtigt udseende gengives gennemsigtigt overalt, hvor der renderes billeder. Et udseende med opacitet under 1, f.eks. `Glass - Clear` eller en selvvalgt `opacity`, vises som glas i `snapshot`, MCP-skærmbilleder og designets miniaturebillede, ligesom i programvinduet. En flade med sit eget gennemsigtige udseende vises stadig med emnets opacitet.

Windows PowerShell 5.1 fjerner dobbelte anførselstegn inde i argumenter, der sendes til et program. Skriv dem som `\\"` dér.

Hvis en kommando fejler, stopper scriptet. En **Feature (modelleringsfunktion)** kan også fejle senere, når en parameterændring genberegner den. I så fald afsluttes `run` med statuskode 1, de fejlende funktioner vises, og der skrives ingen `--out`- eller `--save`-filer, fordi modellen mangler den geometri, funktionerne skulle have skabt. `--allow-errors` skriver filerne alligevel.

`run` udskriver hver kommandos resultat med `--results` (ellers måleresultatet), mens `eval` udskriver måleresultatet som JSON til stdout. `--json-out FILE` gemmer i stedet JSON i en fil. Filen er almindelig UTF-8. På Windows kan PowerShells `>` ændre kodningen til UTF-16 eller UTF-8 med BOM, afhængigt af indstillingerne. Strenge JSON-læsere som Pythons `json.load` kan afvise sådanne filer.

```sh
cargo run --release -p solidcraft-cli -- run examples/bracket.json --results --json-out results.json
cargo run --release -p solidcraft-cli -- eval examples/bracket.json --json-out measures.json
```

Brug `--timings` til at finde langsomme trin i et script. Den viser varigheden af hver kommando i stedet for måleresultatet: `{"timings": [{"index", "command", "ms"}], "total_ms"}`, hvor `index` starter ved 0. `--profile` tilføjer hver kommandos `profile: {"boolean_ms", "tessellate_ms", "other_ms"}`, dvs. tid brugt på booleske operationer i geometrikernen, tessellering og øvrige operationer. Sammen med `--results` ligger resultaterne i samme objekt under `results`; `--results` alene giver stadig et almindeligt array.

```sh
cargo run --release -p solidcraft-cli -- run examples/bracket.json --profile
```

Et designs historik kan eksporteres som et script, der genopbygger modellen. Det gør det muligt at reproducere fejl, der kun opstår i en bestemt designfil:

```sh
cargo run --release -p solidcraft-cli -- script part.solidcraft --out repro.json
cargo run --release -p solidcraft-cli -- eval repro.json               # the same part
```

Scriptet begynder med brugerparametre (`parameters.add`) og designets enheder, regler for pladekonstruktion og plast samt materialer. Derefter tilføjes hver funktion på **Timeline (tidslinjen)** med `timeline.add`, præcis som i designfilen: samme ID'er, navne og parameternavne. Efterfølgende trin kan derfor finde de skitser og emner, de bruger, og en funktion, der fejlede i designet, fejler på samme måde i scriptet. Du kan ændre værdier i et trin eller slette efterfølgende trin for at se modellen på et tidligere tidspunkt.

Udseender, mapper, navngivne visninger og konfigurationer udelades (CLI'en oplyser hvilke). Design med komponenter eller ledforbindelser understøttes endnu ikke. I programmet og via MCP kan det samme script genereres med `timeline.export_script`.

Standardmåleenhederne er millimeter og grader, medmindre et udtryk angiver noget andet (`"1 in"`, `"30 deg"`). Z-aksen peger opad.

## Navn og kompatibilitet

SolidCraft havde tidligere et andet arbejdsnavn. Siden version 0.5.0 bruges det nuværende navn overalt: programmerne `solidcraft` og `solidcraft-cli`, designfiler med `.solidcraft`, miljøvariabler med `SOLIDCRAFT_*`, indstillingsmapper og MCP-serveren `solidcraft`. Ved opgradering bevares kompatibiliteten med designfiler, indstillinger, gendannelsesfiler, miljøvariabler og CLI/MCP-opsætninger fra tidligere versioner. Alle tilpasninger er beskrevet i [kompatibilitetsnoterne](https://github.com/bherbruck/solidcraft/blob/main/docs/compatibility.md).

Brug SolidCrafts nuværende dokumentations- og webappadresser. GitHub viderestiller links til repositoryets tidligere navn, men de gamle GitHub Pages- og Flatpak-adresser vedligeholdes ikke som permanente alternativer.
