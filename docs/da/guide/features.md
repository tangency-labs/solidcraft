# Solid Features (solidmodellering)

Features (modelleringsfunktioner) ligger på **Timeline (tidslinjen)** og genberegnes fra den første funktion, der er ændret. Hver funktion kan redigeres med dobbeltklik, deaktiveres midlertidigt, omdøbes, flyttes i rækkefølgen, rulles tilbage til eller slettes. Rækkefølgen kan kun ændres, hvis afhængighederne fortsat ligger før funktionen. Sletning kræver bekræftelse, hvis andre funktioner afhænger af den.

| Panel | Funktioner |
|---|---|
| Create (opret) | extrude (one side, two sides, symmetric, taper, start offset, to a distance or through all; new body, join, cut, intersect; thin walls along curves), revolve, sweep, loft, box, cylinder, sphere, torus, coil, pipe, rib, web, emboss, hole (simple, counterbore, countersink, tapped; on faces or at sketch points), thread, rectangular/circular/path patterns, mirror |
| Modify (rediger) | press pull, fillet, chamfer, shell, draft, scale, combine, split body, offset face, replace face, move/copy, align, remove |
| Construct (konstruér) | offset plane, plane at angle, plane along path |
| Inspect (undersøg) | measure, section analysis, interference, compare bodies, curvature comb and map, zebra, draft, minimum radius, isocurves, centre of mass |

Extrude vælger automatisk **Join (sammenføjning)**, hvis den aktive komponent allerede indeholder et solidt emne, også ved sideforbindelser og skruefæster, der kun møder en del af en flade. Ekstrudering ind i eksisterende materiale vælger **Cut (udskæring)**. I en tom komponent vælges **New Body (nyt emne)**. Vælg New Body manuelt, hvis tilføjelsen skal være separat. Ændring af standardvalget påvirker ikke allerede gemte operationer.

**Taper (konusvinkel)** giver siderne på enhver profil en hældning, også ved skitsetekst. Linjer, buer, splines og keglesnitskurver forskydes samlet ind eller ud; en kort, flad spids lukkes til et punkt. På splines og keglesnitskurver er hældningen tilnærmet inden for 2 % af forskydningen (eller 10 µm). Kurver, der bøjer for meget, opdeles, og offset-kurver, der folder ind over sig selv, udjævnes. Hvis konusvinklen er for stor i forhold til smalle detaljer eller åbninger, afvises funktionen med en forklaring. En mindre vinkel eller afstand kan bruges. Taper kan kombineres med **Start Offset (startforskydning)**: Profilen flyttes langs normalen, og hældningen begynder dér. Profilen er dermed tværsnittet ved startpunktet. Taper med Two Sides eller Symmetric understøttes endnu ikke.

**Thin Extrude (tyndvægget ekstrudering)** (`thin: true` i `solid.extrude` eller valget i dialogen) bygger en væg med konstant tykkelse langs skitsekurver i stedet for at udfylde profiler. Åbne kæder, lukkede løkker, linjer, buer, cirkler, ellipser og splines understøttes. En valgt profil repræsenterer kurverne omkring den, også huller. Væggen kan centreres på kurven, placeres indvendigt eller udvendigt eller have forskellig tykkelse på begge sider: `side` kan være `center`, `inside`, `outside` eller `two_sides` med `thickness2`. På lukkede løkker er indersiden det omsluttede område; på åbne kæder er det den side, kurven bøjer mod, og på en lige linje venstre side i tegneretningen. Kurver, der mødes ende mod ende, bliver én kæde med geringshjørner. Vægge, der krydser hinanden, bliver ét emne. Alle Extrudes udstrækninger, operationer og mål-emner gælder, ligesom Taper for rette vægge eller hele cirkler. En væg tykkere end indersidens kurveradius afvises. Spline-vægge følger offset-kurven inden for 0,0001 mm.

Når Extrude redigeres, bevares afstandenes fortegn, From/To Object, Taper-udtryk og valgte mål-emner. Revolve bevarer verdens-, skitse- og linjeakser, begge vinkeludtryk og To Object. Et uændret **All Profiles (alle profiler)** inkluderer også profiler, der tilføjes senere. Cancel gendanner hele tidslinjen; en vellykket redigering er ét Undo-trin, også ved efterfølgende parameterændringer. En placeret komponent viser preview før den efterfølgende historik. Profilguider til cylindrisk Emboss følger den valgte skitseforekomst.

