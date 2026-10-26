# Sheet Metal (pladekonstruktion)

Fanen **SHEET METAL** bruges til at fremstille emner af plade med konstant tykkelse. Pladen følger en **Sheet Metal Rule (pladeregel)** med pladetykkelse, bukkeradius og K-faktor, som indstilles under **Sheet Metal Rules** (`sheet.manage_rules`).

- **Flange (flange)** (`sheet.flange`): Base Flange fra en skitseprofil, Contour Flange fra en åben skitse eller Edge Flange fra en kant på emnet.
- **Hem (ombukning)** (`sheet.hem`) og **Fold (buk)** (`sheet.fold`) langs en skitselinje.
- **Unfold / Refold (udfold / genbuk)** (`sheet.unfold`, `sheet.refold`) til at arbejde med den udfoldede plade.
- **Convert (konvertér)** (`sheet.convert`) omdanner en plade med konstant tykkelse til et Sheet Metal-emne eller, når bukkanter er angivet, et massivt eller udhulet emne.
- **Create Flat Pattern (opret udfoldning)** (`sheet.flat_pattern`) viser den udfoldede plade med bukkelinjer og bukketabel og kan eksportere DXF eller bukketabel som CSV via gemmedialogen.

Bukninger følger pladereglens K-faktor, så de udfoldede længder svarer til dem, en kantpresse vil fremstille. Tests sammenligner de udfoldede længder med manuelt beregnede bukketillæg.

En Edge Flange bruger kun tilbagerykningen (**setback**) fra sin egen kant. Slutter kanten i et indvendigt hjørne på basispladen, hvor pladen fortsætter forbi den som benet på et L, bevares resten af pladen. Ved bukkets ende skæres reglens **Bend Relief (bukkeaflastning)** med `relief_width` og `relief_depth` (Relief Width og Relief Depth i Sheet Metal Rules): en slids med den angivne bredde, fra en afstand svarende til relief-dybden før bukkelinjen til en afstand svarende til relief-bredden forbi flangens modsatte kant i udfoldningen. Bukningen og flangen frigøres dermed fra basispladen, både bukket og udfoldet. Kan slidsen ikke skæres, f.eks. ved bredde under 0,01 mm, hvis den er lige så bred som basispladen ved bukket, eller hvis den ville dele pladen i to, udelades den. Flange-funktionen viser en advarsel med placeringen.

Hvis en udskæring fjerner en del af en kant, eksempelvis et afskåret hjørne eller et hak midt på kanten, følger en Edge Flange kun den sammenhængende kantstrækning, der er nærmest det valgte punkt. Setback trækkes kun fra den strækning. Basispladen på den anden side af hakket beholder sin kant, og et valg dér kan oprette en separat flange i samme eller en senere Feature. Hvis der stadig er materiale ved bukkets ende, tilføjes Bend Relief som ovenfor. En kant, der er fjernet fuldstændigt, kan ikke vælges.

Setback skæres ud fra basispladens oprindeligt tegnede kontur. Hvis bukkelinjen krydser en bue i konturen, f.eks. ved et afrundet hjørne, bevares buen og afsluttes ved skæringen, både i udfoldningen og DXF-filen. Friformskurver som ellipser og splines, der skæres af setback, tilnærmes med tangentbuer inden for 0,001 mm.

Når to bukninger fra samme Edge Flange mødes ved et hjørne på basispladen, bestemmer reglens **Corner Relief Shape (hjørneaflastningens form)** (`corner_relief_shape`) udskæringen. Standardværdien `notch` skærer ikke yderligere materiale væk: bukningerne slutter vinkelret ved hjørnet og efterlader et trin i udfoldningen mellem bukkets ende og flangens lukkede ende. `tear` skærer heller ikke ekstra materiale og efterlader intet trin. Begge bukninger går frem til skæringspunktet for bukkelinjerne, og deres ender skæres lige fra hjørnet til begyndelsen af flangernes lukkede ender. I den bukkede model mødes bukningerne ved hjørnet, mens der er den angivne Gap mellem flangerne. Bukketabellen angiver en revet bukning mellem midtpunkterne på dens to ender. Hvis `tear` ville give krydsende snit i udfoldningen eller få bukningerne til at støde sammen, eksempelvis ved meget forskellige vinkler, bruges `notch` i stedet, og der vises en advarsel.

