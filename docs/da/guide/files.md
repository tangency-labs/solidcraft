# Import and Export (import og eksport)

| Format | Læsning | Skrivning |
|---|---|---|
| `.solidcraft` (versionsstyret container eller JSON-design) | Ja | Ja |
| `.json` (selvstændigt design i ren JSON) | Ja | Ja |
| STEP (AP203, AP214, AP242) | Solide emner, samlinger, enheder, navne og farver | AP242 med navne, farver og komponenthierarki |
| IGES 5.3 | Lukkede solide emner eller trimmede overflader samlet til solide emner | Solide emner med navne og farver |
| FreeCAD `.FCStd`, Rhino `.3dm` | Solide emner via cadmpeg (kun desktop) | — |
| 3MF, STL, OBJ | Mesh-emner | Ja |
| glTF 2.0 (`.glb`, `.gltf` med tilhørende `.bin`-filer) | Mesh-emner som komponenter i nodehierarkiet; materialer som udseender | — |
| 3MF med fladetags | Som 3MF | Hver trekant får et tag for sin B-rep-flade og præcise koordinater |
| URDF | — | Samling som robot med links, mesh, masse, inerti og Joints |
| DXF, SVG | Til en skitse | Skitser og udfoldninger som DXF |
| PNG, JPEG | Canvases og decals | — |

Når du åbner en STEP-, IGES-, FreeCAD- eller Rhino-fil, oprettes et design med filens emner som en Base Feature. Indsætter du filen, tilføjes emnerne i det aktuelle design. Senere Features kan bygges oven på dem. Mesh-emner kan vises, måles, flyttes og eksporteres, men solidmodellering kræver B-rep-emner.

Hver importeret STEP-, IGES-, 3MF-, STL-, OBJ- eller glTF-fil kommer ind som en ny komponent opkaldt efter filen, så dens emner ikke blandes med dine egne. STEP-samlinger bevarer deres struktur: dele og undersamlinger bliver indlejrede komponenter. En del brugt flere gange er én komponent med en forekomst for hver placering. Den importerede samling er Rigid (fast): Flyttes dens komponent, følger alt med, men delene kan ikke bevæges enkeltvis. Højreklik på komponenten og vælg **Unlock Contents (frigør indhold)** for at kunne oprette Joints mellem delene. Undo fjerner hele importen i ét trin.

glTF-filer (`.glb` eller `.gltf` med de tilhørende `.bin`-filer i samme mappe) importeres på samme måde. Noder bliver indlejrede komponenter, og gentagne mesh får en forekomst for hver placering. Hele importen er fastlåst, indtil indholdet frigøres. glTF bruger meter og Y-opad, så importen skaleres til millimeter og drejes til Z-opad (`up: "z"` bevarer filens akser). Materialernes grundfarve, metallic og roughness bliver emnets udseende i Render. Teksturer, skins og animationer importeres ikke. Skalerede eller spejlede noder omregnes til deres mesh. Komprimerede mesh (Draco, meshopt) afvises med besked om at gemme filen ukomprimeret først.

Et 3MF-projekt gemt af en slicer kan også importeres, inklusive hvis objekter er organiseret i separate modeldele med 3MF Production-udvidelsen. Hvert objekt på byggepladen bliver et mesh-emne med sine dele på de placeringer, sliceren har angivet.

**File → Save as Mesh… (gem som mesh)** findes også på fanen Mesh under Modify og i emners kontekstmenu. Den gemmer valgte emner som STL, 3MF eller 3MF med fladetags. Med **Send to Slicer (send til slicer)** aktiv gemmes filen i en midlertidig mappe og åbnes i den valgte slicer i stedet for at spørge om filnavn. Listen viser fundne slicere med ikoner. **Custom…** tilføjer et andet program øverst. Tilføjelsen huskes mellem opstarter og kan fjernes ved at pege på den og trykke Delete.

Lukkede mekaniske mesh med konstant tværsnit kan rekonstrueres til solide emner med `mesh.convert` via Command Palette, scripts eller kontrolkanalen.