Nye adaptive profilvalg gemmer identiteten af hele den ydre kontur og hulkonturer, når afgrænsningen kan identificeres entydigt. Ændres en ellipse eller periodisk offset efter Extrude, kan antallet af spline-segmenter ændres uden at miste det valgte materialeområde. Delvise, krydsende eller tvetydige konturer kan ikke bruge denne reference; ændrede hulidentiteter kan også bryde den. Ældre valg af kurvesegmenter beholder deres gemte segmenter. Hvis en gammel funktion har mistet profilen efter en opdeling, skal du redigere funktionen og vælge det aktuelle område igen.

Nye symmetriske Extrude-funktioner bruger den indtastede afstand som **samlet tykkelse**, fordelt ligeligt omkring startplanet: 10 mm på en XY-skitse giver −5 til +5 mm. Skift fra One Side til Symmetric bevarer den samlede tykkelse. Through All når mål-emner i begge retninger; Two Sides har separate afstande. Ældre gemte symmetriske ekstruderinger bevarer deres oprindelige afstand pr. side ved åbning eller redigering. Valg af ny retning i editoren bruger den nuværende regel om samlet tykkelse.

Nye symmetriske **Revolve (omdrejning)**-funktioner fordeler vinklen ligeligt omkring skitseprofilen uanset aksens retning. Two Sides måler den anden vinkel bagud og den første fremad langs retningen bestemt af skitsens normal. Profiler på modsatte sider af aksen får hver sin retning. Design fra før formatversion 11 beholder deres oprindelige vinkelplacering ved åbning, redigering og kopiering; nye Revolve-funktioner bruger den nye regel. To Object følger samme retning for hver profil, også ved buede målflader. Vælg et mål væk fra aksen for at få en entydig vinkelposition.

Ved cylindrisk **Emboss (prægning)** vælger du en understøttet udvendig cylinderflade og en tangent skitseprofil. **Deboss (indprægning)** skærer radialt ind i væggen. Retvinklede linjeprofiler, huller og konkave konturer vikles omkring cylinderen med præcise radiale vægge; profilen må ikke ramme samlinger eller ender. Hele cylindre og koaksiale hylstre understøttes. Gem afventende komponentplaceringer først. Funktionen gemmer målfladen og forekomststierne, så redigering og genåbning bevarer instanserne. Andre buede overflader og friformsprofiler understøttes ikke. Uden fladevalg forsøger Emboss at finde én entydig understøttet cylinder; vælg selv fladen, hvis flere er mulige. Ældre gemte funktioner uden flade bevarer deres plane placering.

Et **Hole (hul)** med **Single (enkeltplacering)** bores dér, hvor du vælger en plan flade. Klikker du på eller tæt ved et synligt skitsepunkt på fladen, f.eks. en cirkels centrum, placeres hullet præcist dér, ligesom med **Multiple (flere placeringer)**. Hold knappen nede for **Select Other (vælg andet)**, som også tilbyder fladen under markøren. Tekstkonturers hjørner tæller ikke som punkter, og projicerede punkter tæller ikke, hvis skitsen skjuler projiceret geometri. Single fastholder placeringen på fladen; brug Multiple, hvis hullerne skal følge senere ændringer af skitsen.

**Fillet (afrunding)** og **Chamfer (affasning)** understøtter rette kanter mellem plane flader, hele glatte løkker, sammenhængende kanter med gerings- eller kugleformede hjørner samt lukkede kæder af buede kanter (rolling-ball). Se udviklingsplanen for andre kendte begrænsninger.

**Curvature (G2, krumningskontinuitet)** under **Radius Type** i Fillet-dialogen skaber en overgang med et præcist femtegradstværsnit på rette kanter mellem to plane flader. Tangentplan og krumning passer til begge sider; krumningen går mod nul ved kontaktlinjerne, i modsætning til en cirkulær G1-Fillet. Hver ende skal møde én plan flade. Vinkelrette og skrå ender understøttes, men ikke buede kanter eller flader, ender med flere flader eller krydsende hjørneovergange. Disse giver fejl og falder ikke tilbage til G1. **Equivalent Radius (ækvivalent radius)** giver samme kontaktafstand som en cirkulær Fillet (ved en ret vinkel er afstanden lig den indtastede værdi), men ikke en konstant krumningsradius. Reduce og Taper to Fit kan ikke bruges.

