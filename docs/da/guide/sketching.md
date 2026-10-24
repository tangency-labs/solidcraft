# Sketching and Constraints (skitsering og bindinger)

En **Sketch (skitse)** ligger på et plan: et af grundplanerne gennem origo, et konstruktionsplan eller en plan flade. Start med **Create Sketch (opret skitse)** (`sketch.create`), og vælg et plan, eller vælg først en flade. Du kan også højreklikke på et plan i visningsområdet eller designtræet og vælge Create Sketch. **Finish Sketch (afslut skitse)** vender tilbage til modellen.

I scripts betegner `XY`, `XZ` og `YZ` samt aliaserne `Top`, `Front` og `Right` grundplanerne. Navnene er reserverede, så konstruktionsplaner hverken kan oprettes eller omdøbes til dem, uanset store og små bogstaver. Hvis et ældre design har et konstruktionsplan med et af navnene, afvises et tvetydigt tekstargument. Brug i stedet et andet alias (`XY` for `Top`) til grundplanet, konstruktionsfunktionens numeriske ID eller et eksplicit plan som `{origin: [x,y,z], normal: [x,y,z]}`. Eksisterende gemte referencer ændres ikke, når designet åbnes.

Kurvernes visning tilpasses zoom og skærmskalering. Markering og fremhævning følger samme synlige kontur, også for små cirkler ved Fit og skitser i roterede komponenter. Udfyldning af profiler følger kurverne og bevarer hullerne. Zoom påvirker kun visningen, ikke skitsegeometri, mål eller Undo-historik. På lave vinduer eller ved forstørret brugerflade kan indstillingerne i Sketch Palette rulles, mens status for bindinger og knappen Finish Sketch forbliver synlige.

## Geometry (geometri)

**Point (punkt)** findes på standardværktøjslinjen Sketch › CREATE. Vælg værktøjet, og klik i skitsen for at placere et punkt. Tilpassede værktøjslinjer bevares. Vælg **Reset Toolbar (nulstil værktøjslinje)** i værktøjslinjens kontekstmenu for at gendanne standardknapperne.

Du kan tegne linjer og polylinjer (L), rektangler (to punkter R, tre punkter eller fra centrum), cirkler (centrum C, to punkter, tre punkter eller tangent), buer (tre punkter, centrum eller tangent), polygoner, aflange huller (Slots), ellipser, splines med tilpasningspunkter eller kontrolpunkter, keglesnitskurver, punkter og tekst. **Construction (konstruktionsgeometri)** (X) gør kurver til referencegeometri, som ikke danner profiler. Redigeringsværktøjerne omfatter Trim (beskær, T), Extend (forlæng), Break (opdel), Fillet (afrund), Chamfer (affas), Offset (forskyd, O), Mirror (spejl), Pattern (mønster), Move/Copy (flyt/kopiér) og Scale (skalér).

Når Trim er aktivt, kan du klikke på en del for at fjerne den eller holde museknappen nede og trække en bane hen over flere dele. De dele, banen krydser, fremhæves og fjernes samlet, når du slipper museknappen, som ét trin der kan fortrydes (`sketch.trim.path`).

Indstillingen **Construction Geometries (konstruktionsgeometri)** i Sketch Palette skjuler konstruktionskurver under arbejdet. Skjulte kurver kan ikke vælges med klik eller markeringsramme. Indstillingen gælder for den enkelte skitse og gemmes med designet. Centerlines (centerlinjer) er en særskilt linjetype og forbliver synlige. Projiceret geometri med konstruktionslinjetype, f.eks. et plans skæringsspor, styres derimod af **Projected Geometries (projiceret geometri)**. Indstillingen påvirker kun visningen; konstruktionskurver er i forvejen udelukket fra profiler.

Polygonværktøjerne viser kanterne, mens det andet punkt placeres, så du kan kontrollere retningen. Indskrevne og omskrevne polygoner bevarer deres hjælpecirkel og opdaterer konturen, når værdien **Sides (sider)** ændres.

