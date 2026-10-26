<a id="glossary"></a>

# Ordliste

Denne side fastlægger SolidCrafts **fælles faglige sprog**: ét aftalt udtryk for hvert begreb.
Udtrykkene defineres ét sted og bruges ens i brugerfladen, kommando-ID'er, parameternøgler,
designfilen, dokumentationen, MCP-værktøjerne og oversættelserne.

Der gælder to regler:

- **Ét begreb, ét udtryk.** Brug det udtryk, der er angivet her, frem for et synonym.
- **Ét udtryk, ét begreb.** Hvis et ord har flere betydninger, præciseres det, f.eks.
  *construction entity* og *construction plane*. Hver betydning får sit eget opslag i oversættelsesfilerne.

En pull request, der tilføjer et nyt begreb — en kommando, en parameter, en filnøgle eller et panel —
tilføjer også udtrykket her. Hvis en tekst, et ID eller en nøgle omdøbes for at følge ordlisten,
skal det gamle ID eller den gamle nøgle fortsat fungere som alias og anføres i `docs/compatibility.md`.

## Engelsk-dansk oversigt

De engelske fagudtryk står også ved hvert opslag, så du kan genkende dem i programmet og i vejledninger.

| Engelsk | Dansk betydning |
|---|---|
| Appearance | Udseende |
| Body | Emne |
| Browser | Modeltræ |
| Component | Komponent |
| Constraint | Geometrisk binding |
| Construction entity | Hjælpegeometri i en skitse |
| Construction plane / axis / point | Konstruktionsplan / -akse / -punkt |
| Curve | Kurve |
| Design | Design |
| Design file | Designfil |
| Dimension | Mål |
| Feature | Feature: et trin, der opretter eller ændrer geometri |
| Folder | Fast mappe i modeltræet |
| Group | Brugeroprettet gruppe |
| Material | Fysisk materiale |
| Occurrence | Placeret forekomst af en komponent |
| Part number | Varenummer |
| Pattern instance | Kopi i et mønster |
| Rollback marker | Tilbagerulningsmarkør |
| Sketch | Skitse |
| Sketch entity | Skitseelement |
| Standard part | Standarddel |
| Timeline | Tidslinje |
| Web browser | Webbrowser |

<a id="how-an-entry-reads"></a>

## Sådan læses et opslag

Hvert opslag indeholder udtrykket og en **definition**: en kort sætning i almindeligt teknisk sprog,
som kan sættes ind i stedet for udtrykket. Definitionen nævner ikke et produkt og forklarer ikke
udtrykket med ordet selv. Det følger principperne for terminologi i ISO 704 og ISO 10241-1.
Derefter følger den praktiske sammenhæng:

- **I SolidCraft:** de kommando-ID'er, parameternøgler og designfilnøgler, som bruger udtrykket.
- **Undgå:** andre betegnelser for samme begreb.
- **Andre betegnelser:** ord, som brugere fra andre CAD-programmer kan kende.
- **Til oversættere:** begreber, som udtrykket ikke må forveksles med.

Nogle ID'er og tekster nedenfor bruger stadig ord, som ordlisten fraråder. De ændres i senere
pull requests, ét emne ad gangen, med henvisning til det relevante opslag.

<a id="terms"></a>

## Begreber

### appearance

**Dansk: udseende.** Hvordan et emne, en flade eller en komponent ser ud på skærmen og på renderede billeder.

Udseendet bestemmer kun farve og overfladens finish. Det påvirker ikke masse eller styrke.
Sammenlign med *material* (fysisk materiale).

- **I SolidCraft:** `appearance.assign`, `appearance.library`, `appearance.list`; nøglen
  `appearances` i designfilen.
- **Undgå:** look, colour (om hele udseendet), render material.
- **Andre betegnelser:** appearance, visual material, colour.
- **Til oversættere:** må ikke forveksles med det fysiske materiale.

### body

**Dansk: emne.** Én sammenhængende massiv form eller overfladeform, som features opretter og ændrer.

Et design kan indeholde flere emner. En komponent samler emner; en forekomst placerer dem.

- **I SolidCraft:** `body.rename`; mappen `bodies` i modeltræet; parametre med navnet `body_name`.
- **Undgå:** part, solid (når overfladeemner også er omfattet), object.
- **Andre betegnelser:** body, solid body, surface body.
- **Til oversættere:** et emne er hverken en komponent eller et helt design.

### browser

**Dansk: modeltræ.** Panelet, der viser designets komponenter, emner, skitser, konstruktionsplaner
og led i en træstruktur.