Scripts og MCP bruger samme kommando, eksempelvis på en kasse på 20 mm:
```json
{"command": "solid.fillet", "params": {"edges": [[20, 0, 10]], "radius": 2, "type": "curvature"}}
```
`type: "g2"` er et alias. Eksisterende Fillet-funktioner bruger fortsat cirkulære G1-overgange med konstant radius som standard.

Referencer til flader og kanter gemmes med navn, så ændringer tidligere på tidslinjen bevarer forbindelsen til den rigtige flade (se [Persistent Naming](../dev/naming.md)). Kan en reference ikke findes, vises en advarsel i stedet for at vælge en anden flade.

**Plane Along Path (plan langs sti)** accepterer én skitsekurve, også en tegnet 3D-kurve, eller en kant på modellen. Vælg stien og angiv **Position (0 to 1)**: 0 er starten, 1 slutningen, og 0,5 er halvvejs langs stien. Planet står vinkelret på det lokale segment. Preview ændrer ikke designet; OK opretter én redigerbar funktion på tidslinjen. Planet følger senere ændringer af stien. Buede stier bruger en samplet polylinje. Absolut afstand, placering efter referenceobjekt og kæder med flere kurver understøttes endnu ikke.

## Sammenligning af to emner

**Compare Bodies (sammenlign emner)** (`inspect.compare`) lægger to versioner af samme emne oven på hinanden, f.eks. en model og en eksport fra et andet program, og finder områder, hvor kun den ene version har materiale. Sammenligningen tager udgangspunkt i overfladerne og ikke i en boolesk operation. Derfor fungerer den også for emner, som deler næsten alle flader — en situation, der er vanskelig for booleske operationer som Combine og Interference.

```json
{"command": "inspect.compare", "params": {"a": "Bracket", "b": "Bracket (2)", "tolerance": 0.01, "color": true}}
```

Uden `a` og `b` sammenlignes de to valgte emner eller de eneste to emner i designet. For begge emner måles volumen, areal, tyngdepunkt og bounding box; `difference` angiver forskellen b − a for hver måling. Overfladerne samples i trekanternes hjørner og på større flader med punkter højst `spacing` (0,5 mm) fra hinanden. For hvert punkt måles afstanden til den anden overflade. `deviation` angiver største og gennemsnitlige afstand i hver retning. Punkter længere væk end `tolerance` (0,01 mm) danner **regions (afvigende områder)**: `only_in_a`, hvor kun `a` har materiale, og `only_in_b`, hvor kun `b` har materiale. For hvert område vises volumen, afvigende overfladeareal, største afvigelse og dens placering, centrum og bounding box samt de berørte flader (`faces_a` og `faces_b`, nummereret som i `model.faces`), sorteret med største volumen først. Volumen integreres langs et gitter af parallelle linjer gennem området med afstand bestemt af `resolution`, så både tynde strimler og større blokke måles. `identical` er true, hvis ingen områder findes, og `verdict` sammenfatter resultatet på én linje. `warnings` angiver forhold, der gør resultatet mindre sikkert: åbne emner, manglende importerede flader eller linjer, som er udeladt fra volumenberegningen, fordi de krydsede en åbning. Med `color` farves fladerne på begge emner røde, hvor kun `a` har materiale, og grønne, hvor kun `b` har. Normalt farves kun flader, hvor størstedelen ligger uden for tolerancen. Hvis ingen flade opfylder dette, vælges fladen med den største andel af området. Farveændringen er ét Undo-trin; uden `color` ændres designet ikke.

Emnerne meshes med en nøjagtighed på en fjerdedel af tolerancen. Geometrikernen genererer ikke et mesh finere end en milliontedel af emnets størrelse, så tolerancen skal være mindst det dobbelte (0,02 mm for et emne, der måler 10 m). Store emner samples grovere med omkring `max_points` (otte millioner) punkter pr. emne. `sampling.spacing_mm` viser den faktisk anvendte afstand. En større `max_points` (op til 50 millioner) giver tættere sampling, men tager længere tid.

## Redigering af en Feature via script

For `solid.pattern.circular` accepterer `axis` værdierne `X`, `Y`, `Z` eller `{origin, dir}`. En eksplicit `origin` skal indeholde tre endelige numeriske koordinater i millimeter (`[x, y, z]` eller `{x, y, z}`). Hvis den udelades, bruges origo i verdenskoordinater. Udtryk og referencer til konstruktionsakser understøttes ikke. Ugyldige koordinater afvises i stedet for stiltiende at blive erstattet med nul.