**Text (tekst)** bruger som standard Liberation Sans. `sketch.text` og `sketch.edit_text` accepterer `font` som skrifttypefamilie, fuldt navn på en installeret skrifttype (f.eks. `"DejaVu Sans"`) eller sti til en `.ttf`- eller `.otf`-fil. Feltet **Font (skrifttype)** findes også i Edit Text-dialogen. Ukendte skrifttyper giver en fejl. Skrifttypen gemmes sammen med teksten, så den fortsat kan redigeres. `SOLIDCRAFT_FONT_DIRS` tilføjer mapper til søgningen efter installerede skrifttyper. Tekstens `height`, `angle` og koordinaterne i `at` kan være parameterudtryk som tekststrenge (`"height": "plate_height / 10"`, `"at": ["plate_width / 2", 5]`). Udtrykkene gemmes og følger ændringer i parametrene. Et almindeligt tal erstatter udtrykket; det sker også for de værdier, der ændres, når du flytter teksten eller trækker i håndtagene. Rotation ændrer ikke højden. En tekststreng uden parameternavn fortolkes som den beregnede værdi. Kommandoerne afviser ugyldige typer for `text`, `at`, `height` og `angle`, ikke-positive højder og udtryk, der ikke kan beregnes, i stedet for at bruge standardværdier. Udeladte felter bevarer deres standardværdier.

Højreklik på en bogstavflade eller dens kontur, og vælg **Edit Text (rediger tekst)**. Du kan også starte værktøjet først og vælge bogstavet. Alle konturer, der hører til teksten, fremhæves under redigeringen. Hvis en profil afgrænses af flere tekster eller anden geometri, kan programmet ikke identificere én tekstkilde. Brug **Select Other (vælg andet)** til at vælge tekstkonturen. Redigér tekst, skrifttype, X/Y-position på grundlinjen, højde eller vinkel, og bekræft med OK. Cancel bevarer originalen. Position og højde bruger dokumentets enheder. De fire numeriske felter accepterer udtryk; et parameterudtryk som `plate_width / 2` bevares og vises uændret ved næste redigering.

Hvis du vil redigere selve bogstavernes geometri, skal du højreklikke på teksten og vælge **Explode Text (opløs tekst)** (`sketch.break_link` med tekstens `link`). Konturerne bliver almindelige linjer, buer og kurver, som kan markeres, beskæres, målsættes og ekstruderes enkeltvis. Tekstobjektet forsvinder og kan ikke længere redigeres som tekst eller følge parametre. Undo gendanner det.

Tekst kan følge en kurve i skitsen via **Path (sti)**. Kurven omdannes til konstruktionsgeometri, så bogstaverne står på den uden at blive opdelt, når deres nedadgående dele krydser stien. Fjernes teksten fra stien, forbliver kurven konstruktionsgeometri; **Normal/Construction** ændrer den tilbage.

Træk i en tekstkontur for at flytte alle bogstaver samlet. Slip for at afslutte som ét Undo-trin, eller tryk Esc under trækket for at gendanne teksten. Den kan stadig redigeres. Til præcis flytning vælges en kontur og **Move/Copy (flyt/kopiér)** fra kontekstmenuen. Angiv **Distance X**, **Distance Y** og eventuelt **Angle** omkring tekstens grundlinje. Afstande kan angives i dokumentets enheder eller eksplicit som `1 in`. Aktivér **Create Copy (opret kopi)** for at bevare originalen og oprette en separat redigerbar kopi. Cancel ændrer intet. Projiceret geometri kan ikke trækkes frit.

**Project (projicér)** (P) og **Intersect (skæringsgeometri)** henter kanter fra modellen ind i skitsen som tilknyttet geometri, der følger senere ændringer. Når du tegner på en flade, projiceres dens kanter automatisk. En projiceret kant følger den oprindeligt valgte kant, også når den flyttes. Hvis kanten forsvinder, bevares projektionens seneste form, og skitsen markerer referencen som mistet. Med Project kan du vælge synlige kurver og punkter fra andre skitser samt modelkanter, flader, hjørnepunkter, emner, akser og planer og derefter trykke P. Hele den understøttede markering projiceres i ét Undo-trin. Afviste projektioner bevarer markeringen, så den kan rettes; en tom eller blandet markering starter det almindelige markeringsværktøj. Referencer til eksterne skitser bevarer kildeskitsen, selv hvis den aktive skitse har punkter eller kurver med samme navne.

En skitse på et konstruktionsplan, der er oprettet ud fra modellen (f.eks. gennem en skitselinje, gennem tre punkter eller midt mellem to flader), understøtter Project, Intersect, Include og tekst som andre skitseplaner. Referencerne følger med, når en parameter flytter planet.