`round` og `rectangular` skærer en aflastning med størrelsen fra reglens **Corner Relief** centreret på bukkelinjernes skæringspunkt. For `round` skæres en rund aflastning i basispladen med halvdelen af størrelsen som radius; for `rectangular` skæres et kvadrat med halvdelen af størrelsen som sidelængde. Begge bukninger stopper med et lige snit, hvor aflastningen begynder, mens flangernes ender bevares. Aflastningen indgår i den bukkede model, udfoldningen, bukketabellen og DXF-filen; en rund aflastning er en bue i DXF. Ved ikke-retvinklede hjørner følger den runde aflastning basispladens vinkel, og den firkantede bliver drageformet med snit vinkelret på kanterne.

Ændring af formen i pladereglen ændrer alle emner, der bruger den. En aflastning, der ikke kan være der (mindre end 0,01 mm, fjerner hele bukket eller rækker til en anden kant), udelades med en advarsel. Hjørneaflastninger beregnes samlet, så rækkefølgen er uden betydning. To overlappende aflastninger udelades begge med en advarsel, der henviser til den anden.

```json
{"command":"sheet.manage_rules","params":{"name":"Sheet 2 mm","thickness":"2 mm","k_factor":0.5,
 "bend_radius":"1 mm","corner_relief_shape":"round","corner_relief":"8 mm","active":true}}
```

Reglens form og størrelse gælder som standard. En Edge Flange kan tilsidesætte dem for hjørnerne mellem sine egne flanger med `corner_relief_shape` (`notch`, `round`, `rectangular` eller `tear`) og `corner_relief` (størrelse som længde eller udtryk), eller med Corner Relief Shape og Corner Relief i Flange-dialogen. Valget Rule og et tomt størrelsesfelt bruger pladereglen. Felterne kan også tilsidesættes hver for sig, så det udeladte følger reglen. Aflastningen gælder kun hjørner mellem flanger oprettet af samme Feature. To Edge Flange-funktioner på samme emne kan derfor bruge forskellige aflastninger under samme regel. Base Flange og Contour Flange bruger altid reglens hjørner og afviser begge parametre. Gap og reglens øvrige indstillinger gælder stadig for alle flanger. Funktionsvalget gemmes i designet (filformat 23).

```json
{"command":"sheet.flange","params":{"edges":[[60,0,2],[120,45,2],[60,90,2],[0,45,2]],
 "height":20,"corner_relief_shape":"round","corner_relief":"6 mm"}}
```

Når Edge Flanges fra samme Feature mødes ved et hjørne, tilpasses enderne til hinanden uanset bukkevinkel. Hver ende skæres vinkelret gennem pladetykkelsen langs en linje i udfoldningen, så de indvendige kanter løber parallelt med reglens **Gap (spalte)** imellem sig på hver side af planet midt mellem flangerne. Ved 72° bliver enderne skrå i udfoldningen; ikke-retvinklede hjørner lukkes på samme måde. Ved 90° svarer hjørnet til reglens tidligere standard. Kan hjørnet ikke lukkes, f.eks. ved Gap under 0,01 mm, modsat bukkeretning eller overlappende ender i udfoldningen, bevares det åbent med en advarsel. En flange oprettet af en senere Feature ændrer ikke det tidligere hjørne: Den gamle flange beholder sin længde, og den nye slutter ved sit buk.

Hver flange i samme Edge Flange-feature kan have sin egen vinkel og højde. Et element i `edges` kan være et punkt `[x, y, z]`, som bruger funktionens `height` og `angle`, eller et objekt `{"point": [x, y, z], "angle": "105 deg", "height": "30 mm"}`, hvor `angle` og `height` (hver for sig eller begge) tilsidesætter værdierne for netop den kant. De er parameterudtryk som funktionens øvrige værdier og optræder som egne input (`Edge angle`, `Edge height`) efter `Height` og `Angle`. Hjørner mellem sådanne flanger lukkes med reglens Gap mellem enderne på hver side af midterplanet. En højere flange beholder afslutningslinjen op til sin egen top. Det er måden at lukke hjørner ved forskellige vinkler på, da flanger fra separate funktioner ikke lukkes mod hinanden. Hvis flere kanter vælges i Flange-dialogen, vises en række pr. kant med vinkel og højde. Et tomt felt bruger den fælles værdi ovenfor.