`timeline.inspect {feature}` returnerer funktionens `definition`: de felter, som `timeline.edit` kan ændre, med deres aktuelle værdier. `timeline.edit {feature, set}` fletter `set` ind i definitionen (objekter flettes felt for felt; andre værdier erstattes), genberegner funktionen og rapporterer `recomputed` og `restored`. Funktionstypen kan ikke ændres, og skitsegeometri redigeres med skitsekommandoerne. Udtryk angives som tekst (`"30"`, `"30 mm"`, `"width / 2"`, `"45 deg"`). Et tal uden enhed fortolkes som længde i designets enheder eller som grader for en vinkel.

```json
{"command": "timeline.inspect", "params": {"feature": "Slab"}}
→ {"type": "ExtrudeFeature",
   "definition": {"sketch": 1, "profiles": {"type": "all"}, "operation": "new_body",
                  "extent": {"distance": "10", "direction": "positive"}},
   "inputs": [{"label": "Distance", "param": "d1", "expression": "10", "unit": "mm"}], …}
{"command": "timeline.edit", "params": {"feature": "Slab", "set": {"extent": {"distance": "12"}}}}
```

`inputs` viser funktionens navngivne input. `parameters.change {name: "d1", expression}` ændrer et af dem direkte. Valgfrie felter, der ikke er sat, vises ikke i definitionen, men kan stadig tilføjes med `set`. De mest almindelige Features har følgende felter:

| Feature | Felter |
|---|---|
| Extrude | `extent`: {`distance`, `direction`: positive\|negative\|symmetric, `distance2`, `start_offset`, `through_all`, `taper`, `to`, `to_offset`, `from`}; `operation`: new_body\|join\|cut\|intersect; `targets`; `profiles`; `sketch`; `thin`: {`curves`, `thickness`, `side`: center\|inside\|outside\|two_sides, `thickness2`} |
| Revolve | `angle`, `angle2`, `symmetric`, `axis`, `to`, `operation`, `targets`, `profiles`, `sketch` |
| Fillet | `radius`, `edges` (et punkt på hver kant), `body`, `style` |
| Chamfer | `distance`, `distance2` eller `angle`, `flip`, `edges`, `body` |
| Box | `length`, `width`, `height`, `corner`, `operation` |
| Cylinder | `radius`, `height`, `base`, `axis`, `operation` |
| Hole | `diameter`, `depth`, `position`, `direction`, `hole` (simple, drilled, counterbore, countersink), `thread`, `to` |
| Shell | `thickness`, `faces`, `direction`, `body`, `tangent_chain` |
| Draft | `angle`, `faces`, `neutral`, `pull`, `body` |
| Offset face | `distance`, `faces`, `body` |
| Pattern | `pattern` (type og antal/afstand), `features`, `bodies` |
| Mirror | `plane`, `features`, `bodies`, `combine` |

For andre Features viser `timeline.inspect` de tilgængelige felter.

Når en kommando vælger en flade via et punkt (`face.delete`, `solid.shell`, `solid.draft`, `solid.offset_face`, `solid.split_face`, `faces` i `solid.replace_face` og `face` i `sheet.convert`), kan et script i stedet angive fladens navn fra `model.faces`: `{"command": "face.delete", "params": {"faces": ["F2:side#1", "F2:side#2"], "body": "Plate"}}`. `model.faces` viser også et `point` på hver flade, som kan bruges til at vælge den. For en flade med hul kan dens centroid ligge i hullet; for en halv cylinder kan den ligge inde i cylinderen. For cylinderflader angives desuden `cylinder` med `origin` og `axis` for aksen samt `radius`. Et navn, der svarer til flere flader (f.eks. dele af en opsplittet flade eller flader på flere emner), afvises. Brug `body` til at angive det ønskede emne.

Tryk **Delete** for at slette det valgte, uanset om det er emner, flader, skitser, Features, planer, komponenter eller canvases. Det sker som ét Undo-trin, og programmet viser de funktioner, der også vil blive slettet eller fejle.