- **I SolidCraft:** `browser.group`, `browser.move`, `browser.order`, `browser.visibility` og de
  øvrige `browser.*`-kommandoer.
- **Undgå i den engelske terminologi:** model tree, scene browser, parts browser.
- **Andre betegnelser:** browser, model tree, feature tree, design tree.
- **Til oversættere:** dette er ikke en webbrowser. Et sprog kan bruge sit eget CAD-udtryk for træet
  (f.eks. *arborescence* på fransk og *modeltræ* på dansk), så de to begreber holdes adskilt.
  Se også *web browser*.

### component

**Dansk: komponent.** En navngivet beholder med emner, skitser, konstruktionsgeometri og andre
komponenter, som kan placeres én eller flere gange.

Hver placering er en *occurrence* (forekomst). Ændringer i komponenten slår igennem i alle dens forekomster.

- **I SolidCraft:** `component.create`, `component.activate` og de øvrige `component.*`-kommandoer;
  nøglen `components` i designfilen.
- **Undgå:** part, assembly, sub-assembly (om selve komponenten).
- **Andre betegnelser:** component, part, sub-assembly.
- **Til oversættere:** en komponent er ikke det samme som en forekomst.

### constraint

**Dansk: geometrisk binding.** En regel, der fastlægger forholdet mellem skitseelementer,
f.eks. at de skal være sammenfaldende, parallelle eller tangere hinanden.

En binding med en talværdi er en *dimension* (et mål).

- **I SolidCraft:** `sketch.constraint.coincident`, `sketch.constraint.parallel` og de øvrige
  `sketch.constraint.*`-kommandoer; parameternøglen `constraint`; skitsens nøgle `constraints` i designfilen.
- **Undgå:** relation, condition.
- **Andre betegnelser:** constraint, geometric constraint, relation.
- **Til oversættere:** et led mellem komponentforekomster er ikke en skitsebinding.

### construction entity

**Dansk: hjælpegeometri i en skitse.** Et skitseelement, som bruges til at tegne efter,
men ikke indgår i en profil.

Det kan være en linje, en bue, en cirkel eller en anden kurve. Det kan have bindinger og mål
ligesom andre skitseelementer.

- **I SolidCraft:** parameteren `construction` i skitsens tegnekommandoer; feltet `construction`
  på en skitsekurve; opslaget `ui.construction` i oversættelsesfilerne.
- **Undgå:** construction alene, hvis det kan forveksles med *construction plane*.
- **Andre betegnelser:** construction line, construction geometry, reference line.
- **Til oversættere:** dette er ikke et konstruktionsplan, en konstruktionsakse eller et
  konstruktionspunkt. Modeltræets Construction-mappe og denne linjetype deler stadig opslaget
  `Construction`, så mappen på tysk, koreansk og tyrkisk vises med ordet for konstruktionslinje.

### construction plane, construction axis, construction point

**Dansk: konstruktionsplan, konstruktionsakse, konstruktionspunkt.** Et plan, en akse eller et punkt,
som en feature opretter for at placere andre features, og som ikke indgår i et emne.

De vises samlet i modeltræets *Construction*-mappe.

- **I SolidCraft:** `construct.plane.offset`, `construct.axis.two_points`,
  `construct.point.vertex` og de øvrige `construct.*`-kommandoer; værktøjslinjens CONSTRUCT-panel.
- **Undgå:** construction alene, hvis det kan betyde *construction entity*; work plane.
- **Andre betegnelser:** construction plane, reference plane, datum plane (tilsvarende for akse og punkt).
- **Til oversættere:** må ikke forveksles med hjælpegeometri i en skitse.

### curve

**Dansk: kurve.** Et skitseelement, som ikke er et punkt: en linje, bue, cirkel, ellipse, spline eller keglesnitskurve.

- **I SolidCraft:** skitsens nøgle `curves` i designfilen; kurve-ID'er i parametre som `path`.
- **Undgå:** geometry, edge (om en skitsekurve).
- **Andre betegnelser:** curve, sketch curve, segment.
- **Til oversættere:** en skitsekurve er ikke en kant på et emne.

### design

**Dansk: design.** Den model, der er åben i SolidCraft, med tidslinje, komponenter, emner, skitser,
parametre og indstillinger.

Et design gemmes som en *design file* (designfil).

- **I SolidCraft:** `file.new` ("New Design"), `document.units` ("Change Design Units"),
  `document.metadata` ("Design Properties"), `doc.open`, `doc.recover`.