Når scripts eller kontrolkanalen opretter skitsegeometri, returnerer hver kommando ID'er for de nye elementer (`created: {points, curves, constraints, dimensions}`). ID'erne er stabile og genbruges aldrig i samme skitse, heller ikke efter sletning. Derfor kan et script straks målsætte eller tilføje bindinger til den nye geometri.

Extend finder skæringer med præcise ellipser, rationelle keglesnitskurver og spline-segmenter. Den oprindelige linje eller cirkulære grundgeometri bevares og forlænges kun ud over det valgte endepunkt. Modstridende bindinger eller tvetydige overlap efterlader skitsen uændret.

Lukkede konvekse kæder af tangentforbundne linjer og cirkelbuer understøtter indadgående Offset, der kan opsluge et afrundet hjørne. Når forskydningen overstiger hjørneradius, mødes de tilstødende rette linjer i et skarpt hjørne. De tilbageværende linjer og endepunkters ID'er bevares. De fjernede buer vender tilbage med deres oprindelige ID'er, hvis en senere ændring genskaber deres radius. Profilvalg baseret på hele konturen følger ændringen gennem gemning og genåbning, så en ekstruderet kant kan forblive associativ. Eksisterende egnede lukkede analytiske Offset-funktioner opgraderes uden ændring af gemt geometri eller ID'er.

Overgangen kræver en lukket, konveks kontur med tangentforbindelser og et verificeret ikke-tomt område. En bue, der refereres individuelt af en binding eller en anden Offset, kan ikke fjernes automatisk. Hvis en allerede skarp kontur med tidligere fjernede buer skal forskydes igen, skal forbindelsen først brydes. Åbne, ikke-konvekse og blandede friformskæder beholder deres tidligere opførsel. **Break Offset Association (bryd Offset-forbindelse)** fastlåser den aktuelle kontur, inklusive interne buecentre og konstruktionsflag, så den kan redigeres uafhængigt.

Offset på glatte ellipser, keglesnitskurver og splines følger kilden efter ændrede mål, træk og genåbning. Den gemte kubiske spline har en maksimal normalafstandsfejl over hele spændet på 0,002 mm. Singulære tangenter, krumningsspidser, mulige selvskæringer og for mange opdelinger afvises. Forskyd disse kurver enkeltvis; blandede kurvekæder kræver foreløbig separat geometri. `sketch.offset.break` frigør en Offset til selvstændig redigering, og Undo gendanner forbindelsen. Ældre Offset-kurver, der blev samplet uafhængigt, forbliver uafhængige. Tilpasningspunkter på sådanne Offset-kurver skjules bortset fra endepunkterne på åbne kurver. Linjer og buer kan snappe til disse og forblive sammenfaldende, mens Offset følger kilden, så åbningen mellem en spline og dens Offset kan lukkes til en profil. Glatte åbne og nyoprettede glatte lukkede Offset-kurver kan forskydes igen, også efter begrænset genberegning af en ændret kilde. Nye lukkede kurver bruger periodisk kubisk tilpasning med sammenhængende tangent og krumning ved samlingen. Break Offset Association bevarer interpolationen og holder lukningspunktet sammen under træk. Sletning af et resultat fjerner også ubrugte interne tilpasningspunkter. En kilde kan ikke slettes, hvis dens afhængige geometri bevares.

Til eksplicit lukkede Fit Splines accepterer `sketch.spline.fit_point` værdien `periodic: true` og mindst tre forskellige punkter i cyklisk rækkefølge. Kurven lukkes automatisk. Eksisterende kald og ældre gemte lukkede splines bevarer naturlig interpolation; ældre associative lukkede Offsets beholder også deres gemte tilpasning. En ældre samling uden entydig normal tillader fortsat ikke en ekstra Offset. Valget om periodicitet gemmes på hver kurve og bevares ved gemning, genåbning og Break. Når du tegner en Fit Spline, kan du efter tre forskellige punkter klikke på startpunktet for at lukke den jævnt. Højreklik afslutter en almindelig naturlig tilpasning. Offset accepterer både ellipsehåndtag og tætte tilpassede kurver; andet klik bestemmer side og afstand.

Under tegning i en plan skitse ændrer **Construction (X)** linjetypen uden at afslutte værktøjet eller miste valgte punkter og indtastede værdier. Preview og fremhævning bevarer stiplede konstruktionslinjer og centerlinjers streg-prik-mønstre. Når kurver er markeret, ændrer Construction de valgte kurver som normalt.