Face Extrude, Face Revolve og Press Pull bevarer forbindelsen til genererede profiler. Hvis en ændret plan flade mangler et konstruktionscertifikat, bruger disse interne profiler den ældre metode til tilknytning via navngivne flader og projektion. Så kan ændringer tidligere på tidslinjen stadig opdatere plan og kontur. Denne kompatibilitet gælder kun de pågældende modelleringsværktøjer: Nye skitser, som brugeren knytter til en flade, kræver en certificeret flade eller et eksplicit fast plan. Ældre interne tilknytninger kan følge den nærmeste tilsvarende flade efter ændringer i topologien. Kontrollér derfor resultatet efter større ændringer tidligere på tidslinjen. Aktivér den ønskede forekomst, før du genererer en fladeprofil fra en gentaget komponent. Gem en separat flyttet kildeposition, før du opretter en ældre profil på tværs af komponenter.

Negativ Press Pull på understøttede, flugtende prismatiske flader bevarer runde gennemgående huller, også efter en sammenføjet Extrude og i en roteret komponentforekomst. Udskæringen opdeler materialet i aksiale sektioner og bevarer hullet gennem Undo, Redo og genåbning. Vægge, der ikke kan identificeres som en parallel ekstrudering, bruger stadig den generelle booleske metode og kan give en fejl om ikke-understøttede sammenfaldende overflader uden delvise ændringer.

## Ordliste – engelske CAD-udtryk på dansk

Denne ordliste oversætter udtrykkene fra tabellerne ovenfor. De engelske navne er bevaret i SolidCraft, scripts og API'er, så de kan findes direkte i programmet. Oversættelserne forklarer betydningen på dansk.

### Create – opret

| Engelsk udtryk | Dansk oversættelse |
|---|---|
| Extrude | Ekstrudering |
| One Side | Én side |
| Two Sides | To sider |
| Symmetric | Symmetrisk |
| Taper | Konusvinkel / skrå sider |
| Start Offset | Startforskydning |
| To a Distance | Til en bestemt afstand |
| Through All | Gennem alt |
| New Body | Nyt emne |
| Join | Sammenføj |
| Cut | Skær ud / fjern materiale |
| Intersect | Fællesmængde / behold overlappende materiale |
| Thin Extrude | Tyndvægget ekstrudering |
| Thin Walls Along Curves | Tynde vægge langs kurver |
| Revolve | Omdrejning om en akse |
| Sweep | Før en profil langs en sti |
| Loft | Overgang mellem profiler |
| Box | Kasse / firkantet solid |
| Cylinder | Cylinder |
| Sphere | Kugle |
| Torus | Torus / ringformet emne |
| Coil | Spiral / helix |
| Pipe | Rør langs en sti |
| Rib | Forstærkningsribbe |
| Web | Forstærkningsvæg |
| Emboss | Prægning / hævet mønster |
| Deboss | Indprægning / forsænket mønster |
| Hole | Hul |
| Simple Hole | Almindeligt hul |
| Drilled Hole | Boret hul |
| Counterbore | Cylindrisk forsænkning |
| Countersink | Konisk forsænkning |
| Tapped Hole | Gevindhul |
| On Faces | På flader |
| At Sketch Points | Ved skitsepunkter |
| Thread | Gevind |
| Rectangular Pattern | Rektangulært mønster |
| Circular Pattern | Cirkulært mønster |
| Path Pattern | Mønster langs en sti |
| Mirror | Spejling |

### Modify – rediger

| Engelsk udtryk | Dansk oversættelse |
|---|---|
| Press Pull | Træk eller skub en flade / profil |
| Fillet | Afrunding |
| Chamfer | Affasning |
| Shell | Udhuling til en skal med vægtykkelse |
| Draft | Formslip / hældning på flader |
| Scale | Skalering |
| Combine | Kombinér emner |
| Split Body | Opdel emne |
| Offset Face | Forskyd flade |
| Replace Face | Erstat flade |
| Move | Flyt |
| Copy | Kopiér |
| Move/Copy | Flyt/kopiér |
| Align | Justér / flugt med |
| Remove | Fjern |

### Construct – konstruér

| Engelsk udtryk | Dansk oversættelse |
|---|---|
| Offset Plane | Forskudt plan |
| Plane at Angle | Plan i en vinkel |
| Plane Along Path | Plan langs en sti |

### Inspect – undersøg