```json
{"command":"sheet.flange","params":{"edges":[[60,0,2],{"point":[120,45,2],"angle":"90 deg"},
 {"point":[60,90,2],"angle":"105 deg","height":"30 mm"},{"point":[0,45,2],"angle":"120 deg","height":"25 mm"}],
 "height":38.6,"angle":"75 deg"}}
```

En flange kan også placeres på yderkanten af en eksisterende flange som en **Return (ombukket kant)**, eksempelvis en indadvendt kant langs toppen af en bakkes vægge. Return følger hele yderkanten og dermed flangens lukkede afslutning, uanset om den er skrå. To Returns i samme Feature, som mødes ved et hjørne på basispladen, lukkes også mod hinanden med reglens Gap. De geringsskæres på begge sider af et plan, som følger middelretningen af panelernes normaler, halverer vinklen mellem deres bukkelinjer og går gennem skæringen mellem de linjer, panelerne begynder på. Returns i samme plan (f.eks. vandrette indadvendte kanter på en bakke) mødes dermed på hjørnets vinkelhalveringslinje som en billedramme. Hvert panel får en skrå afslutning i udfoldningen.

Hvert buk stopper med et lige snit dér, hvor hele dets bredde har mindst halvdelen af Gap til planet, og senest ved enden af den flange, det står på. Det kan efterlade et kort frit stykke af flangens kant. Returns bukket i modsatte retninger passerer hinanden. En Return uden nabo ved hjørnet slutter med sin støtteflange. Hvis en naboflange hælder ind over den, afkortes Return, så den har mindst samme afstand som støtteflangens afslutning. Returns, der ikke kan lukkes, bliver stående åbne med en advarsel. Returns i forskellige Features tilpasses heller ikke hinanden; opret derfor Returns omkring samme hjørne i én Feature. Corner Relief og Tear ændrer ikke denne regel, fordi flangernes afslutninger bevares.

```json
{"command":"sheet.flange","params":{"edges":[[60,2,20],[118,45,20],[60,88,20],[2,45,20]],
 "height":12,"angle":"90 deg"}}
```

Eksemplet viser de fire indadvendte Returns på en bakke på 120 × 90 mm i 2 mm plade med 20 mm høje vægge ved 90°. Punkterne ligger midt på hver vægs øverste kant på indersiden, så Returns vender ind over bunden.

En flange kan også placeres på en ret sidekant af en eksisterende flange. Vælg den og den tilstødende kant på støttepanelet i samme Feature for at lukke de nye flanger på tværs af det eksisterende buk. Panelerne geringsskæres med reglens Gap. Corner Relief med `round` eller `rectangular` skærer en tilsvarende slids tværs over den eksisterende bukkezone i udfoldningen. Slidsens bredde bestemmes af `corner_relief` og bevares ved Unfold, Refold og DXF-eksport. `notch` bevarer Gap uden et ekstra snit. `tear` understøttes endnu ikke på tværs af et eksisterende buk og giver en advarsel samt Notch som alternativ. Hvis et geringssnit ville overlappe i udfoldningen, efterlades hjørnet åbent med en advarsel. Setback på sidekanter af buede paneler eller kun delvist bevarede kanter understøttes endnu ikke. Kanten mellem et panel og dets eksisterende buk er ikke en fri kant. Se `examples/regressions/sheet-side-flange-corner.json` for et lille eksempel.

**Convert (konvertér)** kan også arbejde med et massivt eller udhulet emne. Vælg den flade, der skal forblive fast, og de skarpe kanter, som skal blive til bukninger (`bends`, Bend Edges i dialogen). Plane flader, som kan nås fra den faste flade på tværs af de valgte kanter, bliver pladens flader. Et udhulet emne bevarer vægge og tykkelse. For andre massive emner bliver de valgte flader pladens yderside, og reglens tykkelse lægges ind i emnet. Hver skarp kant bevarer positionen for de to teoretiske fladers skæring, så emnets ydre mål bevares. Udfoldningen beregnes ud fra udvendige længder minus bukkeafkortning pr. buk: `2·(R + T)·tan(θ/2) − θ·(R + K·T)`.

En bukkekant kan vælges på begge sider af et udhulet emne. Bukninger kan gå i begge retninger, følge efter hinanden og have vilkårlige vinkler, f.eks. 72° på en konisk bakke. En bukning modelleret som Chamfer (en plan strimmel mellem fladerne, som repræsenterer en korde i bukkeradius) vælges midt på strimlen. De to flader forbindes da ved deres virtuelle skarpe hjørne med reglens radius, og strimlen bliver ikke en separat pladeflade. En allerede afrundet bukning (en cylinderflade tangent til to plane flader eller en Fillet) vælges på cylinderfladen på begge sider. Den omdannes med reglens radius og K-faktor, ikke den oprindeligt modellerede radius, så udfoldningen følger reglen.