Aktivér **3D Sketch (3D-skitse)** i Sketch Palette for at tegne linjer, Fit Splines og punkter uden for skitseplanet. Under placering kan du snappe til punkter i andre synlige skitser. En forbindelseslinje mellem to skitser oprettes ved at vælge Line og klikke på et endepunkt i hver kildeskitse. Hvert endepunkt bruger sin kildes plan, selv hvis det afviger fra den aktive skitse. Snappingen gemmer positioner, men opretter ikke associative referencer. Ny 3D-geometri følger skitsens komponentplacering, også den valgte forekomst og endnu ikke gemte positionsændringer. Klik på et tegnet Fit Point for at vise flytteaksen (triad), hvor pilene flytter langs verdensakserne X, Y og Z. Scripts angiver også verdenskoordinater til `sketch.line3d`, `sketch.spline3d`, `sketch.point3d` og `sketch.move3d`. Aktivér den ønskede forekomst, før du redigerer en eksplicit skitse i en komponent med flere placeringer.

Ældre tegnede eller frigjorte 3D-kurver bevarer deres gemte position i verdenskoordinater. Inkluderede og overfladeknyttede kurver med eksplicit komponentmarkør bevarer den betydning, selv efter forbindelsen brydes. Ældre filer, hvor referencer gik tabt efter forkert afgrænset opdatering på tværs af komponenter, kan kræve en ny tilknytning, når kilden er tilgængelig. De gemte punktværdier bevares ved åbning.

## Constraints and Dimensions (bindinger og mål)

**Horizontal/Vertical (vandret/lodret)** kan anvendes på en linje eller mellem to forskellige skitsepunkter, herunder origo, endepunkter og cirklers centrum. Ved to punkter vælges vandret eller lodret ud fra deres aktuelle placering. Hold **Shift** nær midten af en linje for at fremhæve og vælge dens midtpunkt som et af punkterne. Midtpunktet følger begge endepunkter, når linjen flyttes, ændrer længde eller roteres. To linjers midtpunkter kan også placeres vandret eller lodret i forhold til hinanden.

Fremhævning og det første valg er kun en forhåndsvisning; Esc annullerer uden at ændre skitsen. Når det andet punkt bekræftes, genbruges eller oprettes et skitsepunkt med en almindelig Midpoint-binding, hvorefter H/V-bindingen tilføjes. Det sker i ét trin, der kan fortrydes. Bindingerne kan vælges og slettes hver for sig; punktet bevares, hvis en af dem slettes. Efter en afrunding eller fasning henviser Midpoint-bindingen til den oprindelige fulde side, som bevares som hjælpegeometri, ligesom et manuelt bundet midtpunkt.

Scripts bruger samme kommando med en midtpunktsreference i arrayet `points`:

```json
{"command":"sketch.constraint.horizontal_vertical","params":{"points":[{"midpoint":"l1"},"c1.center"],"mode":"vertical"}}
```

`mode` kan være `horizontal` eller `vertical`. Udelades den, vælges retningen ud fra punkternes aktuelle placering.

SolidCraft har sin egen solver (dæmpet Gauss–Newton med analyse af frihedsgrader). Bindingerne omfatter Coincident (sammenfaldende), Horizontal/Vertical (vandret/lodret), Parallel (parallel), Perpendicular (vinkelret), Tangent (tangent), Equal (ens), Concentric (koncentrisk), Collinear (på samme linje), Midpoint (midtpunkt), Symmetry (symmetri), Fix (fastlås), Curvature (G2-krumning) og Polygon. **Sketch Dimension (skitsemål)** (D) opretter lineære, parallelle, vandrette, lodrette, punkt-til-linje-, linje-til-cirkel-, radius-, diameter- og vinkelmål. Hvert mål er en parameter (`d1`, `d2` osv.) og kan derfor indeholde et udtryk.

Tryk D, vælg geometrien, der skal målsættes, og skriv værdien. Så snart målet er fastlagt ved klik på en linje, cirkel eller bue eller på det andet punkt eller den anden linje, får værdifeltet fokus med den målte værdi markeret. Du kan straks skrive en ny værdi. Tal, enheder (`1 in`), udtryk (`2*width`) og parameternavne kan bruges. Bogstaver skrives i feltet og aktiverer ikke genvejstaster. Under indtastning følger målteksten markøren. Enter placerer den som ét Undo-trin, eller på standardplaceringen hvis markøren ikke er flyttet. Esc annullerer målet.

