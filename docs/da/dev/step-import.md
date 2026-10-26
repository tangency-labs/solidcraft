# STEP Import (STEP-import)

SolidCraft læser STEP (ISO 10303-21, AP203, AP214 og AP242-geometri) med sin egen læser i `crates/kernel/src/step_in`. Læseren er skrevet ud fra offentlige standarder uden tredjeparts-STEP-kode. En STEP-fil bliver en **Import Base Feature** på tidslinjen. Den indeholder filens B-rep, og STEP-teksten gemmes i designfilen, så designet kan genåbnes uden originalfilen. Senere Features som Fillet, Hole, Cut skitseret på modellens flader og Shell bygger videre på de importerede emner.

- **Open (åbn):** File → Open (`doc.open`) åbner `.step`/`.stp` uanset store og små bogstaver som et nyt design. Du kan også bruge `solidcraft part.step`, `solidcraft-cli eval part.step`, `solidcraft-cli run script.json --in part.step` eller MCP-værktøjet `open`.
- **Insert (indsæt):** File → Insert STEP… eller SOLID → INSERT (`file.insert_step {path}`) føjer filen til det aktuelle design.
- **Drag and Drop (træk og slip):** En STEP-fil sluppet på vinduet indsættes, hvis designet allerede har Features; ellers åbnes den som et nyt design.

## Importer bliver komponenter

Hver import (STEP, IGES, 3MF, STL, OBJ; indsat, sluppet, åbnet som nyt design eller udført med `file.insert_step`, `file.insert_mesh` eller `doc.open` fra script, CLI eller MCP) oprettes som en **ny komponent med filens navn** (`bracket.step` bliver `bracket`). Komponenten placeres under den aktive komponent ved filens origo. Import-funktionen tilhører den, så filens emner ikke blandes med dine egne. Også åbning som et nyt design opretter komponenten, som navngives i Browser. Kommandosvaret indeholder `component` (ID), `component_name` og `occurrence`.

En STEP-**Assembly (samling)** beholder sin struktur. Hvert produkt under filens rod bliver en underkomponent til importkomponenten, med produktets navn og den placering, STEP-filen angiver. Dele og undersamlinger brugt flere gange bliver **én komponent med en forekomst pr. placering**. Import-funktionen opbygger delen én gang og springer senere kopier over (`skip` i Feature, filformat 22); forekomsterne viser den på de øvrige positioner. Kommandosvaret angiver desuden `subcomponents` og `rigid_joints`.

Browser genbruger oplysninger om, hvilke emner, skitser og konstruktionsplaner der tilhører komponenterne, samt børnelister og beregnede placeringer, mens kameraet flyttes. Derfor kan store samlinger beholde deres træ åbent uden at gennemgå alle dele og mapper for hvert frame. Geometriredigering, samlingsændringer og positioner, der gemmes eller gendannes, opdaterer oplysningerne. Markering, synlighed og fastlåsning læses direkte.

Hover Picking (forhåndsmarkering ved musen) sættes på pause, mens musen navigerer visningen, og fortsætter, når navigationen stopper. En 3D-mus kan bevæge visningen uden at stoppe forhåndsmarkeringen, så den anden hånd kan vælge objekter samtidig. Markeringen bruger aktuelt kamera og cachelagrede emnegrænser i modelrækkefølge, så alle emners bounding boxes ikke beregnes igen.

Når markøren holdes over en komponent i Browser, bevares fremhævningsbufferen for hele emnet under rotation, panorering og zoom. GPU'en transformerer bufferne sammen med kameraet. Widgets for origo og konstruktionsgeometri ændrer størrelse uafhængigt. Fremhævning af hele emner scanner hvert mesh én gang i stedet for hver flade separat. Feedback for skitseprofiler opdaterer stadig de projicerede kurver og løfter dem mod kameraet under visningsændringer.

Indholdet i en importeret Assembly er **Rigid (fast) som standard**. Rigid Joints (en Rigid Group uden synlige symboler) holder delene dér, hvor filen placerede dem i moderkomponenten. Flyttes importkomponenten, følger alle dele med, og ingen intern del kan flyttes alene. Frihedsgraderne for alle delene tælles som for én samlet enhed. Brug **Unlock Contents (frigør indhold)** på komponentens kontekstmenu (`component.unlock {component}`) for at kunne forbinde delene individuelt med Joints. Det er ét Undo-trin. Undo kan også fjerne hele importen i ét trin.