Resultatet er et almindeligt Sheet Metal-emne med Edge Flange, Cut, Unfold, Flat Pattern og eksport. Det følger det oprindelige emne ved parameterændringer, fordi kanter og fast flade genfindes ved navn. Uden Bend Edges forventer Convert som hidtil en plade. Huller, slidser, afrundede hjørner og hak bevarer deres cirkler og buer i udfoldning og DXF, selv hvis STEP-filer opdeler en kurve i flere stykker. Huller og slidser i alle pladeflader bevares som udskæringer. Forsænkninger med plan bund på højst halvdelen af pladetykkelsen bevares som lommer, også på begge sider af et udhulet emne. Endnu ikke understøttet: andre buede flader end bukninger (valset plade), huller og graveringer der går ind i et buk, lommer dybere end halvdelen af tykkelsen når Bend Edges bruges, samt bukninger der lukker en ring. Lav disse udskæringer efter Convert.

En plade kan have fordybninger i en af sine flader, f.eks. en graveret tekst eller en cylindrisk forsænkning. Fordybningen skal have en plan bund og vægge vinkelret på pladen og må højst være halvt så dyb som pladetykkelsen. Hver fordybning bliver en **Pocket (lomme)** i Sheet Metal-emnet og bevares ved Convert. Pladen beholder sin tykkelse. Udfoldningen viser hele pladen med fordybningen under `pockets` med kontur, side og dybde. I DXF tegnes konturen på et Pocket-lag for den pågældende dybde, adskilt fra gennemgående udskæringer, så laseren kan markere den i stedet for at skære igennem. Fordybningen må ligge på den valgte eller modsatte flade. Fordybninger dybere end halvdelen af tykkelsen, fordybninger på begge sider og plader, hvor fordybningerne fylder mere end de omgivende flader (en tyndere plade med forhøjede dele), afvises med en forklaring. Fordybningen ligger i en åbning af den flade, den er skåret i. Flader på benene af et bukket emne opfylder derfor ikke denne betingelse: Et sådant emne skal konverteres med Bend Edges. En bukning på tværs af en Pocket afvises, så placer Fold eller Flange uden om graveringen.

```json
{"command":"solid.box","params":{"length":100,"width":60,"height":30,"body_name":"Tray"}}
{"command":"solid.shell","params":{"faces":[[50,30,30]],"thickness":2}}
{"command":"sheet.convert","params":{"body":"Tray","face":[50,30,0],
 "bends":[[50,0,0],[100,30,0],[50,60,0],[0,30,0]]}}
```

`face` kan også angives med navnet fra `model.faces`, eksempelvis `"face":"F1:+z"`. Det gør det unødvendigt selv at beregne et punkt på en flade med huller eller buet kontur. `model.faces` indeholder et `point`, der ligger på hver flade.

Til værkstedstegninger og regneark returnerer `sheet.export_bend_table` som standard CSV. Vælg `format: "json"` for strukturerede data. Tabellen indeholder pladeregel, tykkelse, bukkevinkler, radier, bukketillæg og koordinater for bukkelinjernes midte på udfoldningen. Længder angives i millimeter og vinkler i grader. Et buk står kun i tabellen, hvor bukkelinjen har materiale: En bukning forkortet af en udskæring får længde og resterende endepunkter, mens en bukning opdelt af udskæringer får én række pr. del med samme bukkenummer. Udfoldningen og DXF viser de samme delstykker. Angiv `path` for at gemme filen, eller udelad `path` for at få indholdet som kommandosvar. Som standard vælges det seneste Sheet Metal-emne under `body`. Eksport tilføjer ikke noget på Timeline og bruger ikke et Undo-trin.

```json
{"command":"sheet.export_bend_table","params":{"format":"csv","path":"bracket-bends.csv"}}
{"command":"sheet.export_dxf","params":{"path":"bracket-flat.dxf"}}
```

DXF-filen er en AutoCAD 2000-tegning (`AC1015`) i millimeter, beregnet til nesting- eller CAM-software på et laserskæreværksted. Hver linjetype ligger på sit eget Layer med en farve (AutoCADs farveindeks) og, for bukninger, en linjetype defineret i filen.