- **Undgå:** document, doc, project, file (om selve modellen), part.
- **Andre betegnelser:** design, document, model, part file, assembly file.
- **Til oversættere:** dette er hverken designegenskaben *Project* eller skitsekommandoen Project.

### design file

**Dansk: designfil.** En fil med filendelsen `.solidcraft`, som indeholder ét gemt design.

- **I SolidCraft:** `file.save`, `file.save_as`, `doc.open`; formatmærket `solidcraft/N`.
- **Undgå:** document, project file.
- **Andre betegnelser:** design file, model file.
- **Til oversættere:** fil bruges om det, der ligger på disken, og som åbnes, gemmes, importeres eller eksporteres.

### dimension

**Dansk: mål.** En binding, hvis værdi fastlægger en størrelse, f.eks. en længde, afstand, vinkel eller radius.

Et mål kan styre skitsen eller være afledt af skitsen og kun vise den aktuelle værdi.

- **I SolidCraft:** `sketch.dimension`, `sketch.dimension_text`, `sketch.toggle_driven`;
  et måls ID er et bindings-ID.
- **Undgå:** measurement (om en målsætning), size.
- **Andre betegnelser:** dimension, dimensional constraint, driving dimension.
- **Til oversættere:** dette er hverken et mål på en arbejdstegning eller en målt værdi fra måleværktøjet.

### feature

**Dansk forklaring: et trin, der opretter eller ændrer geometri.** Et trin på tidslinjen,
f.eks. en ekstrudering, afrunding eller oprettelse af et konstruktionsplan.

- **I SolidCraft:** `timeline.edit`, `timeline.suppress`, `timeline.reorder` og de øvrige
  `timeline.*`-kommandoer; nøglen `features` i designfilen.
- **Undgå:** operation, step (i engelske tekster i brugerfladen); feature om noget, SolidCraft
  generelt kan — brug i stedet tool eller capability på engelsk.
- **Andre betegnelser:** feature, operation.
- **Til oversættere:** på fransk bruges *fonction* om en feature på tidslinjen, så AI features
  måtte ændres til *fonctionnalités IA*. Skeln mellem en feature på tidslinjen og en programfunktion.

### folder

**Dansk: fast mappe i modeltræet.** En af komponentens faste sektioner: Bodies, Sketches, Construction eller Joints.

Disse mapper findes i alle komponenter og kan ikke omdøbes eller fjernes.
En beholder, som brugeren selv opretter, er en *group* (gruppe).

- **I SolidCraft:** parameteren `folder` i `browser.group`, `browser.order` og
  `browser.visibility` (`bodies`, `sketches`, `construction`).
- **Undgå:** folder om en brugeroprettet gruppe.
- **Andre betegnelser:** folder, node.
- **Til oversættere:** dette er hverken en gruppe eller en mappe på disken.

### group

**Dansk: gruppe.** En navngivet beholder, som brugeren opretter i modeltræet eller på tidslinjen
for at samle elementer.

En gruppe ændrer kun, hvordan elementerne vises. Den påvirker hverken geometrien eller den
rækkefølge, som features beregnes i.

- **I SolidCraft:** `browser.group`, `browser.ungroup`, `browser.rename_group`; `timeline.group`,
  `timeline.ungroup`, `timeline.rename_group`, `timeline.move_to_group`; parameternøglen `group`;
  nøglerne `browser_groups` og `timeline_groups` i designfilen.
- **Undgå:** feature folder, folder (om en gruppe).
- **Andre betegnelser:** folder, group.
- **Til oversættere:** må ikke forveksles med en rigid group (et led) eller en fast *folder* i modeltræet.

### material

**Dansk: fysisk materiale.** Det stof, et emne består af, som bestemmer dets densitet og mekaniske egenskaber.

Materialet bestemmer masse, tyngdepunkt og styrkeværdier. Hvordan emnet ser ud, bestemmes af dets
*appearance* (udseende).

- **I SolidCraft:** `material.assign` ("Physical Material"), `material.list`, `material.create`
  og de øvrige `material.*`-kommandoer; nøglerne `materials` og `user_materials` i designfilen.
- **Undgå:** material om det visuelle udseende eller et materiale, som kun bruges til rendering.
- **Andre betegnelser:** material, physical material.
- **Til oversættere:** må ikke forveksles med appearance.

### occurrence

**Dansk: forekomst.** Én placeret kopi af en komponent i dens overordnede komponent.