Design fra før importer blev komponenter, bevarer deres Import-funktion på den oprindelige placering og genopbygges som tidligere uden automatisk at oprette en ny komponent.

Læseren understøtter: lukkede B-rep-solider med hulrum, facetterede B-reps og shell-baserede overflademodeller; planer, cylindre, kegler, kugler, torus, B-spline- og rationelle B-spline-overflader med knots og Bézier, uniforme og kvasiuniforme varianter, revolutions- og lineært ekstruderede overflader samt trimmede flader. Kurver omfatter linjer, cirkler, ellipser, hyperbler og parabler som præcise rationelle kurver, B-splines, polylinjer, trim-, overflade- og seam-kurver. Længde- og vinkelenheder, SI-præfikser og konverteringsbaserede enheder som tommer, produktnavne, fladefarver og samlinger understøttes. Emnenavne kommer fra solidets navn eller produktnavnet, medmindre `names` vælger andet. Assembly-forekomster med transformationer og mapped items bevares som et produkttræ, der omsættes til importkomponenter. Filer behandles som upålidelige: Filstørrelser, antal enheder og indlejringsdybde begrænses, og alle kernel-kald er beskyttede. Geometri, der ikke kan læses, giver en advarsel på Feature; udeladte flader kan resultere i et åbent emne. Endnu ikke understøttet: Offset Surfaces og kanter, der kun er beskrevet ved pcurves.

Ekstruderede cirkulære og elliptiske flader beholder profilens parameter-samling uden for fladens trim eller på en eksplicit samlingskant. Smalle flader som afrundede tandspidser bevarer dermed deres smalle form gennem hele tykkelsen frem for at vikle sig om den underliggende ellipse på midten.

## Filer fra andre CAD-systemer

CAD-systemer kan bruge forskellige opadretninger og navne til solider. `doc.open`, `file.insert_step` og MCP-værktøjet `open` accepterer to valg for STEP- og IGES-filer. Uden dem importeres filen som gemt.

- **`up`:** `"z"` (standard) bevarer filens akser. Mange systemer gemmer Y opad, så delen kan ligge på ryggen i SolidCraft, der bruger Z opad. `"up": "y"` drejer importkomponenten +90° om X, så Y bliver Z, som vinduets **Stand It Up (rejs op)**. Hele samlingen drejes samlet som ét Undo-trin. Svaret angiver forekomsten under `stand_up`. Eksisterende designgeometri drejes ikke. Vinduet tilbyder at rejse en STEP-import op, medmindre kaldet eksplicit har valgt `up`, også når valget er `"z"`.
- **`names`:** Angiver emnenavne. `"solid"` (standard) bruger solidets navn eller produktnavnet, hvis det mangler. Nogle CAD-systemer navngiver solider efter sidste Feature (`Cut-Extrude3`), hvilket gør samlingsnavne uklare. `"product"` navngiver ud fra delen i STEP-produkttræet, mens `"instance"` bruger samlingsforekomsten (`Post-2`) eller produktet uden en forekomst. Manglende eller midlertidige pladsholdernavne erstattes med næste brugbare navn (instans, produkt, solid). Gentagne navne nummereres (`Post`, `Post (2)`). Navnene gemmes med Import-funktionen og bevares ved genåbning. Standard er solidnavnet, fordi SolidCrafts egen STEP-eksport gemmer emnenavnet dér.

```json
{"command": "file.insert_step", "params": {"path": "frame.step", "up": "y", "names": "instance"}}
```

## Importerede udseender

Importerede emnefarver og fladefarver bevares gennem senere Features. Emnefarver bruges som fallback under eksplicit tildelte materialer og udseender. Fladefarver følger fladernes vedvarende navne. En slettet flade har ingen farve, som kan vises på det resulterende emne.

## IGES