| Layer | Farve | Linjetype | Indeholder |
|---|---|---|---|
| `OUTLINE` | 7, hvid eller sort | Kontinuerlig | Den udvendige kontur for hvert pladestykke |
| `CUT` | 4, cyan | Kontinuerlig | Indvendige udskæringer: huller, slidser og løse dele skabt af et snit (f.eks. midten af en ring) |
| `TEXT` | 2, gul | Kontinuerlig | Bogstaver skåret gennem pladen med deres indre konturer; kun med `"text": "cut"` (standard) |
| `ETCH` | 6, magenta | Kontinuerlig | Bogstaver som enkeltkonturer, der skal markeres og ikke gennemskæres: `"text": "etch"`; også tekst bearbejdet til en bestemt dybde |
| `BEND_UP`, `BEND_DOWN` | 1, rød / 5, blå | `DASHED` | Bukkelinjernes midterlinjer fordelt efter bukkeretning |
| `BEND_NOTES` | 8, grå | Kontinuerlig | Noter ud for bukkelinjerne, kun med `"bend_notes": true` |
| `POCKET_…` | 3, grøn | Kontinuerlig | Fordybninger, ét Layer pr. dybde |
| `PERFORATION` | 3, grøn | Kontinuerlig | Huller i perforerede områder |

`OUTLINE`, `CUT`, `BEND_UP` og `BEND_DOWN` findes altid i DXF-filens Layer-tabel. De øvrige tilføjes kun, hvis emnet indeholder den pågældende geometri. Indvendige konturer (`CUT`, `TEXT`) skrives før den udvendige kontur, så programmer, der følger filrækkefølgen, skærer indefra og ud.

Runde huller bliver `CIRCLE`, cirkelbuer `ARC`, hele ellipser `ELLIPSE` og ellipsebuer elliptiske `ELLIPSE`-elementer. Spline-segmenter, tekstkonturer og andre keglesnitskurver bliver `SPLINE` af deres oprindelige grad; kurverne erstattes altså ikke af rette linjestykker. Hvis maskinen kun kan læse linjer og buer, kan du angive `"curves": "arcs"` i `sheet.export_dxf`. Så tilnærmes friformskurver med tangentbuer og linjer inden for 0,001 mm.

Bogstaver adskilles fra andre udskæringer ved deres oprindelse. En Cut fra en skitse med Sketch Text markeres ved modelberegningen som bogstavkonturer, inklusive indvendige huller. Der gemmes ingen ekstra oplysninger i designfilen. Hvis et bogstav overlapper en anden udskæring eller bliver klippet af pladens kant, er det blevet til én samlet kontur og forbliver på `CUT`.

```json
{"command":"sheet.export_dxf","params":{"path":"tray-flat.dxf","text":"etch","bend_notes":true}}
```

Med `"bend_notes": true` får hver bukkelinje en note på `BEND_NOTES`, f.eks. `B1 UP 90° R1` (bukkenummer, bukkeretning, vinkel og indvendig radius; `%%d` er AutoCADs gradtegn). Teksten er 3 mm høj, medmindre `note_height` ændres, og placeres med sit centrum en halv teksthøjde ved siden af linjens midtpunkt, parallelt med linjen og læsbart nedefra eller fra højre. Hvis udskæringer har opdelt en bukning, får hvert stykke en note. Noter er slået fra som standard, fordi CAM-software ellers kunne komme til at markere dem.

Et design med flere Sheet Metal-emner kan eksportere én DXF-fil pr. emne med `"per_body": true`. Filnavnene bliver `<path>-<body>.dxf`. Tegn, der ikke må indgå i filnavne, bliver `_`, og navnesammenfald får suffikser som `_2`, `_3` osv. Resultatet indeholder filerne under `files`. Udelades `path`, returneres DXF-teksterne i stedet.