En komponent kan have flere forekomster. Flytter du en forekomst, flyttes kun den kopi.
Ændrer du selve komponenten, ændres alle kopier. Modeltræet viser forekomster med et nummer,
f.eks. `Bolt M20:3`.

- **I SolidCraft:** `occurrence.copy`, `occurrence.move`, `occurrence.ground`,
  `occurrence.properties`; nøglen `occurrences` i designfilen; `path` med forekomst-ID'er.
- **Undgå:** instance, placement.
- **Andre betegnelser:** occurrence, instance, component instance.
- **Til oversættere:** dette er ikke en *pattern instance* (kopi i et mønster).

### part number

**Dansk: varenummer.** En designegenskab med den identifikation, en organisation giver et design.

- **I SolidCraft:** designegenskaben `part_number`, som vises i Design Properties og indstilles med
  `document.metadata.set`.
- **Undgå:** part number om størrelsen på en standarddel.
- **Til oversættere:** se part i afsnittet om ord med flere betydninger.

### pattern instance

**Dansk: kopi i et mønster.** Én kopi af features, emner eller skitseelementer, som et mønster opretter.

Den første kopi er originalen.

- **I SolidCraft:** `solid.pattern.rectangular`, `solid.pattern.circular`, `solid.pattern.path`,
  `sketch.pattern.rectangular`, `sketch.pattern.circular`; parametrene `count` og `spacing`
  (afstanden mellem kopier).
- **Undgå:** instance om en komponentforekomst; element, item.
- **Andre betegnelser:** instance, element, occurrence.
- **Til oversættere:** må ikke forveksles med en forekomst af en komponent.

### rollback marker

**Dansk: tilbagerulningsmarkør.** Markøren på tidslinjen, hvorefter ingen features beregnes.

Flytter du markøren tilbage, vises designet, som det så ud på det trin.
Features efter markøren bevares; de slettes ikke.

- **I SolidCraft:** `timeline.rollback`, `timeline.roll_to`; nøglen `marker` i designfilen.
- **Undgå:** timeline marker, history marker, rollback bar.
- **Andre betegnelser:** rollback bar, rollback marker, end-of-part marker.
- **Til oversættere:** history er fortrydelseshistorikken, ikke tidslinjen. På fransk fandtes der
  tre navne for denne markør, ligesom på engelsk.

### sketch

**Dansk: skitse.** Punkter og kurver med deres bindinger på ét plan, som features bruger som
profiler, baner og styrekurver.

- **I SolidCraft:** `sketch.create`, `sketch.finish` og de øvrige `sketch.*`-kommandoer;
  mappen `sketches` i modeltræet.
- **Undgå:** drawing, profile (om hele skitsen).
- **Andre betegnelser:** sketch, 2D sketch.
- **Til oversættere:** en skitse er ikke en arbejdstegning med visninger og mål.

### sketch entity

**Dansk: skitseelement.** Et punkt eller en kurve i en skitse.

- **I SolidCraft:** skitsekommandoernes parameter `entities`, som modtager punkt- og kurve-ID'er;
  skitsens nøgler `points` og `curves` i designfilen.
- **Undgå:** geometry (om skitsens indhold), element, object i den engelske terminologi.
- **Andre betegnelser:** sketch entity, sketch geometry, sketch element.
- **Til oversættere:** entity omfatter både punkter og kurver.

### standard part

**Dansk: standarddel.** En færdig komponent, f.eks. en skrue, møtrik, skive, stift eller et leje,
som SolidCraft opbygger ud fra en standardstørrelse.

- **I SolidCraft:** `parts.library` ("Standard Parts"), `parts.insert`, `parts.fastener`,
  `parts.resize`; nøglen `parts` i designfilen.
- **Undgå:** library part, part alene, content.
- **Andre betegnelser:** standard part, library part, fastener, catalogue part.
- **Til oversættere:** se part i afsnittet om ord med flere betydninger.

### timeline

**Dansk: tidslinje.** Designets ordnede liste af features, som SolidCraft beregner fra den første til den sidste.

- **I SolidCraft:** `timeline.*`-kommandoerne; nøglen `features` i designfilen, i tidslinjens rækkefølge.
- **Undgå:** feature history, history (om tidslinjen).
- **Andre betegnelser:** timeline, history, feature tree.
- **Til oversættere:** tidslinjen er ikke fortrydelseshistorikken.

### web browser

**Dansk: webbrowser.** Et program, der viser websider, f.eks. det program, som kører SolidCrafts webversion.