Du kan også placere målteksten før indtastning. Et klik på et tomt område i skitsen placerer teksten og beholder fokus i værdifeltet. Klikker du i stedet på et andet punkt eller en linje, ændres målet til en afstand eller vinkel. Tab lader markeringen vente på et klik til placering af teksten.

Mellem to punkter eller på en linje, der ikke flugter med en akse, bestemmer måltekstens placering, hvad der måles: på tværs af punkterne måles den direkte afstand, over eller under dem den vandrette afstand og ved siden af dem den lodrette afstand.

Mellem en linje og en cirkel eller bue måles afstanden vinkelret fra linjen til cirklens kant. Klik på linjen og derefter cirkelkanten eller omvendt. Når de berører hinanden, er afstanden 0. Hvis linjen går gennem cirklen, måles hvor langt kanten går forbi linjen. En bue måles i forhold til den cirkel, den ligger på. Hvis du vil måle til centrum, skal du vælge centrumpunktet i stedet for kanten. I scripts: `{"command": "sketch.dimension", "params": {"entities": ["l1", "c1"], "value": 5}}`.

Hvis geometrien allerede er fastlagt af andre bindinger og mål, tilføjes et nyt mål som **Driven Dimension (referencemål)** i stedet. Det vises i parentes, måler geometrien og har ingen redigerbar værdi.

Ved **Coincident (sammenfaldende)** vælger du to punkter, et punkt og en kurve (i vilkårlig rækkefølge) eller to forskellige rette linjer. To linjer forbindes ved det nærmeste par endepunkter. Den først valgte linje forbliver på plads, hvis de eksisterende bindinger tillader det. Vælg endepunkterne direkte, hvis et andet par skal forbindes.

Horizontal/Vertical kan også anvendes direkte på en markeret linje eller to markerede punkter. Brug Shift + klik til at markere det andet punkt, før bindingen vælges.

Ved Parallel, Perpendicular, Equal, Tangent, Concentric og Collinear markerer du præcis to skitsekurver (Shift + klik for den anden) og vælger bindingen, som anvendes med det samme. Du kan også starte bindingen og derefter vælge kurverne. En ufuldstændig eller blandet markering lader værktøjet fortsætte med almindeligt valg.

Coincident accepterer præcis to punkter eller et punkt og en kurve. Midpoint accepterer et punkt og en linje i vilkårlig rækkefølge. Symmetry accepterer to forskellige punkter og en symmetrilinje, uanset valgrækkefølgen. Vellykkede bindinger rydder markeringen; afviste bindinger bevarer den.

**Under-constrained (ikke fuldt bundet)** geometri er blå, mens **Fully Constrained (fuldt bundet)** geometri bliver mørk. Status viser de resterende frihedsgrader. Bindinger og mål, der ville gøre skitsen overbestemt, afvises. Du kan trække et punkt for at flytte det, mens solveren opretholder alle bindinger.

## Profiles (profiler)

Lukkede konturer bliver automatisk til profiler, også områder opdelt af krydsende linjer og øer inde i andre konturer. Extrude, Revolve, Sweep og Loft kan bruge profilerne.

En Feature husker en valgt profil ud fra kurverne omkring den, ikke hvor du klikkede: `"profiles": [{"curves": ["l1", "l2", "a1", "l3"]}]` (eller `[{"curves": ["c1"]}]` for en cirkel) vælger området, hvis ydre kontur består af netop de angivne kurver eller deres dele efter skæring med andre kurver. Området findes igen ved hver genberegning, så funktionen kan følge parameterændringer. Øer inde i området bliver dets huller. Hvis ingen profil eller flere profiler afgrænses af samme kurver, vises en fejl frem for at vælge et andet område. Profiler valgt efter indeks eller punkt (`[{"point": [x, y]}]`) gemmes på samme måde, når funktionen oprettes.

## Measurement Units (måleenheder)