| Engelsk udtryk | Dansk oversættelse |
|---|---|
| Measure | Mål |
| Section Analysis | Snitanalyse |
| Interference | Kontrol af sammenfald / overlap mellem emner |
| Compare Bodies | Sammenlign emner |
| Curvature Comb | Krumningskam |
| Curvature Map | Krumningskort |
| Zebra | Zebraanalyse af overfladekontinuitet |
| Draft Analysis | Analyse af formslip |
| Minimum Radius | Mindste krumningsradius |
| Isocurves | Isoparametriske kurver |
| Centre of Mass | Tyngdepunkt |

### Felter og værdier i scripts

Feltnavnene nedenfor er tekniske identifikatorer og **må ikke oversættes i JSON eller kommandoer**. Den danske tekst beskriver kun deres funktion.

| Felt / værdi | Dansk betydning |
|---|---|
| Feature | Modelleringsfunktion |
| \`extent\` | Udstrækning |
| \`distance\` | Afstand |
| \`distance2\` | Anden afstand |
| \`direction\` | Retning |
| \`positive\` | Positiv retning |
| \`negative\` | Negativ retning |
| \`symmetric\` | Symmetrisk |
| \`start_offset\` | Startforskydning |
| \`through_all\` | Gennem alt |
| \`taper\` | Konusvinkel |
| \`to\` | Til mål / reference |
| \`to_offset\` | Forskydning fra mål |
| \`from\` | Fra udgangspunkt / reference |
| \`operation\` | Operationstype |
| \`new_body\` | Nyt emne |
| \`join\` | Sammenføj |
| \`cut\` | Skær ud |
| \`intersect\` | Behold fællesmængden |
| \`targets\` | Mål-emner |
| \`profiles\` | Profiler |
| \`sketch\` | Skitse |
| \`thin\` | Tyndvægget ekstrudering |
| \`curves\` | Kurver |
| \`thickness\` | Tykkelse |
| \`thickness2\` | Anden tykkelse |
| \`side\` | Side / placering i forhold til kurven |
| \`center\` | Centreret |
| \`inside\` | Indvendigt |
| \`outside\` | Udvendigt |
| \`two_sides\` | Begge sider |
| \`angle\` | Vinkel |
| \`angle2\` | Anden vinkel |
| \`axis\` | Akse |
| \`radius\` | Radius |
| \`edges\` | Kanter |
| \`body\` | Emne |
| \`bodies\` | Emner |
| \`style\` | Stil / udformning |
| \`flip\` | Vend retning |
| \`length\` | Længde |
| \`width\` | Bredde |
| \`height\` | Højde |
| \`corner\` | Hjørne |
| \`base\` | Base / grundflade |
| \`diameter\` | Diameter |
| \`depth\` | Dybde |
| \`position\` | Position |
| \`hole\` | Hultype |
| \`simple\` | Almindeligt hul |
| \`drilled\` | Boret hul |
| \`counterbore\` | Cylindrisk forsænkning |
| \`countersink\` | Konisk forsænkning |
| \`thread\` | Gevind |
| \`faces\` | Flader |
| \`tangent_chain\` | Kæde af tangentforbundne flader |
| \`neutral\` | Neutralplan / neutral reference |
| \`pull\` | Trækretning |
| \`pattern\` | Gentagelsesmønster |
| \`features\` | Modelleringsfunktioner |
| \`plane\` | Plan |
| \`combine\` | Kombinér |
| \`origin\` | Origo / udgangspunkt |
| \`dir\` | Retningsvektor |
| \`definition\` | Definition af funktionen |
| \`inputs\` | Inputparametre |
| \`set\` | Felter og værdier, der skal ændres |
| \`expression\` | Udtryk / formel |
| \`recomputed\` | Genberegnet |
| \`restored\` | Gendannet |
| \`tolerance\` | Tolerance |
| \`spacing\` | Afstand mellem målepunkter |
| \`deviation\` | Afvigelse |
| \`resolution\` | Opløsning |
| \`only_in_a\` | Materiale kun i emne A |
| \`only_in_b\` | Materiale kun i emne B |
| \`identical\` | Identiske / uden registrerede afvigelser |
| \`verdict\` | Konklusion |
| \`warnings\` | Advarsler |
| \`color\` | Farvelægning |
| \`max_points\` | Maksimalt antal målepunkter |
| \`sampling.spacing_mm\` | Anvendt afstand mellem samplingpunkter i mm |
| \`faces_a\` | Flader på emne A |
| \`faces_b\` | Flader på emne B |