- **I SolidCraft:** webversionen; opslaget `home.in_this_browser` i oversættelsesfilerne.
- **Undgå:** browser alene.
- **Til oversættere:** dette er ikke panelet *browser*. Hvis sproget bruger samme ord for begge,
  bruges CAD-betegnelsen om panelet.

<a id="words-with-more-than-one-meaning"></a>

## Ord med flere betydninger

Når to betydninger af et engelsk ord deler et opslag i oversættelsesfilerne, kan oversættelsen
passe til den ene betydning og være forkert for den anden. Hver betydning nedenfor får sit eget
udtryk og på sigt sit eget opslag.

| Ord | Betydning | Brug | Hvor |
|---|---|---|---|
| Construction | Skitseelement, der ikke indgår i en profil | Hjælpegeometri i skitsen (*construction entity*) | Skitsens tegnekommandoer, `ui.construction` |
| | Modeltræets mappe med konstruktionsplaner, -akser og -punkter | Konstruktionsplan, -akse, -punkt | Construction-mappen, `construct.*` |
| Instance | Kopi oprettet af et mønster | Kopi i et mønster (*pattern instance*) | Mønsterkommandoer |
| | Placeret kopi af en komponent | Forekomst (*occurrence*) | "Paste (Instance)" i `occurrence.copy` |
| History | Historikken for fortryd og gentag | Fortrydelseshistorik | Undo, Redo |
| | Listen over features | Tidslinje (*timeline*) | "Feature history" (`ui.feature_history`), "Roll History Marker" |
| Part | En standarddel | Standarddel (*standard part*) | `parts.*` |
| | Et emne, en forekomst eller hele designet | Emne, forekomst, design | Tekster, der bruger part |
| | En designegenskab | Varenummer (*part number*) | Design Properties |
| Gap | Mellemrummet i en ombukket pladekant | Mellemrum i ombukningen | Gap-rækken i Hem-dialogen |
| | En åben ende i skitsens oversigt over sammenhæng | Åben ende | Skitsens oversigt over sammenhæng |
| Clearance | Kommandoen Clearance | Kontrol af friafstand | `inspect.clearance` |
| | Et felt med en minimumsafstand | Friafstand | Dialogerne Interference og plastregler |
| Flat | Et huls flade bund | Flad bund | Spidsformen i Hole-dialogen |
| | En ombukning, der ligger fladt mod pladen | Flad ombukning | Typen i Hem-dialogen |
| Round | En rund aflastning eller hjørneform | Rund form | Aflastninger, plastunderstøtninger og hjørner på regelflader |
| | En afrunding | Afrunding (*fillet*) | `solid.fillet` |
| Normal | Skitseelement, der ikke er hjælpegeometri | Almindelig linjetype | Linjetypen i skitsepaletten |
| | Retningen vinkelret på en overflade | Normalretning | Normal-tilstanden for en regelflade |
| | Den mellemste visningskvalitet | Normal kvalitet | `ui.display_quality_normal` |

Sammenblandingen ses allerede i nogle oversættelser: På japansk og kinesisk vises et huls flade bund
med ordet for udfoldning eller plan. På kinesisk vises Round som fillet, og på kinesisk og russisk
vises linjetypen Normal med ordet for normalvektor.

<a id="two-commands-one-label"></a>

## To kommandoer med samme navn

Et navn i brugerfladen skal betegne én kommando. I dag deler to kommandopar samme navn:

- "Edit Feature": `timeline.redefine` og `timeline.edit`.
- "Break Link": `component.break_link` (en forekomsts forbindelse til et andet design) og
  `sketch.break_link` (et projiceret skitseelements forbindelse til dets kilde).

Søgningen viser begge kommandoer med samme navn, og opslaget af navnet giver dem samme oversættelse.

<a id="a-features-type"></a>

## En features type

En featuretype har ét navn: nøglen i designfilen, f.eks. `sheet_flange`.
Navnet i brugerfladen, f.eks. Flange, er dens visningsnavn. Svar og scripts bør bruge nøglen
fra designfilen. I dag rapporterer `timeline.inspect` et andet navn (`EdgeFlangeFeature`),
så et script ikke kan sammenligne de to navne direkte.

<a id="open-questions-for-the-maintainer"></a>

## Åbne spørgsmål til vedligeholderen

- **Britisk eller amerikansk stavning** i brugerfladen og dokumentationen. I dag bruges begge
  former (colour og color, centre og center, favourite og favorite), mens ID'er og nøgler bruger
  amerikansk stavning. #2501 foreslår en sprogindstilling for English (UK), så begge stavemåder
  kan vises, uanset hvilken der vælges som grundlag.