IGES 5.3-filer (`.igs`/`.iges`) kan åbnes, indsættes eller slippes på samme måde. `crates/kernel/src/iges_in.rs`, skrevet efter den offentliggjorte specifikation, omsætter filen til STEP, som derefter læses af STEP-læseren. Derfor gemmer Import-funktionen STEP-tekst. Læseren understøtter lukkede B-rep-solider (186) med skaller, flader, loops, kant- og hjørnepunktslister; trimmede (144) og afgrænsede (143) overflader samt enkeltstående B-spline-patches, som sys til et solid ved fælles kanter (eller et åbent emne, hvis de ikke lukker). Kurver: linjer, cirkelbuer, ellipser, sammensatte kurver og rationelle B-splines. Flader: planer (108, 190), cylindre, kegler, kugler, torus, rationelle B-splines, rotationsflader og tabulerede cylindre. Transformationer, enheder, farver (314 og prædefinerede værdier) og emnenavne (navneegenskaber eller etiketter) understøttes. Endnu ikke understøttet: Ruled/Offset Surfaces og kurver kun angivet i overfladens parameterkoordinater. Eksport med `file.export` til `.igs` skriver hvert emne som en lukket B-rep med samme overflader som STEP-eksporten samt navn og farve.

## FreeCAD og Rhino

FreeCAD `.FCStd` og Rhino `.3dm` kan åbnes, indsættes eller slippes på desktop og via CLI. De indlæses med [cadmpeg](https://github.com/cadmpeg/cadmpeg) (Apache-2.0, `crates/io/src/native_cad.rs`), som genkender formatet ud fra filens indhold, afkoder en neutral model under ressourcebegrænsninger, kontrollerer modellen og skriver STEP AP214. SolidCrafts STEP-læser importerer derefter resultatet. Import-funktionen gemmer STEP-teksten, så originalfilen ikke kræves ved genåbning. Solider importeres, men Feature-historik, skitser og parametre gør ikke. Tab rapporteret af cadmpeg bliver advarsler på importen. Filer uden et importérbart solid afvises med en forklaring. cadmpeg køres under kernelens panic guard. Detaljer om understøttelse findes under [format support](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md).

Kun codecs til åbne formater kompileres. Workspacet slår cadmpegs standardfunktioner fra og aktiverer `fcstd`, `rhino` og `step` til skrivning. Codecs til kommercielle CAD-formater bygges ikke, heller ikke `sat`, der kan afkode ShapeManager-streams med ACIS som variant. ACIS `.sat`/`.sab` understøttes derfor endnu ikke. `crates/io/src/native_cad.rs` er den eneste indgang til cadmpeg. En senere cadmpeg-baseret STEP-læser kan tilføjes dér bag en indstilling for at sammenligne med SolidCrafts egen læser uden at ændre den. Browserappen kan endnu ikke læse disse formater, fordi cadmpeg bygger på zstd C-biblioteket, som wasm-bygningen udelader.

## Ordliste – engelsk til dansk

| Engelsk udtryk | Dansk betydning |
|---|---|
| STEP Import | STEP-import |
| Reader | Filindlæser |
| Parser | Fortolker |
| Import Base Feature | Importeret grundfunktion |
| Body | Emne |
| B-rep | Grænsefladerepræsentation |
| Assembly | Samling |
| Component | Komponent |
| Occurrence | Forekomst |
| Rigid Group | Fast gruppe |
| Unlock Contents | Frigør indhold |
| Drag and Drop | Træk og slip |
| Hover Picking | Forhåndsmarkering med mus |
| Bounding Box | Afgrænsningskasse |
| Tessellation | Opdeling af overflader i trekanter |
| Surface | Overflade |
| Trimmed Surface | Afgrænset overflade |
| Manifold | Lukket, entydig topologi |
| Seam | Parametersamling |
| Knot | Knudepunkt i spline |
| B-spline | B-splinekurve |
| Rational B-spline | Rationel B-spline |
| Stand It Up | Rejs modellen op |
| Imported Appearance | Importeret udseende |
| Up Axis | Opadakse |
| Solid Name | Solidnavn |
| Product Name | Produktnavn |
| Instance Name | Forekomstnavn |
| FreeCAD | FreeCAD |
| Rhino | Rhino |
| Kernel | Geometrikerne |
| Panic Guard | Beskyttende fejlhåndtering |
| `file.insert_step` | Indsæt STEP-fil |
| `component.unlock` | Frigør samlingens indhold |