Base Flange-profiler bevarer deres indvendige huller. Gennemgående Cut, der krydser pladens kant, bliver til hak i udfoldningens kontur. Overlappende og gentagne udskæringer fjerner kun deres samlede areal én gang. Flat Pattern-preview, DXF og det udfoldede solid bruger de samme materialegrænser. Linjer og cirkelbuer forbliver præcise. Ellipser, splines og tekst bevarer deres oprindelige kurver i udfoldningen og det udfoldede solid. Det bukkede solid tilnærmer dem med tangentbuer inden for 0,001 mm, mens punktlister bruger rette segmenter inden for 0,001 mm. DXF bevarer de oprindelige kurver. Flat Pattern-JSON angiver hver udskæring to gange i samme rækkefølge: som punkter (`cutouts`) og som præcis geometri (`cutout_shapes`). Runde huller angives som `{"circle": {"center", "radius"}}`; øvrige konturer bruger `line`, `arc` (centrum, radius, start og vinkel i grader) og `polyline` (ét element pr. ellipsekvart, spline eller bogstavkurve).

Cut kan efterlade separate materialestykker, eksempelvis centrum af en ringudskæring. Disse forbliver én logisk Sheet Metal-gruppe gennem Unfold/Refold og eksporteres som separate konturer i udfoldningen. Hvis en udskæring løsner et stykke fra basispladen, f.eks. en flange skåret fri langs bukningen, bevares stykket både bukket og udfoldet. En advarsel angiver antal løsrevne stykker, deres areal og placering i udfoldningen. Øer omgivet af andet materiale, f.eks. midten af en ring eller et bogstavs indre, tælles ikke som løsrevne. Store profiler, der overstiger geometriberegningens grænse, giver en fejl frem for en ufuldstændig eksport.

Box Cut og Cylinder Cut, hvor cylinderens akse er vinkelret på pladen, bearbejder pladen som en Extrude af værktøjets kontur: Gennem hele pladen bliver det en udskæring (rund for Cylinder), og delvist gennem pladen bliver det en Pocket (fordybning). Udskæringer med sådanne grundformer i andre vinkler eller på tværs af et bukket område afvises.

Hole bearbejder pladen fra en plan flade på basispladen eller en flange, på begge sider og både bukket og udfoldet, vinkelret på det valgte panel. Et gennemgående hul, almindeligt eller med gevind, bliver en rund udskæring i Flat Pattern og en `CIRCLE` i DXF. Counterbore eller et blindhul med plan bund bliver en rund Pocket. Skråtstillede huller, huller boret ind i pladekanten, Countersink og borede spidser understøttes ikke.

Parallelle Extrude Cut uden Taper bevarer også lave Pocket-fordybninger gennem Unfold/Refold. Bund og sidevægge bliver ved med at være solid geometri. Overlappende udskæringer fra modsatte sider bliver et gennemgående hul, hvor hele pladetykkelsen er fjernet. Flat Pattern-JSON angiver `pockets` med side, dybde samt bundens og toppens tykkelseskoordinater i millimeter. I DXF bruger Pocket-konturer Layers med navnene `POCKET_BOTTOM_Z…_TO_…_MM`, `POCKET_TOP_Z…_TO_…_MM` eller `POCKET_INTERNAL_Z…_TO_…_MM`. Det er bearbejdningsoperationer: Importér derfor ikke alle Pocket-lag som gennemgående skærekonturer i CAM. Tekst bearbejdet til en bestemt dybde ligger som markering på `ETCH`.

Extrude Intersect på en lige basisplade eller en udfoldet plade bevarer materialet inden for både værktøjets kontur og dets dybdeinterval, inklusive indvendige huller, løsrevne øer og tidligere Pockets. Ved en lav Intersect gemmes fjernede lag over og under det bevarede materiale som Top- og Bottom-Pockets. Pladereglen bevarer den oprindelige pladetykkelse, selv hvis det tilbageværende solid er tyndere. En helt tom skæring fjerner både pladen og dens metadata. DXF gemmer Pocket-dybder i Layer-navnene; almindelig import af DXF som skitsegeometri gendanner ikke bearbejdningsdybder automatisk. Alle bevarede eller fjernede lag med positiv dybde skal opfylde geometrikernens opløsning på 0,000001 mm for Extrude. Mindre lag giver en fejl frem for at blive afrundet væk. To Object kan bruges til at trimme værktøjet mod et ret prisme og bevarer den faktisk trimmede kontur. Buede værktøjer med varierende dybde afvises, før pladens geometri ændres. Pocket-profiler kan følge plane paneler under formning, men en Pocket på tværs af et bukkeområde kan endnu ikke Refold. Bearbejdning af materiale i bukkede områder, Cut med Taper og andre ikke-understøttede 3D Sheet Metal Cut/Intersect giver fejl før ændringer, så tidligere fjernet materiale ikke utilsigtet gendannes ved genberegning.