**File → Save as Mesh…** med **3MF with Face Tags** (`file.export` eller `file.save_mesh` med `format: "3mf-tagged"`) gemmer en almindelig 3MF, hvor hver trekant også angiver sin B-rep-flade, og hver koordinat bruger tesselleringens præcise double-værdi. Mesh- og analyseværktøjer kan bruge tags til at bevare fladeidentitet, f.eks. ved belastninger, randbetingelser eller Feature-kanter. Andre 3MF-læsere ignorerer dem. Formatet beskrives i [`docs/formats/3mf-face-tags.md`](https://github.com/bherbruck/solidcraft/blob/main/docs/formats/3mf-face-tags.md).

**File → Export URDF…** (`file.export` med en `.urdf`-sti) eksporterer samlingen til robotsimulatorer og ROS. Hver komponentforekomst bliver et link med mesh, masse og inertimoment samt samlingens Joints, så delene kan bevæge sig som i SolidCraft. Mesh gemmes som STL ved siden af `.urdf`-filen. Se [`docs/urdf.md`](https://github.com/bherbruck/solidcraft/blob/main/docs/urdf.md).

Lagring er atomisk: Et nedbrud efterlader enten den gamle eller den nye fil. Den erstattede version gemmes som `<file>.bak`. Filrettigheder bevares, og hvis et design åbnes gennem et symbolsk link, gemmes ændringerne i målet, så linket bliver ved med at virke. Ugemte ændringer gemmes automatisk i en Recovery-mappe, og ved næste opstart tilbydes gendannelse. Ældre designfiler opgraderes ved åbning; nyere, ikke-understøttede filformater afvises med en meddelelse.

**Save (gem)** skriver som standard en containerfil. Filer navngivet `.json` bevarer JSON-formatet ved senere lagringer. **Save As (gem som)** tilbyder container eller ren JSON på desktop. **Save As JSON** findes også i File-menuen på desktop og web; vælg filendelsen `.json`. **Export Design as JSON** opretter en separat selvstændig kopi uden at ændre den aktuelle fil. Scripts kan angive `"format": "json"` til `file.save` eller `file.save_as`, også selv om filnavnet ender på `.solidcraft`. Åbning genkender formatet ud fra indholdet, så ældre JSON-design fortsat virker. Containere prioriterer hurtig læsning og lagring; scripts kan vælge `"compression": "compact"`, hvis pladsbesparelse er vigtigere end CPU-tid.

Containerfiler fjerner dubletter af identiske STEP-, mesh- og billeddata. Mesh-koordinater bevarer deres oprindelige double-præcision; de konverteres ikke til STL. Geometrien genopbygges stadig fra Timeline ved åbning. Hvis et gemt preview er tilgængeligt, viser desktopprogrammet det under kontrollen af geometrien; redigering begynder efter fuld genopbygning. Cancel bevarer det aktuelle design. Preview kan ikke redigeres eller måles. Scripts kan på varme lagringer aktivere en midlertidig geometricache med `cache: true`. Lagring genberegner aldrig kold geometri kun for at oprette en cache. Normale lagringer og browserdownloads bruger som standard `cache: false`. En ekstra cache til visningsmesh kan vælges med `preview: true`. Ikke-understøttede eller forkerte caches ignoreres. Browserappen genopbygger stadig synkront. Varme lagringer med `cache: true` kan indeholde et lille PNG-miniaturebillede, så Home kan vise seneste design uden genberegning. Manglende eller ugyldige billeder genereres som normalt. `cache: false` udelader også miniaturebilledet. Se `docs/design-container.md` i repositoryet for begrænsninger og kodningsdetaljer.

Design gemmes i det nyeste format, den aktuelle SolidCraft-version kender (`"format": "solidcraft/N"` i filen). Alle ældre formater kan åbnes og opgraderes. Hvis filen kommer fra en nyere version, bliver du bedt om at opdatere i stedet for at programmet gætter. Nightly-udviklingsversioner gemmer sommetider i formater, som ingen stabil version endnu kan åbne.

Sketch Patterns bevarer referencer til deres retningslinjer og placeringsindstillinger ved lagring. Ældre mønstre beholder deres faste retningsvektorer og tidligere standardplaceringer.

Opgradering bevarer designets oprindelige betydning. Gemte overførsler af emner, koordinatsystemer for eksplicitte 3D-kurver og certificerede fladetilknytninger bevares sammen med konfigurationsstile, associative friforms-Offsets og cylindriske Emboss-valg. Ældre ejerskabsrelationer, geometri og udtryk beholder deres betydning. Gamle 3D-kurver med bevarede tilknytninger får producentens koordinatsystem-markør uden ændring af punkterne. Frigjorte kurver beholder verdenskoordinaterne. En mistet reference mellem komponenter efter en ældre opdatering kan kræve manuel genlinkning, når kilden er tilgængelig. Indlæsning gætter aldrig på numeriske rettelser. Associationsdata, koordinatlister og placeringer kontrolleres og begrænses.

## Design Properties (designegenskaber)

**File → Design Properties…** findes også under **Document Settings** i Browser og i rodkomponentens højreklikmenu. Her registreres designets projekt, varenummer, revision, forfatter, organisation, beskrivelse og licens. **OK** anvender ændringerne som ét Undo-trin. Scripts og agenter bruger `document.metadata` til at læse dem, `document.metadata.set` (med `key` og `value` eller `values: {key: value, …}`) til at ændre dem og `document.metadata.remove` (med `key` eller `keys`) til at fjerne dem. `document.inspect` og MCP-værktøjet `inspect_design` inkluderer dem.

Eksporterede filer medtager oplysningerne, hvor formatet giver plads:

| Format | Oplysninger, der skrives |
|---|---|
| STEP | `author` og `organization` under `FILE_NAME`, `description` i `FILE_DESCRIPTION` (én linje) |
| IGES | `author` og `organization` i den globale sektion |
| 3MF | `project` som `Title`, `author` som `Designer`, `description` som `Description`, `license` som `LicenseTerms` |

Varenummer og revision har ikke et standardfelt i disse formater og forbliver derfor i designfilen.

I filen findes oplysningerne som ét valgfrit `metadata`-objekt på øverste niveau. Uden egenskaber udelades objektet, og designet gemmes som før:

```json
"metadata": {
  "project": "ACME enclosures",
  "part_number": "ACME-1042",
  "revision": "B",
  "author": "Jane Doe",
  "organization": "ACME",
  "description": "Vented case for Raspberry Pi 4",
  "license": "CC-BY-4.0",
  "com.acme.description": { "shop_id": 88123 },
  "org.printfarm": { "slicer_profile": "PETG-0.2", "internal_id": 4 }
}
```

- **SolidCrafts egne nøgler har intet præfiks** og indeholder tekst. Alle andre nøgler uden præfiks er reserveret til fremtidige versioner.
- **Andre værktøjer opbevarer deres data under én reverse-DNS-nøgle**, f.eks. `com.example.tool`, med mindst to punktumadskilte led med bogstaver, tal, `-` eller `_`. Værdien skal være et objekt. Hvert værktøj skriver kun under sin egen nøgle.
- **SolidCraft bevarer alle nøgler uændret** ved åbning, redigering og lagring i både container og JSON og kontrollerer ikke værdier fra andre værktøjer. Et velkendt felt med forkert datatype, f.eks. `"author": 42`, bevares, men læses som manglende og forhindrer ikke åbning. Redigering i Design Properties erstatter værdien.
- **Andre værktøjers nøgler eksporteres ikke**, men vises skrivebeskyttet under Design Properties.
- **Hold værdierne små.** `document.metadata.set` afviser tekst over 4 KiB, objekter over 64 KiB, indlejring over 32 niveauer og samlet mere end 256 nøgler eller 256 KiB. Gem binære data i en separat fil ved siden af designet.

Designegenskaber gemmes første gang i format 21. Indsættelse eller linkning af andre design kopierer ikke deres egenskaber. Save As bevarer dem. Et kommandoscript fra designet (`timeline.export_script`) indstiller dem i ét trin.

## Gem design i browserappen

I Chrome, Edge og Opera åbner **Save As (gem som)** browserens gemmedialog og skriver `.solidcraft`-filen i den mappe, du vælger. **Open (åbn)** åbner browserens filvælger. Derefter gemmer **Save (gem)** tilbage i samme fil. Browseren husker filen. Når siden er genindlæst, beder den første Save én gang om tilladelse til at skrive i filen og fortsætter derefter. Et design, du åbner fra computeren, gemmes tilbage i den fil, du åbnede.

Firefox og Safari kan ikke skrive direkte i en valgt mappe. Her gemmer **Save** og **Save As** i stedet design i browserens private lager, og programmet gør opmærksom på det. Hvis webstedets data ryddes, kan designfilerne gå tabt. Download derfor en kopi til opbevaring uden for browseren eller flytning til en anden enhed.

I alle browsere kan du vælge **File → Download design (download design)** for at hente en kopi af det aktuelle design inklusive ugemte ændringer. I **File → Files in this browser… (filer i denne browser)** — og i Firefox og Safari også **File → Open** — gør **Download open design (download åbent design)** det samme. **Download** ud for en gemt fil henter dens senest gemte indhold. Om en mappevælger vises, afhænger af browserens downloadindstillinger.

Brug **Open from computer… (åbn fra computer)** i samme vindue for at åbne et design fra computeren. Hvis browseren kan skrive i en valgt mappe, gemmes tilbage i den åbnede fil; ellers arbejdes der med en kopi i browserlageret. Brug **Rename (omdøb)** ved en gemt fil for at ændre dens navn. Automatisk lagring og gendannelse bruger browserens private lager i alle browsere. Eksportfiler som STEP og STL downloades.

## Ordliste – engelske udtryk på dansk

| Engelsk udtryk | Dansk oversættelse |
|---|---|
| Import | Importér / indlæs |
| Export | Eksportér / skriv |
| Read | Læs |
| Write | Skriv |
| Body | Emne |
| Assembly | Samling |
| Component | Komponent |
| Occurrence | Forekomst / instans |
| Subassembly | Undersamling |
| Rigid | Fast forbindelse |
| Unlock Contents | Frigør indhold |
| Mesh | Trekantnet / mesh |
| B-rep | Grænsefladerepræsentation af solid |
| Solid | Massivt emne |
| Tessellation | Opdeling af overflade i trekanter |
| Face Tag | Fladeidentifikator |
| Slicer | 3D-print-slicer |
| Send to Slicer | Send til slicer |
| Save as Mesh | Gem som mesh |
| Save | Gem |
| Save As | Gem som |
| Save As JSON | Gem som JSON |
| Export Design as JSON | Eksportér design som JSON |
| Render | Rendering / visning |
| Appearance | Udseende |
| Texture | Tekstur |
| Roughness | Ruhed |
| Metallic | Metallisk materialeegenskab |
| Recovery | Gendannelse |
| Autosave | Automatisk lagring |
| Cache | Mellemlager |
| Thumbnail | Miniaturebillede |
| Preview | Forhåndsvisning |
| Timeline | Tidslinje |
| Design Properties | Designegenskaber |
| Part Number | Varenummer |
| Revision | Revision |
| Author | Forfatter |
| Organization | Organisation |
| Description | Beskrivelse |
| License | Licens |
| Document Settings | Dokumentindstillinger |
| Files in This Browser | Filer i denne browser |
| Download Open Design | Download åbent design |
| Open from Computer | Åbn fra computer |
| Rename | Omdøb |

### Kommandoer og felter

| Teknisk navn | Dansk betydning |
|---|---|
| `mesh.convert` | Konvertér mesh til solid |
| `file.export` | Eksportér fil |
| `file.save_mesh` | Gem mesh |
| `file.save` | Gem design |
| `file.save_as` | Gem design som |
| `document.metadata` | Læs designegenskaber |
| `document.metadata.set` | Angiv designegenskaber |
| `document.metadata.remove` | Fjern designegenskaber |
| `document.inspect` | Undersøg design |
| `timeline.export_script` | Eksportér kommandoscript |
| `metadata` | Metadata |
| `part_number` | Varenummer |
| `description` | Beskrivelse |
| `revision` | Revision |
| `cache` | Geometricache |
| `preview` | Forhåndsvisning |
| `compression` | Komprimering |
| `format` | Filformat |
