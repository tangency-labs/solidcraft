# Components and Joints (komponenter og ledforbindelser)

Et design er et hierarki af **Components (komponenter)**. **New Component (ny komponent)** (`component.create`) tilføjer en komponent, og **Create Components from Bodies (opret komponenter fra emner)** (`component.from_bodies`) omdanner emner til komponenter. Hver komponent har sit eget origo, egne emner, skitser og funktioner på tidslinjen. Den **Active Component (aktive komponent)** vælges med radioknappen i Browser (designtræet) og modtager nye skitser og Features. De øvrige vises nedtonet.

En komponent placeres via sine **Occurrences (forekomster)**. Paste opretter endnu en instans af samme komponent, mens **Paste New** opretter en uafhængig kopi. Flyt en forekomst med flytteaksen (Move Triad), og vælg **Capture Position (gem position)** for at bevare den eller **Revert (gendan)** for at gå tilbage. SolidCraft spørger før næste kommando, hvis en flytning ikke er gemt. **Ground (fastlås)** låser en forekomst på plads.

Paste New kopierer underliggende komponentdefinitioner, skitser, Features og genererede målparametre. Gentagne underkomponenter forbliver fælles instanser inden for den nye kopi. Materialer, udseender og fuldt interne Joints og bevægelsesdata får kopierede referencer. Brugerparametre for hele designet forbliver fælles. Bevarede eksterne links og udeladte bevægelsesdata på tværs af komponentgrænsen rapporteres. Ikke-understøttede destruktive links får hele kopieringen afvist. Se [detaljer om instanser og kopiering](https://github.com/bherbruck/solidcraft/blob/main/docs/occurrence-workflows.md).

**Joints (ledforbindelser)** (`joint.create`, J) forbinder to komponenter med Joint Origins (ledkoordinatsystemer), som kan snappe til flader, kanter, cirkler og hjørnepunkter. Typerne er Rigid (fast), Revolute (drejeled), Slider (skydeled), Cylindrical (cylinderled), Pin-Slot (tap-i-spalte), Planar (planled) og Ball (kugleled), med Offset, Flip og Limits. **As-built Joint** bevarer den aktuelle placering, **Rigid Group** låser komponenter sammen, **Drive Joints** bevæger et led gennem dets værdier, og **Motion Link** kobler to led. Links kan kædes sammen, så ét led driver det næste. En værdi kan føres gennem op til 64 links uanset oprettelsesrækkefølge. Contact Sets stopper bevægelsen, hvor emner ellers ville passere gennem hinanden. **Motion Study** styrer ledværdier langs en tidslinje. **Interference** viser overlappende volumener med rødt, lister par inden for **Clearance (afstand)** og hvert emnes nærmeste nabo.

Et Joint Origin på en flade eller et hul bevarer den orientering, det fik ved valg. Hvis fladen hælder efter en bukning eller en parameterstyret rotation, hælder komponenten med fladen uden at rotere utilsigtet omkring dens normal.

Et Joint Origin kan også placeres på kanten af en plan flade (`"edge": [x, y, z]`): midt på kanten med retningen ud fra fladen og y-aksen ind i den. Med `"corner": [x, y, z]` placeres det i hjørnet nær punktet, med x- og y-akser langs de to kanter ind i fladen. Kant, flade og hjørne genfindes via deres navne. Et beslag placeret 40 mm fra hjørnet på et panel (et Planar Joint med værdierne `[40, 40, 0]`) forbliver derfor 40 mm fra hjørnet, selv om panelets størrelse ændres.

Et leds Offset, Angle og øvrige værdier kan være udtryk baseret på designets parametre (`"offset": "gap + 2"`, `"values": ["travel"]`). Udtrykkene gemmes og beregnes igen ved ændringer, så en komponent placeret `gap` over et panel følger parameteren. Hvis en parameter omdøbes, opdateres leddene også. En parameter, der bruges af et led, kan ikke slettes. Hvis udtrykket ikke længere kan beregnes, f.eks. fordi en parameter bliver en vinkel, udelades leddet fra løsningen, og `joint.solve` angiver årsagen i stedet for at placere komponenten ud fra forældede tal. Et direkte tal eller Drive erstatter udtrykket.

**Relations (relationer)** er Joints, der kun begrænser en del af komponentens placering. Du kan dermed placere en komponent gradvist: `parallel` (z-akser parallelle, men fri bevægelse og rotation omkring aksen), `perpendicular` og `angle` (z-akser vinkelrette eller med den angivne `angle` fra sammenfaldende), `point_on_plane` (punkt på en flades plan, eventuelt med `offset`), `point_on_line` (punkt på en akse) og `line_on_plane` (akse på et plan; en cylinders radius som `offset` giver tangens). Relationen oprettes ud fra komponenternes aktuelle placering; B flyttes kun så meget, relationen kræver. Flere relationer mellem samme komponenter virker sammen. Solveren reducerer de resterende frihedsgrader og rapporterer relationer, der ikke kan opfyldes, i stedet for at gennemtvinge dem:

```json
{"command": "joint.create", "params": {"type": "parallel", "a": "Panel top", "b": "Bracket bottom"}}
{"command": "joint.create", "params": {"type": "point_on_plane", "a": "Panel top", "b": "Bracket corner"}}
{"command": "joint.create", "params": {"type": "parallel", "a": "Panel front", "b": "Bracket face", "flip": true}}
{"command": "joint.create", "params": {"type": "point_on_plane", "a": "Panel front", "b": "Bracket corner", "offset": -40}}
{"command": "joint.create", "params": {"type": "point_on_plane", "a": "Panel end", "b": "Bracket corner", "offset": -40}}
```

Dette placerer beslaget plant på panelet, vinkelret på dets forkant og med hjørnet 40 mm fra både forkant og ende, uanset panelets størrelse. Sammenfaldende flader svarer til et `planar` Joint; afstand mellem flader er `planar` med `offset`, koncentriske flader er `cylindrical`, og to mødesammenfaldende punkter er `ball`.

Kontrollen `inspect.clearance` kan undersøge et vilkårligt antal emner i ét kald og er velegnet til scripts og AI-agenter, der kontrollerer en hel samling:

```json
{"command": "inspect.clearance", "params": {"clearance": 1, "nearest": 2, "skip_within": "component"}}
```

Resultatet indeholder `overlaps` (emnepar med fælles volumen på mindst `min_volume`, som standard 0,01 mm³, samt fælles volumen, areal, bounding box og centrum), `near` (emnepar tættere end `clearance`, inklusive afstand og nærmeste to punkter; kontakt under `min_volume` vises her som 0 med `overlap_mm3`), `nearest` (hvert emnes nærmeste naboer) og `unchecked` (par, hvor overlap ikke kunne beregnes, med årsag). `against` begrænser kontrollen til par med ét emne i `bodies` og ét i `against`. `skip_within` udelader par inden for samme forekomst (`"occurrence"`) eller samme placerede komponent inklusive underkomponenter (`"component"`), f.eks. dele i en købt komponent, der med vilje berører hinanden. Afstande beregnes på præcise flader og kanter. Mesh bruges kun til at udelukke par, der er for langt fra hinanden; resten kontrolleres parallelt. `inspect.interference` undersøger stadig kun overlap: Emnepar, hvis mesh er adskilt, udelades hurtigt, resten skæres parallelt, og beregningen afsluttes senest efter `budget_s` sekunder (standard 240). Par, der ikke nåede at blive kontrolleret, angives.

Mesh-emner importeret fra STL, 3MF eller OBJ kontrolleres ikke af nogen af de to kommandoer. Hvert af dem angives i `unchecked` som `{a: name, b: null, reason: "mesh body: not checked"}`. Derfor må `count: 0` eller en tom `overlaps`-liste ikke fortolkes som, at der ikke er overlap, hvis designet indeholder et mesh-emne.

**Gear (tandhjul)**, **Rack and Pinion (tandstang og tandhjul)** og **Screw (skrueføring)** under **ASSEMBLE** er Motion Links, hvor forholdet bestemmes af komponenternes geometri frem for et manuelt tal:

- **Gear** (`joint.gear {a, b, ra, rb}`) kobler to roterende led. Led 2 drejer `ra/rb` gange så langt som led 1, men i modsat retning. Med **Internal Gear (indvendigt tandhjul)** roterer de samme vej. Vælg en rund flade på hvert tandhjul, f.eks. delingscylinderen, eller indtast radierne. Tandantal kan også bruges, fordi kun forholdet er vigtigt. Valg af tandhjulets flade vælger også det led, hjulet roterer omkring.
- **Rack and Pinion** (`joint.rack_pinion {a, b, radius}`) kobler et roterende led til et skydeled: tandstangen flyttes med tandhjulets delingsradius gange rotationsvinklen i radianer (r·θ). Begge led kan drive det andet. I `joint.list` angives forholdet pr. grad ligesom for andre links mellem vinkel og afstand.
- **Screw** (`joint.screw {joint, pitch}` eller `{a, b, pitch}` for et drejeled og et skydeled) flytter `pitch` pr. omdrejning. Et Cylindrical Joint fungerer også alene som en skrueføring: rotationen styrer forskydningen. Gevindretningen er højre som standard; **Reverse (vend retning)** gør den venstre.

Radius eller gevindstigning kan være et udtryk baseret på designets parametre, og en valgt flade genfindes, når designet ændres. Ændres parameteren eller tandhjulets diameter, opdateres udvekslingen. Et nyt Motion Link bevarer det drevne leds aktuelle placering, så modellerede tandhjul bliver i indgreb. **Reverse** vender rotationen for led med modsatrettede akser. Også når et link omdefineres (`link: i`, f.eks. ved Reverse), bevares det drevne leds aktuelle position. Koblingerne virker i lukkede mekanismer, med Contact Sets og i Motion Studies. En tandstang kan f.eks. drive stemplet i en krumtapmekanisme eller styre hjul gennem styrestænger. Det er det drivende led, du skal aktivere (led 1, tandhjulet eller skruens rotation), ikke det drevne. Et led kan højst følge ét link, og en kobling, der ville drive sin egen driver, afvises. `joint.list` viser hver kobling under `links` og angiver en `error`, hvis den ikke kan beregnes, f.eks. fordi en valgt flade er forsvundet. I så fald beholder linket sit seneste udvekslingsforhold.

**Cam (knast)** (`joint.cam {a, b, profile}` under **ASSEMBLE**) får en følger til at bevæge sig langs en knastprofil. Knasten roterer om et Revolute Joint (a). Følgeren kan være et Slider Joint (b), eksempelvis en ventiltrykker, eller et Revolute Joint (b), eksempelvis en vippearm. Profilen er knastens kontur i et plan vinkelret på dens akse: en lukket skitseprofil i knastens komponent eller kanter valgt på emnet. Ét klik på en kant vælger hele den lukkede kontur i planet. Følgeren kan være:

- **Knife Edge (knivkant):** Et punkt på knasten.
- **Roller (rulle):** Med en indtastet radius eller radius fra rullens runde flade. Når fladen vælges, bestemmes også rullens placering.
- **Flat Face (plan flade):** Vinkelret på skydeleddets bevægelseslinje, eksempelvis en cylindrisk ventiltrykker.

En lineært bevægelig følger bevæger sig langs skydeleddets akse gennem det valgte **Tip (kontaktpunkt)**, dvs. spidsen på en knivkant eller rullens centrum. En forskudt følger følger derfor den korrekte side af knasten. En roterende følger kræver et kontaktpunkt eller en rulleflade for at bestemme armen. Følgeren bliver på den side af knasten, hvor den allerede befinder sig; **Other Side (anden side)** vælger den modsatte. Følgerens bevægelse beregnes ud fra profilen og gemmes med linket: én værdi pr. grad med en glat kurve mellem værdierne, som ikke overskrider samplepunkterne. Bevægelsen beregnes igen ved ændringer i design, skitse, parametre eller rulle. Som ved tandhjul bevares følgerens aktuelle position ved oprettelse af linket, så den bør modelleres i kontakt med knasten. Cam er et Motion Link, som fungerer med Drive Joints, Motion Studies, lukkede mekanismer og Contact Sets. Profilen kan også ligge på en fastlåst komponent med følgerens føring på en roterende holder. `joint.list` viser for en Cam værdien `lift`, dvs. følgerens laveste og højeste placering samt løftehøjde, i stedet for et forhold. Åbne profiler, linjer eller arme, som mister kontakten, følgere der ville springe, og akser, der ikke er vinkelrette eller parallelle som krævet, afvises.

Et Joint forbinder to forskellige komponenter. Hvis begge valgte punkter ligger på emner i samme komponent, kan der ikke oprettes et led. Det sker ofte med STEP-filer, hvor flere dele importeres som emner i én komponent. Dialogerne Joint og As-built Joint viser problemet ved markeringen og tilbyder **Create Components from Bodies**. Hvert valgt emne får sin egen komponent og beholder sin placering. Markeringerne bevares, og et preview af leddet vises. `joint.create` afviser tilsvarende input med samme vejledning.

Joints kan danne lukkede mekanismer, f.eks. en fireledsmekanisme eller et krumtap-skydeled. Når et led i kredsløbet drives, følger de øvrige, så alle forbindelser fortsat opfyldes. Mekanismen forbliver i sin oprindelige opstilling; en åben fireledsmekanisme skifter ikke pludselig til en krydset. Hvis bevægelsen kommer ud over en position, hvor kredsløbet kan lukkes, f.eks. når en vippearm når enden af sit udsving, stopper den ved sidste gyldige position og rapporterer `stopped_by_loop`. Et kredsløb, der slet ikke kan lukkes, vises under `conflicts`, og intet i kredsløbet flyttes. `joint.list` viser de tilbageværende frihedsgrader, når lukkede kredsløb tages med: bevægelige dele i en fireledsmekanisme har én fælles frihedsgrad, mens et kredsløb låst med Rigid Joint har nul. Et led drevet af et Motion Link giver ingen ekstra frihedsgrad; to tandhjul i indgreb på et fast hus har samlet én. Drive Joints-dialogen forklarer, hvis bevægelsen stopper før den indtastede værdi. Under preview vises den opnåede værdi og årsagen, f.eks. et kredsløb der ikke kan lukkes eller dele stoppet af Contact Set. Efter OK gentages informationen i statuslinjen. Værdifeltet beholder den indtastede værdi. Dialogen starter med det led, der er valgt i Browser, eller det første bevægelige led, hvis intet er valgt.

**Drag a Part (træk en komponent)** flytter komponenten så langt, dens Joints tillader. Punktet, du tager fat i, følger markøren i et plan gennem punktet vendt mod kameraet. Leddene i kæden indstilles til den nærmeste mulige placering inden for deres grænser og gennem Motion Links, mens lukkede mekanismer forbliver lukkede. En fireledsmekanismes vippearm stopper ved enden af sit udsving. Hele trækket er ét Undo-trin. En Grounded (fastlåst) komponent eller en komponent uden placerende Joints flyttes ikke; et træk opretter i stedet en markeringsramme. Hver frame kører `joint.reach`, som scripts og agenter også kan bruge direkte: `joint.reach {occurrence, point}` bevæger leddene fra deres aktuelle position til nærmeste mulige stilling. En arm beholder eksempelvis albuen oppe eller nede, og en mekanisme forbliver på samme bevægelsesgren. `at` vælger et punkt på komponenten (som standard emnernes centrum). `z` med `axis` kan også orientere en af dens akser, og `joints` angiver, hvilke led der må drives (andre led i samme lukkede kredsløb følger med). Svaret angiver, om punktet blev nået (`reached`), afvigelsen (`error_mm`, `error_deg`) og eventuelle begrænsninger (`stopped_by_limit`) eller lukkede kredsløb (`stopped_by_loop`), der stoppede bevægelsen. `joint.reach` kontrollerer ikke Contact Sets; brug Drive Joints til det.

Solveren placerer komponenter, den kan nå fra roden eller fra Grounded-komponenter. En kæde af Joints, der ikke er forbundet til noget fastlåst, placeres ikke, så Drive ikke flytter noget. `joint.drive` returnerer `moved: false`, en `reason` og de `unconnected` komponenter, f.eks. `Pedestal:1 is not connected to anything grounded`. Drive Joints-dialogen viser samme information, `motion.play` angiver leddene under `unmoved`, og `joint.list` viser kæderne under `floating`. Fastlås en komponent i kæden, eller forbind den med en fastlåst komponent. Et Rigid As-built Joint bevarer dens aktuelle placering.

`joint.drive` returnerer leddets værdier som `joint.list`, dvs. grader og millimeter med enheder og grænser. `moved` fortæller, om en komponent blev flyttet. Som standard vender B's z-akse mod A's, når Joint Origins sammenkobles, som to flader der mødes. `flip: true` retter i stedet akserne samme vej, så to origins i samme punkt med ens z-retning forbliver orienteret som før. Uden `angle` vælger et nyt Joint (bortset fra Relations, som starter i komponenternes aktuelle position) enten ingen rotation eller en halv omdrejning, alt efter hvad der flytter B mindst. Derfor roterer en plade under et panel ikke utilsigtet omkring deres fælles normal. I scripts med mange trin kan `quiet: true` udelade frihedsgrader pr. komponent fra svarene. `motion.play` med `quiet: true` giver kun positioner for komponenter, der faktisk flyttede sig.

**Motion Trace (bevægelsesspor)** (`motion.trace`, under **ASSEMBLE**) tegner den bane, et punkt på en komponent følger, når et Joint drives eller et Motion Study afspilles, f.eks. koblingskurven for en fireledsmekanisme. Vælg et punkt på komponenten (fladecentrum, cirkelcentrum, kantmidtpunkt eller hjørnepunkt) og derefter enten et Joint med start- og slutværdier eller et Motion Study. Banen forhåndsvises i Viewport, som mekanismen faktisk bevæger sig, med hensyntagen til lukkede kredsløb. Den slutter, hvis kredsløbet ikke længere kan lukkes, eller hvis et Contact Set stopper bevægelsen; årsagen vises i dialogen. OK gemmer banen som en 3D-spline i en ny skitse under rodkomponenten (`Motion Trace1` osv.). Den påvirkes ikke af Joints og kan bruges som sti for Sweep eller Pipe. Banen er fast og følger ikke senere ændringer i mekanismen. Hvis den ender, hvor den begyndte, gemmes den som en glat, lukket spline. `motion.trace {occurrence, point, joint, from, to, index?, samples?}` eller `{occurrence, point, study, samples?}` returnerer punkter i verdenskoordinater uden at ændre designet; `motion.trace.keep` med de samme parametre og eventuelt `name?` gemmer dem. Der bruges 2–500 samples: som standard 100 for et Joint og ét pr. trin for et studie.

**Export Animation… (eksportér animation)** i dialogen **Motion Study** gemmer studiet som en animeret GIF fra Viewports kamera. Trinnene i dialogen medregnes også før OK. `motion.export_animation` gør det samme fra scripts, CLI eller agenter uden et vindue: angiv et `study` eller et `joint` med `from` og `to` (og eventuelt `index?`) samt en `path`. En sti med `.gif` gemmer en GIF, der gentages; andre stier opfattes som mapper, hvor billeder gemmes som `frame-0001.png`, `frame-0002.png` osv. Mappen oprettes om nødvendigt, og overflødige frames fra en tidligere, længere eksport fjernes. Billederne kan bruges i et videoværktøj. Webappen kan kun skrive GIF. `samples` er antal frames (som standard ét pr. trin i et studie eller 100 for et Joint), `width` og `height` er størrelse i pixels (standard 800 × 500, højst 4096 på hver led), og `fps` er billedhastighed (standard 20). `grid: false` og `axes: false` skjuler gitter og akser. `camera` accepterer svaret fra kontrolkanalens `ui.camera` eller dele af det. Manglende værdier kommer fra visningen valgt med `view.apply` eller Home-visningen. Frames renderes som ved `solidcraft-cli snapshot` på en kopi, så designet ikke ændres. En GIF bruger én farvepalette gennem alle frames for at undgå flimren og gemmer kun ændringer mellem dem. Maksimum er 1000 frames og samlet højst `2^26` billedpixels (cirka 160 frames ved 800 × 500).

**Motion Analysis (bevægelsesanalyse).** Feltet **Study** i Motion Study-dialogen åbner et gemt studie til redigering. Et diagram ved tidslinjen viser studiets bevægelse. **Plot** vælger en bevægelig Joint-værdi (et led valgt i Browser vælges også her) eller **Picked Point (valgt punkt)**, når et punkt er valgt under **Point**. Under **Quantity (størrelse)** vælger du værdi, hastighed eller acceleration. Diagrammet viser studiets trin med en markør ved det aktuelle trin. Klik eller træk i diagrammet for at skifte trin. Værdier vises i grader og millimeter, mens hastighed og acceleration angives pr. trin eller pr. sekund, når **Duration** angiver studiets varighed i sekunder. For et valgt punkt vises afstanden fra start, hastighed og accelerationens størrelse. **Check Collisions (kontrollér kollisioner)** gennemspiller studiet og finder komponentpar, der krydser hinanden, samt de trin hvor det sker; klik på et fund for at gå til første trin. Trinene markeres i diagrammet. Kontrollen bruger de valgte **Components**, eventuelt i forhold til **Against**. Ellers kontrolleres alle komponenter eller parrene fra designets Contact Sets, hvis de findes.

`motion.plot {study, joints?, occurrence?, point?, samples?, duration?}` returnerer de samme målinger: `value`, `velocity` og `acceleration` pr. sample for hvert Joint samt `position`, `velocity` og `acceleration` som vektorer, `speed` og `acceleration_norm` for et punkt. Studiet afspilles som med `motion.play` (lukkede kredsløb følger, Contact Sets stopper bevægelsen), trin for trin på en kopi, så designet ikke ændres. Hastighed og acceleration beregnes med endelige differenser mellem samples: centralt mellem punkter og ensidigt i enderne. Resultatet bliver glattere ved tættere sampling (højst 1000 samples, som standard ét pr. trin). Vinkler fortsætter med at tælle uden nulstilling ved en halv omdrejning.

`motion.interference {study, occurrences?, against?, contact_sets?, samples?}` finder hvert kolliderende komponentpar med `first_step`, `last_step`, `steps` (intervaller med overlap), `depth_mm` (største indtrængning ved `deepest_step`) og fælles `volume_mm3` (null, hvis det er for beregningstungt). Studiet afspilles som ved `motion.play`, men Contact Sets stopper ikke bevægelsen. Kun samplede positioner undersøges; en kollision mellem samples kan derfor overses. Komponenter identificeres ud fra deres forekomst: en underkomponent i en samling, der er indsat to gange, tæller som to forskellige forekomster, og sammenlignes ikke med sig selv. Højst 256 emner kan kontrolleres ad gangen. I store design kan du derfor vælge `occurrences` og eventuelt `against`. Hvis beregningen ikke kan fuldføres, returneres `truncated` samt det seneste kontrollerede trin `checked_to`.

`joint.sweep {joint, from, to, index?, step?, refine?, occurrences?, against?, contact_sets?, report_gaps?, apply?}` bevæger en Joint-værdi fra `from` mod `to` og viser, hvor langt den kan bevæges før kontakt. `stopped_at` er sidste værdi uden kollision; `stopped_by` angiver de to kontaktende komponenter og kontaktpunktet `at` eller `"loop"` ved et kredsløb, der ikke kan lukkes, eller `"limit"` ved en grænse. Med `report_gaps` vises nærmeste afstand ved hvert sample og den mindste afstand for hvert emne (`min_gap`). Sampleafstanden er lille nok til, at ingen dele passerer hinanden mellem samples, eller den angivne `step`. Første kontakt findes ved intervalhalvering, indtil ingen del bevæges mere end `refine` (standard 0,01 mm) over intervallet. Afstande beregnes på mesh (`gap_method: "mesh"`): præcist for plane flader og inden for `gap_tolerance_mm` for buede flader. Par, der allerede berører hinanden ved `from`, f.eks. to sammenføjede flader eller en dør mod sin karm, vises under `touching_at_start` og stopper først beregningen, hvis de trænger længere ind i hinanden. Sweep køres på en kopi (lukkede kredsløb følger, mens Contact Sets ikke stopper den). Designet ændres kun, hvis `apply` er true.

**Insert Design (indsæt design)** (`component.insert_design {path, translate?}` under **ASSEMBLE** og i **File**-menuen) indsætter en komponent, der er linket til et andet gemt design, så hver del kan ligge i sin egen fil. STEP- og meshfiler indsættes som med Insert STEP eller Insert Mesh. Solid Bodies indsættes med deres placering fra kildedesignet og gemmes også i samlingen, så samlingen kan åbnes uden kildefilen. Linket gemmes relativt til samlingens mappe, når samlingen gemmes. **Update Links (opdater links)** (`component.update_links`) læser kildefilen igen efter ændringer. **Break Link (bryd link)** (`component.break_link`) bevarer emnerne, men fjerner filreferencen. `component.links` viser links og fortæller, om deres filer findes. En linket komponent indeholder kun kildedesignets emner; dens Timeline, parametre og Sheet Metal-data (Flat Pattern og Bend Table) bliver i kildedesignet.

Hvis en kildefil flyttes eller omdøbes, skal du højreklikke på den linkede komponent eller forekomst i Browser og vælge **Relink Source (All Instances)… (vælg ny kildefil for alle instanser)**. Vælg det gemte `.solidcraft`-design. Genlinkning opdaterer stien og emnerne for **alle instanser**, men bevarer deres identitet og placering. Cancel eller valg af en manglende, ugyldig, tom eller selvrefererende fil efterlader samlingen uændret. Handlingen er ét Undo-trin. Den nye sti gemmes relativt til samlingen, når det er muligt.

Scripts bruger `component.relink {component, path}`. Svaret indeholder `previous_path`, `path`, `warnings`, `joint_errors` og `joint_conflicts`. Ændret geometri kan ugyldiggøre en fladereference i et Joint. Genlinkning rapporterer sådanne Joints i svaret og i status-/meddelelseshistorikken. Ret de berørte led eller fortryd genlinkningen. Relink hverken reparerer eller verificerer den oprindelige hensigt med eksisterende Joints. Når linket geometri med Joints ændres, vises en påmindelse om at kontrollere leddene, også hvis solveren ikke melder fejl: en vellykket beregning betyder ikke nødvendigvis, at forbindelsen stadig svarer til den ønskede konstruktion.

STEP-eksport gemmer komponenthierarkiet som en Assembly (samling).

Højreklik på en komponent i Browser, og vælg **Properties (egenskaber)** for at se masse med hensyn til materiale, volumen, overfladeareal, tyngdepunkt og ydermål i verdenskoordinater. Underkomponenter og gentagne instanser tælles med i samlingens totaler, også endnu ikke gemte flytninger. En komponent bruger sin første forekomst; `occurrence.properties` vælger en bestemt instans. Properties på rodkomponenten måler hele samlingen. Volumener summeres, også når emner overlapper, i stedet for at måle deres fælles union. Samme funktioner findes via `component.properties` og `occurrence.properties`; se [detaljer om placering og måling](https://github.com/bherbruck/solidcraft/blob/main/docs/assembly-properties.md).

## Part Properties and Bill of Materials (deloplysninger og stykliste)

Hver komponentdefinition har sine egne **Part Properties (deloplysninger)**: varenummer, beskrivelse, revision, data fra andre værktøjer under en reverse-DNS-nøgle (efter samme regler som designets egne egenskaber) og frie tekstfelter som materialekvalitet eller overfladebehandling. Alle forekomster af komponenten deler oplysningerne. Paste New kopierer dem, og de gemmes med komponenten. `component.metadata {component}` læser dem, og `component.metadata.set {component, part_number?, description?, revision?, values?, properties?}` ændrer dem i ét Undo-trin (en tom værdi fjerner feltet). Rodkomponenten er selve designet og bruger `document.metadata`. Standarddele (`parts.insert`) får varenummer ud fra standard og størrelse (f.eks. `ISO 4762 M6x20` eller en lejebetegnelse) samt en beskrivelse. **Change Part Size (ændr delstørrelse)** opdaterer nummeret, men bevarer et felt, du selv har ændret.

**Bill of Materials (stykliste)** (`assembly.bom` under **INSPECT**) tæller placerede komponenter:

- `levels: "flat"` (standard) viser dele på alle niveauer, også gennem under-samlinger. En del, der ligger én gang i en under-samling, som er placeret to gange, tæller derfor 2. En under-samling opdeles i sine dele, medmindre den selv har emner.
- `levels: "top"` viser direkte underkomponenter og tæller en under-samling som én række.
- `group_by: ["part_number"]` (standard) samler definitioner med samme varenummer på én række. Definitioner uden varenummer får hver deres række. `["definition"]` giver en række pr. definition.
- `group_children: true` medtager dele placeret inde i en del med varenummer på samme række. En bolt med skive og møtrik tæller da som én række, mens skive og møtrik står under `children` (punkt 1.1, 1.2 osv., med `each` pr. bolt og samlet `count`) i stedet for særskilte rækker. Bolte med forskelligt indhold, f.eks. med og uden møtrik, placeres på forskellige rækker. `total_occurrences` tæller også underdelene, så totalen bliver den samme.
- `component` viser delene fra én bestemt under-samling i stedet for hele designet.

Rækkerne sorteres efter varenummer (rækker uden varenummer sidst) og indeholder `item`, `part_number`, `description`, `revision`, `count`, `definition` (komponentnavne), `components` (deres ID'er) og `properties` samt totalen `total_occurrences`. Med `path` skrives resultatet til `.csv` (kolonnerne item, part_number, description, revision, count og definition med alle værdier i anførselstegn; underdele følger hovedrækken) eller `.json`. Programvinduet viser samme rækker, lader dig skifte niveau og gruppering, viser underdele under deres hoveddel, redigere varenummer, beskrivelse og revision samt eksportere. Ændring af deloplysninger påvirker ikke geometrien, så modellen genberegnes ikke, og Joints placeres ikke igen.

## Ordliste – engelske udtryk på dansk

De engelske navne svarer til betegnelser i SolidCraft. Kommandoer og tekniske feltnavne skal altid forblive på engelsk i scripts.

| Engelsk udtryk | Dansk oversættelse |
|---|---|
| Components | Komponenter |
| Component | Komponent |
| Assembly | Samling / sammensat konstruktion |
| Subassembly | Under-samling |
| Body | Emne |
| New Component | Ny komponent |
| Create Components from Bodies | Opret komponenter fra emner |
| Active Component | Aktiv komponent |
| Browser | Designtræ |
| Origin | Origo |
| Occurrence | Forekomst / instans |
| Paste | Indsæt endnu en instans |
| Paste New | Indsæt som uafhængig kopi |
| Move Triad | Flytteakse |
| Capture Position | Gem position |
| Revert | Gendan |
| Ground / Grounded | Fastlås / fastlåst |
| Joint | Ledforbindelse |
| Joint Origin | Ledkoordinatsystem |
| Rigid | Fast forbindelse |
| Revolute | Drejeled |
| Slider | Skydeled |
| Cylindrical | Cylinderled |
| Pin-Slot | Tap-i-spalte |
| Planar | Planled |
| Ball | Kugleled |
| As-built Joint | Ledforbindelse i eksisterende placering |
| Rigid Group | Fast gruppe |
| Drive Joints | Bevæg led |
| Motion Link | Bevægelseskobling |
| Motion Study | Bevægelsesstudie |
| Contact Set | Kontaktgruppe |
| Interference | Overlap / kollisionskontrol |
| Clearance | Frigang / afstand |
| Relations | Relationer |
| Parallel | Parallel |
| Perpendicular | Vinkelret |
| Point on Plane | Punkt på plan |
| Point on Line | Punkt på linje |
| Line on Plane | Linje på plan |
| Gear | Tandhjul |
| Internal Gear | Indvendigt tandhjul |
| Rack and Pinion | Tandstang og tandhjul |
| Screw | Skrueføring |
| Pitch | Gevindstigning / stigning |
| Reverse | Vend retning |
| Cam | Knast |
| Follower | Følger |
| Knife Edge | Knivkant |
| Roller | Rulle |
| Flat Face | Plan flade |
| Tip | Kontaktpunkt |
| Other Side | Anden side |
| Solver | Beregningsmotor |
| Degree of Freedom | Frihedsgrad |
| Closed Loop | Lukket mekanisme |
| Four-bar Linkage | Fireledsmekanisme |
| Slider-Crank | Krumtap-skydeled |
| Drag a Part | Træk en komponent |
| Motion Trace | Bevægelsesspor |
| Export Animation | Eksportér animation |
| Motion Analysis | Bevægelsesanalyse |
| Plot | Diagram |
| Picked Point | Valgt punkt |
| Quantity | Størrelse |
| Velocity | Hastighed |
| Acceleration | Acceleration |
| Duration | Varighed |
| Check Collisions | Kontrollér kollisioner |
| Insert Design | Indsæt design |
| Insert STEP | Indsæt STEP |
| Insert Mesh | Indsæt mesh |
| Update Links | Opdater links |
| Break Link | Bryd link |
| Relink Source | Vælg ny kildefil |
| Properties | Egenskaber |
| Part Properties | Deloplysninger |
| Part Number | Varenummer |
| Description | Beskrivelse |
| Revision | Revision |
| Bill of Materials (BOM) | Stykliste |
| Change Part Size | Ændr delstørrelse |
| Flat Pattern | Udfoldet plade |
| Bend Table | Bukketabel |
| Surface Area | Overfladeareal |
| Centre of Mass | Tyngdepunkt |

### Kommandoer og felter

| Teknisk navn | Dansk betydning |
|---|---|
| `component.create` | Opret komponent |
| `component.from_bodies` | Opret komponenter fra emner |
| `component.insert_design` | Indsæt et linket design |
| `component.update_links` | Opdater kildefiler |
| `component.break_link` | Bryd link til kildefil |
| `component.links` | Vis kildefillinks |
| `component.relink` | Vælg ny kildefil |
| `component.properties` | Komponentegenskaber |
| `occurrence.properties` | Egenskaber for instans |
| `component.metadata` | Læs deloplysninger |
| `component.metadata.set` | Opdater deloplysninger |
| `document.metadata` | Designets oplysninger |
| `parts.insert` | Indsæt standarddel |
| `joint.create` | Opret ledforbindelse |
| `joint.solve` | Beregn leddenes placering |
| `joint.list` | Vis led og forbindelser |
| `joint.drive` | Bevæg et led |
| `joint.reach` | Bevæg til ønsket punkt |
| `joint.sweep` | Undersøg bevægelse frem til kontakt |
| `joint.gear` | Kobling mellem tandhjul |
| `joint.rack_pinion` | Tandstang og tandhjul |
| `joint.screw` | Skruekobling |
| `joint.cam` | Knastkobling |
| `motion.play` | Afspil bevægelse |
| `motion.trace` | Beregn bevægelsesbane |
| `motion.trace.keep` | Gem bevægelsesbane |
| `motion.export_animation` | Eksportér animation |
| `motion.plot` | Beregn bevægelsesdiagram |
| `motion.interference` | Kontrollér kollisioner i bevægelse |
| `inspect.clearance` | Undersøg frigang |
| `inspect.interference` | Undersøg overlap |
| `assembly.bom` | Opret stykliste |
| `occurrence` | Instans / forekomst |
| `offset` | Forskydning |
| `angle` | Vinkel |
| `flip` | Vend orientering |
| `limits` | Bevægelsesgrænser |
| `radius` | Radius |
| `pitch` | Stigning |
| `lift` | Løftehøjde |
| `clearance` | Frigang |
| `overlaps` | Overlap |
| `nearest` | Nærmeste naboer |
| `unchecked` | Ikke kontrolleret |
| `error_mm` | Positionsafvigelse i mm |
| `error_deg` | Vinkelafvigelse i grader |
| `stopped_by_loop` | Stoppet af lukket mekanisme |
| `stopped_by_limit` | Stoppet af bevægelsesgrænse |
| `samples` | Antal beregningspunkter |
| `fps` | Billeder pr. sekund |
| `width` | Bredde |
| `height` | Højde |
| `part_number` | Varenummer |
| `description` | Beskrivelse |
| `revision` | Revision |
| `count` | Antal |
| `group_by` | Grupper efter |
| `group_children` | Grupper underdele |
| `total_occurrences` | Samlet antal forekomster |
| `levels` | Niveauer |
| `path` | Filsti |