Andre Features, der ændrer pladens materiale uden at Sheet Metal-modellen kender ændringen, understøttes endnu ikke i Flat Pattern. Det gælder f.eks. Fillet eller Chamfer på et hjørne, Extrude Join samt Mirror eller Combine, der sammenføjes med pladen. Funktionen viser en advarsel, og så længe emnet indeholder ændringen, afvises Flat Pattern, DXF og bukketabel med en fejl, der navngiver funktionen. Dermed eksporteres der ikke en plade, som afviger fra modellen. En senere Sheet Metal-feature genopbygger pladen ud fra Sheet Metal-funktionerne og fortæller, at ændringen ikke er bevaret. Flytning af emnet, opdeling af flader og kosmetiske gevind påvirker ikke udfoldningen.

En skitse kan placeres på en plan flade på Sheet Metal-emnet, både basispladen og en flange, på begge sider. Vælg fladen, eller giv `sketch.create` punktargumentet `plane: {face: [x, y, z]}`. Skitsen knyttes derefter til panelet i stedet for en certificeret kildeflade. Den gemmer fladens navn, `sheet:<body>:<panel>:<top|bottom>`, hvor panelet er `base` eller flangens Feature-tag. Fladen genfindes efter parameterændringer som tykkelse eller flangens vinkel, ved Unfold/Refold, ved genåbning og efter omdøbning af emnet. Skitsekoordinaterne følger udfoldningen, så punkter på en flange ligger samme sted som i Flat Pattern og DXF. På undersiden vender y-aksen modsat, sådan som den ses nedefra. Hvis fladen forsvinder, fordi flangen deaktiveres eller slettes, melder skitsen om en mistet tilknytning frem for at flytte til en anden flade. **Redefine Sketch Plane (omdefinér skitseplan)** knytter skitsen til en ny flade. `fixed: true` fastholder derimod det valgte plan i rummet.

Alle flader på en bukket plade har tilsvarende navne, som `model.faces` viser under `sheet_face`: `<panel>:top` og `<panel>:bottom`; `<panel>:rim.<edge>` for en kant (ID fra en linje i basispladens skitse eller flangens `far`, `side0`, `side1`; `rim.bend0`/`rim.bend1` for bukkets plane ender og `rim.ext0`/`rim.ext1` hvor en flange fortsætter forbi bukket mod en nabo); samt `<flange>:bend.inner` og `<flange>:bend.outer` for bukkets buede flader. Kommandoer, der accepterer fladenavne, kan også bruge disse, f.eks. `sheet:Body1:F3.1:top`. Fladen genfindes direkte fra Sheet Metal-modellen, så navnet følger den ved ændringer i størrelser, og når andre flanger tilføjes eller fjernes. Flader, Sheet Metal-modellen ikke beskriver, f.eks. væggen i et hul, får ikke sådanne navne.

## Ordliste – engelske Sheet Metal-udtryk på dansk

De engelske betegnelser og tekniske navne er bevaret i dokumentationen, så de svarer til SolidCraft og kan bruges direkte i scripts.