Skift det aktuelle designs enheder under **Document Settings > Units (dokumentindstillinger > enheder)** i designtræet. Standardindstillingen for enheder gælder nye design. Eksisterende skitsemål, længder under tegning og Measure følger det aktuelle designs enheder uden at ændre geometrien. Tommer vises med mindst tre decimaler og en betydende fjerde decimal; højere præcision kan vælges under Preferences. Længder skrevet uden enhed i målværktøjet bruger designets enheder. Eksplicitte enheder som `25.4 mm` er gyldige i et design med tommer. Vinkelmål bevarer deres vinkelenheder. Bevidst indtastede tommeværdier med fire decimaler bevarer efterstillede nuller. Afstandskontroller for komponenter og canvases samt målinger i Inspect, komponentbibliotek og udfoldet plade bruger også designets enheder. Parameterudtryk og navne på metriske standarder bevarer deres eksplicit angivne enheder.

Measure viser den valgte afstandstype (centrum, minimum eller maksimum) samt separate X-, Y- og Z-afstande i visningsområdet. Akseafstandene følger verdenskoordinaterne og forbliver synlige, indtil Measure-dialogen lukkes. Slå **Preferences > Graphics > Show X/Y/Z Distances (vis X/Y/Z-afstande)** fra for at skjule de ekstra linjer og etiketter. Indstillingen gemmes mellem opstarter.

### Gennemgang af Mirror, Pattern og Offset

Mirror, rektangulær eller cirkulær Pattern og Offset viser det aktive valgtrin i et fælles værktøjsvindue. Klik på et input for at ændre det. Klik igen på et valgt objekt, eller brug Ctrl/Cmd + klik, for at fjerne det. Hvert input har sin egen Clear-knap. Kildeinput viser antallet af valgte objekter; enkelte referencer viser den valgte spejlingslinje, retning eller centrum. Markering med ramme fungerer også, mens dialogen er åben. Skitseværktøjer til redigering bruger geometri fra den skitse, der aktuelt redigeres.

Foreslået skitsegeometri vises **grønt** før bekræftelse. Ændr input eller værdier for at opdatere Preview, og tryk derefter OK. Cancel kasserer forhåndsvisningen. Centrum for en cirkulær Pattern kan være et eksisterende punkt eller en placering valgt i et tomt område af skitsen.

Sketch Patterns (skitsemønstre) bevarer forbindelsen mellem hver gentagelse og kildegeometrien. Når kilden flyttes, følger kopierne med; en kopi kan ikke ændre form uafhængigt. Radius for cirkler og ellipser følger også kilden. Vælg en cirkels centrumpunkt for at centrere et cirkulært mønster på cirklen. Et centrum placeret på tom skitseflade bliver et skitsepunkt, som kan målsættes eller bindes sammenfaldende. Dobbeltklik på mønsterets badge (ved centrum for et cirkulært mønster eller ved kilden til et rektangulært mønster), eller højreklik på kilden eller en kopi og vælg **Edit Sketch Pattern (rediger skitsemønster)** for at ændre antal, afstand, retning eller samlet vinkel. Bekræftede ændringer bevarer ID'er på tilbageværende gentagelser. Cancel kasserer forhåndsvisningen. Indstillingerne overlever gemning, genåbning, Undo og Redo. Sletning af kilde, kopi eller mønsterrelation bryder forbindelsen og efterlader den øvrige geometri uafhængig. Mønstre fra ældre versioner er uafhængige kopier; opret dem igen for at få sammenkædningen.

Offset-afstande er redigerbare **Driving Dimensions (styrende mål)**. Dobbeltklik på målet, eller ændr modelparameteren for at tilpasse Offset uden at flytte kildekurverne. Valgt side, afstand og tilknytning bevares ved gemning, genåbning, Undo og Redo. Symmetric Offset deler én styrende afstand; Two Sides har en separat afstand for hver side. Alle valgte kurver eller kurvekæder forskydes, også hvis de ikke hænger sammen. Break Association bevarer den aktuelle geometri og fjerner den tilhørende styrende parameter. Ældre Offset-funktioner bevarer deres gemte faste afstande.

## Ordliste – engelske CAD-udtryk på dansk

De engelske betegnelser er bevaret, så du kan finde værktøjerne i SolidCraft. Kommandoer, parameternavne og JSON-felter må ikke oversættes i scripts.