| Engelsk udtryk | Dansk oversættelse |
|---|---|
| Sheet Metal | Pladekonstruktion |
| Sheet Metal Rule | Pladeregel |
| Sheet Metal Rules | Pladeregler |
| Thickness | Pladetykkelse |
| Bend Radius | Bukkeradius |
| K-factor | K-faktor |
| Bend Allowance | Bukketillæg |
| Bend Deduction | Bukkeafkortning |
| Press Brake | Kantpresse |
| Flange | Flange / bukket kant |
| Base Flange | Basisflange |
| Contour Flange | Konturflange |
| Edge Flange | Kantflange |
| Return | Ombukket kant |
| Hem | Ombukning |
| Fold | Buk |
| Unfold | Udfold |
| Refold | Genbuk |
| Convert | Konvertér |
| Flat Pattern | Udfoldning |
| Create Flat Pattern | Opret udfoldning |
| Bend Line | Bukkelinje |
| Bend Table | Bukketabel |
| Setback | Tilbagerykning |
| Bend Relief | Bukkeaflastning |
| Relief Width | Aflastningsbredde |
| Relief Depth | Aflastningsdybde |
| Corner Relief | Hjørneaflastning |
| Corner Relief Shape | Hjørneaflastningens form |
| Notch | Hak / udskæring |
| Tear | Åben hjørneaflastning |
| Round | Rund |
| Rectangular | Rektangulær |
| Gap | Spalte / afstand |
| Mitre | Geringssamling |
| Bend Edges | Bukkekanter |
| Body | Emne |
| Solid Body | Massivt emne |
| Shelled Body | Udhulet emne |
| Panel | Pladeflade |
| Pocket | Fordybning / lomme |
| Engraving | Gravering |
| Counterbore | Cylindrisk forsænkning |
| Countersink | Konisk forsænkning |
| Through Cut | Gennemgående udskæring |
| Blind Hole | Blindhul |
| Hole | Hul |
| Tapped Hole | Gevindhul |
| Extrude | Ekstrudering |
| Cut | Skær ud |
| Join | Sammenføj |
| Intersect | Fællesmængde |
| Taper | Konusvinkel |
| To Object | Til objekt |
| Fillet | Afrunding |
| Chamfer | Affasning |
| Mirror | Spejling |
| Combine | Kombinér emner |
| Sketch | Skitse |
| Redefine Sketch Plane | Omdefinér skitseplan |
| DXF | DXF-tegnefil |
| CAM | Computerstøttet fremstilling |
| Nesting | Placering af udskæringer på en plade |
| Layer | Lag |
| Outline | Kontur |
| Etch | Markering / gravering |
| Perforation | Perforering |
| Centre Line | Midterlinje |
| Tangent Arc | Tangentbue |
| Spline | Splinekurve |
| Bounding Geometry | Afgrænsende geometri |
| Feature | Modelleringsfunktion |
| Timeline | Tidslinje |
| Undo | Fortryd |

### Kommandoer, felter og DXF-lag

Feltnavne og kommandoer er identifikatorer: skriv dem altid uændret i JSON, scripts og DXF.

| Teknisk navn | Dansk betydning |
|---|---|
| `sheet.manage_rules` | Administrér pladeregler |
| `sheet.flange` | Opret flange |
| `sheet.hem` | Opret ombukning |
| `sheet.fold` | Buk plade |
| `sheet.unfold` | Udfold plade |
| `sheet.refold` | Genbuk plade |
| `sheet.convert` | Konvertér til pladekonstruktion |
| `sheet.flat_pattern` | Opret udfoldning |
| `sheet.export_bend_table` | Eksportér bukketabel |
| `sheet.export_dxf` | Eksportér DXF |
| `sketch.create` | Opret skitse |
| `model.faces` | Vis emnets flader |
| `thickness` | Tykkelse |
| `bend_radius` | Bukkeradius |
| `k_factor` | K-faktor |
| `relief_width` | Bukkeaflastningens bredde |
| `relief_depth` | Bukkeaflastningens dybde |
| `corner_relief_shape` | Hjørneaflastningens form |
| `corner_relief` | Hjørneaflastningens størrelse |
| `notch` | Hak |
| `tear` | Åben aflastning |
| `round` | Rund |
| `rectangular` | Rektangulær |
| `gap` | Spalte |
| `edges` | Kanter |
| `bends` | Bukninger |
| `height` | Højde |
| `angle` | Vinkel |
| `face` | Flade |
| `body` | Emne |
| `point` | Punkt |
| `fixed` | Fastlåst |
| `format` | Filformat |
| `path` | Filsti |
| `pockets` | Fordybninger |
| `cutouts` | Udskæringer som punkter |
| `cutout_shapes` | Præcise udskæringskonturer |
| `text` | Teksttype |
| `bend_notes` | Bukkenoter |
| `note_height` | Notehøjde |
| `curves` | Kurver |
| `arcs` | Buer |
| `per_body` | Én eksportfil pr. emne |
| `OUTLINE` | Udvendig kontur |
| `CUT` | Udskæringer |
| `TEXT` | Gennemskåret tekst |
| `ETCH` | Markering / gravering |
| `BEND_UP` | Buk opad |
| `BEND_DOWN` | Buk nedad |
| `BEND_NOTES` | Bukkenoter |
| `POCKET_…` | Fordybninger efter dybde |
| `PERFORATION` | Perforeringer |
| `DASHED` | Stiplet linje |
| `CIRCLE` | Cirkel |
| `ARC` | Cirkelbue |
| `ELLIPSE` | Ellipse |
| `SPLINE` | Spline |
| `sheet_face` | Navngiven Sheet Metal-flade |