| Engelsk udtryk | Dansk oversættelse |
|---|---|
| Sketch | Skitse |
| Create Sketch | Opret skitse |
| Finish Sketch | Afslut skitse |
| Sketch Palette | Skitsepalette |
| Geometry | Geometri |
| Point | Punkt |
| Line | Linje |
| Polyline | Polylinje |
| Rectangle | Rektangel |
| Centre | Centrum |
| Circle | Cirkel |
| Arc | Cirkelbue |
| Polygon | Polygon / mangekant |
| Slot | Aflangt hul |
| Ellipse | Ellipse |
| Fit Point Spline | Spline gennem tilpasningspunkter |
| Control Point Spline | Spline styret af kontrolpunkter |
| Conic | Keglesnitskurve |
| Construction | Konstruktionsgeometri |
| Construction Geometries | Konstruktionskurver |
| Centerline | Centerlinje |
| Projected Geometries | Projiceret geometri |
| Trim | Beskær |
| Extend | Forlæng |
| Break | Opdel / bryd forbindelse |
| Fillet | Afrunding |
| Chamfer | Affasning |
| Offset | Forskydning |
| Mirror | Spejling |
| Pattern | Gentagelsesmønster |
| Move/Copy | Flyt/kopiér |
| Scale | Skalér |
| Text | Tekst |
| Edit Text | Rediger tekst |
| Explode Text | Opløs tekst til geometri |
| Path | Sti |
| Baseline | Grundlinje |
| Project | Projicér |
| Intersect | Skæringsgeometri |
| Include | Medtag geometri |
| 3D Sketch | 3D-skitse |
| Fit Point | Tilpasningspunkt |
| Triad | Flytteakse / aksekryds |
| Constraints | Geometriske bindinger |
| Sketch Dimension | Skitsemål |
| Coincident | Sammenfaldende |
| Horizontal | Vandret |
| Vertical | Lodret |
| Parallel | Parallel |
| Perpendicular | Vinkelret |
| Tangent | Tangent |
| Equal | Ens |
| Concentric | Koncentrisk |
| Collinear | På samme linje |
| Midpoint | Midtpunkt |
| Symmetry | Symmetri |
| Fix | Fastlås |
| Curvature (G2) | Krumningskontinuitet (G2) |
| Solver | Beregningsmotor for bindinger |
| Degree of Freedom | Frihedsgrad |
| Under-constrained | Ikke fuldt bundet |
| Fully Constrained | Fuldt bundet |
| Over-constrained | Overbestemt |
| Driving Dimension | Styrende mål |
| Driven Dimension | Referencemål |
| Radius | Radius |
| Diameter | Diameter |
| Angle | Vinkel |
| Profiles | Profiler |
| Extrude | Ekstrudering |
| Revolve | Omdrejning |
| Sweep | Profil langs en sti |
| Loft | Overgang mellem profiler |
| Measurement Units | Måleenheder |
| Document Settings | Dokumentindstillinger |
| Measure | Mål |
| Preview | Forhåndsvisning |
| Clear | Ryd |
| Edit Sketch Pattern | Rediger skitsemønster |
| Break Offset Association | Bryd forbindelsen til Offset |
| Symmetric | Symmetrisk |
| Two Sides | To sider |
| Undo | Fortryd |
| Redo | Gentag |

### Kommandoer og felter

| Kommando eller felt | Dansk betydning |
|---|---|
| `sketch.create` | Opret skitse |
| `sketch.text` | Opret skitsetekst |
| `sketch.edit_text` | Rediger skitsetekst |
| `sketch.break_link` | Bryd en tilknytning |
| `sketch.offset.break` | Frigør Offset til selvstændig redigering |
| `sketch.spline.fit_point` | Opret spline gennem tilpasningspunkter |
| `sketch.line3d` | Opret 3D-linje |
| `sketch.spline3d` | Opret 3D-spline |
| `sketch.point3d` | Opret 3D-punkt |
| `sketch.move3d` | Flyt 3D-geometri |
| `sketch.dimension` | Opret skitsemål |
| `SOLIDCRAFT_FONT_DIRS` | Mapper, hvor SolidCraft søger efter skrifttyper |
| `origin` | Origo |
| `normal` | Normalvektor |
| `font` | Skrifttype |
| `height` | Højde |
| `angle` | Vinkel |
| `at` | Position |
| `created` | Oprettede elementer |
| `points` | Punkter |
| `curves` | Kurver |
| `constraints` | Bindinger |
| `dimensions` | Mål |
| `periodic` | Periodisk / lukket spline |
| `entities` | Geometriske elementer |
| `value` | Værdi |
| `profiles` | Profiler |

