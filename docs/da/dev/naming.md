# Persistent Naming (vedvarende navngivning)

En Fillet gemmer de kanter, den afrunder, en skitse den flade, den ligger på, og en Shell de flader, den åbner. Hvis referencerne kun var **flade- og kantindekser**, ville en tidligere ændring, der tilføjer en flade, få Fillet til at flytte til en anden kant. SolidCraft gemmer derfor **navne** og finder dem igen, hver gang Timeline genberegnes.

## Hvorfor navne følger kernelens historik

Tidligere blev navnene udledt af geometrien: Prøvepunkter på nye flader blev slået op på Feature-inputfladerne, og den flade med flest træffere vandt. Stykker af en opdelt flade blev nummereret efter deres centres placering. Det virker normalt, men fejler netop, når det er vigtigst. Når en tidligere Feature opdeler, sammenføjer, spejler eller formindsker en flade, kan punkter havne på nabofladen, og centre kan bytte rækkefølge. Så kan en Fillet eller skitse lydløst knytte sig til den forkerte flade. Uden en advarsel er det en særlig alvorlig CAD-fejl.

Kernen rapporterer nu for hver operation, hvor hver resulterende flade og kant stammer fra (`crates/kernel/src/history.rs`: fladens oprindelse og hvordan den ændredes samt kantens oprindelige kant eller de flader, hvis møde dannede den). Navnene følger denne historik og bliver dermed præcise og deterministiske. Kan en reference ikke længere findes entydigt, vises en fejl frem for at gætte.

## Sådan navngives et emnes flader

`crates/doc/src/lineage.rs` sammensætter historikken fra kernel-operationer inden for én Feature (et skitsebaseret værktøj, udskæringer i mål-emner og opdeling af resterende emner) til én **Lineage (afstamning)** for navngivbare operander:

| Operand | Fladenavne |
|---|---|
| Et emne før Feature | De tidligere navne |
| En flyttet kopi (Pattern, Mirror) | `<name>@F<feature>.<instance>` |
| Indersiden af en Shell, som vokser udad | `F<feature>:inner:<name>` |
| Et emne oprettet af funktionen | Navn ud fra rolle: `F<feature>:side:<sketch curve>`, endeflader `start`/`top`/`bottom`/`end`, boksflader `+x` osv., importerede flader `face:<k>` |
| En kopieret Feature-tool i Pattern | `F<feature>:<role>@F<pattern>.<instance>` |

`crates/doc/src/history_names.rs` navngiver resultatets flader ud fra deres ophav. En bevaret eller trimmet flade beholder sit oprindelige navn, mens en Fillet- eller Chamfer-flade får `F<feature>:blend:<edge>` efter den kant, den erstatter. En indvendig Shell-væg får `F<feature>:inner:<face>`. En sides skitsekurve er den kurve, som profilsegmentet ligger præcist på; der samples ikke.

- **Stykker:** Flader med samme navn er stykker af én oprindelig flade. De skelnes gennem naboflader, som adskiller dem fra hinanden: `F2:top#[F2:side:l4,F4:side:l4]` er stykket af boksens top afgrænset af dens venstre væg og slidsens venstre væg. Det overlever, at slidsen flyttes, vokser eller krymper. Kan naboflader ikke adskille stykkerne, nummereres de (`#1`, `#2`) efter deres oprindelse, hvis den er forskellig, ellers efter kernelens rækkefølge. Navnet markeres da som ikke verificeret.
- **Sammenlagte flader:** En flade, som opstår ved samling, bruger navnet fra den fælles oprindelige flade, hvis alle dele stammer fra den, ellers mål-emnets navn. Den kan også genfindes under de andre delnavne (aliases).
- **Kanter:** Navngives efter de flader, de adskiller, `<face A>|<face B>`. Hvis flere kanter findes mellem samme flader, bruges `~k` og placeringen til at skelne. Navne, som kun kan kontrolleres geometrisk, rapporteres.
- **Adskilte emnestykker:** Når Cut deler et emne, navngives hele objektet først, så fladestykker på forskellige emner kan skelnes. Emnenavne som `Bar` og `Bar (1)` tildeles efter det mindste fladenavn frem for kernelens shell-rækkefølge, som tidligere ændringer kan påvirke. Se [Ældre design](#ældre-design).

Navnene beregnes først ved behov og deles mellem kloner af samme emne.

### Hvor der ikke findes en historik

Operationer, der endnu ikke rapporterer historik (visse kernel-forløb samt Sheet Metal-, Plastic- og Surface-funktioner, Emboss, Rib, Replace Face og facetteret Thread Cut), eller flader uden en entydig rolle (Loft-sider, genererede Coil-/Pipe-profiler og Hole-værktøjer), bruger den tidligere metode baseret på geometri. Navnene markeres som **ikke verificerede** (`model.faces` returnerer `exact: false`). Hvis en reference findes gennem et ikke-verificeret navn, men det gemte punkt ikke længere ligger på fladen, advares brugeren: `face ... was found by geometry, not exact history; check it`.

## Sådan finder en reference sin flade igen

`crates/doc/src/name_lookup.rs` følger denne rækkefølge:

1. Fladen med det præcise navn.
2. Én flade, som også genkender navnet, fordi den er lagt sammen med en anden eller er det navngivne stykke stadig afgrænset af de flader, der står i navnet.
3. Hvis den oprindelige flade er opdelt eller findes i flere emner, vælges stykket nærmest referencens punkt, og Feature fortæller, at den blev opdelt.
4. Hvis intet findes, bruger Feature den gemte geometri som fallback og meddeler, at fladen ikke længere eksisterer.

Et kantnavn findes som kanten mellem de to navngivne flader, som hver først findes efter ovenstående regler. Er der præcis én kant imellem dem, er det den korrekte, også efter en opdeling af en af fladerne.

En skitses reference til en projiceret kant (`sketch.project {edge: [x, y, z]}`, Intersect og Include 3D Geometry for en kant) gemmer kantnavnet sammen med punktet. Referencen følger kanten, selv når den flyttes. Hvis kanten opdeles, bruges stykket nærmest det oprindelige referencepunkt med en advarsel. Findes navnet ikke længere, mistes referencen, og skitsen bevarer sin seneste geometri i stedet for at vælge en tilfældig nærliggende kant. Ældre referencer uden navn, som kun gemte punktet, findes stadig ved nærmeste kant og får dens navn næste gang skitsen redigeres.

En kommando, der normalt vælger en flade via et punkt, kan i stedet bruge navnet fra `model.faces` (`face.delete`, `solid.shell`, `solid.draft`, `solid.offset_face`, `solid.split_face`, feltet `faces` i `solid.replace_face` og `face` i `sheet.convert`). Samme opslag bruges gennem `naming::locate_one`, som finder fladen med navnet eller præcis én flade, der genkender det, på det angivne eller ethvert emne. Tvetydige navne afvises. Kommandoen finder et punkt på fladen med `naming::named_face_point` (centrum for den mest rummelige trekant i fladen projiceret på et plan, en cylinder, kegle, kugle eller torus). Feature gemmer derefter punktet som ved et manuelt valg sammen med navnet. Designfilen får ingen nye felter. `model.faces` viser punktet på alle flader.

### Profiler valgt ud fra deres kurver

En profil valgt via et punkt eller indeks gemmes ved kurve-ID'erne i sin ydre kontur (`ProfileSel::Curves`) og kan derfor følge mål- og parameterændringer præcist. Hvis selve konturen redigeres, findes måske ikke længere en profil afgrænset af netop de gemte kurver. Så forsøger Feature den bedste match med en advarsel i stedet for at fejle og fjerne både sit emne og efterfølgende Features (`profile_refs::fallback_loop`):

1. Hvis alle kurver stadig findes og lukker en kontur, men en anden kurve nu opdeler området, bruges de delområder inde i konturen, der berører den, så det oprindeligt valgte område samlet bevares (indre øer forbliver huller).
2. Ellers vælges profilen, hvis ydre kontur deler flest af de gemte kurver, mindst halvdelen. Eksempelvis når en side slettes og tegnes igen eller et hak beskæres. Ved lighed vælges profilen med færrest ekstra kurver, der ikke var i den oprindelige kontur. Stadig lighed eller intet match giver fejl.

Konturer med Offset-kurver og ældre graph-span-multisets bevarer deres præcise betydning. Nye kommandokald kontrollerer profilvalg efter den præcise regel; en kurveliste uden tilhørende profil afvises ved oprettelse. En skitse på en Extrudes endeflade bruger samme fallback: Dens certifikat angiver konturen. Endefladen fra samme skitse, samme mål-emne og samme ende, som deler flest kurver (mindst halvdelen), vælges, medmindre den præcise endeflade stadig ligger på et andet emne (`Document::anchored_face_indices`).

Et hul placeret via en position på en flade er en undtagelse fra princippet om nærmeste referencepunkt. Positionen er en faktisk placering, ikke et valghjælpemiddel. Hullet følger fladen langs sin egen akse, når fladen flyttes. Hvis fladen ikke længere dækker placeringen, beholder hullet den oprindelige position og giver en advarsel frem for at glide til kanten og bore et halvt hul. Huller ved skitsepunkter følger skitsen.

## Ældre design

Design fra før format 14 gemte geometrisk udledte navne. Nogle af disse navne afviger fra de nye historikbaserede navne, f.eks. en rørs ydre væg, som var navngivet efter den skitsekurve nærmest dens centrum (den indre cirkel). Migrationen markerer navnene som `?F2:top#2`, og de findes blandt de gamle geometrisk bestemte navne, så designet evalueres som oprindeligt gemt. Hvis brugeren vælger fladen igen, gemmes dens præcise navn.

Disse Features bevarer også den gamle rækkefølge fra kernelens shells ved navngivning af emnestykker (`Feature::shell_piece_order`, som sættes ved migration). Det sikrer, at senere gemte emnenavne til Fillet, Chamfer, mål, materialer, udseender og Joints fortsat peger på de samme stykker. Nye Features navngiver emnestykker efter deres flader.

## Endnu ikke implementeret

- Certifikater (`FaceSource`, de strenge tilknytningsidentiteter for skitser på flader og konfigurerede fladestile) følger kun de kernel-stier, hvor plan-kilden stadig kan kortlægges. Hvis de blev ført videre gennem `History::face_sources`, kunne et certifikat følge en flade ændret af en senere Join. Det afvises bevidst i dag.
- Operationerne uden historik ovenfor bruger fortsat ikke-verificerede geometriske navne.

## Ordliste – engelsk til dansk

| Engelsk udtryk | Dansk betydning |
|---|---|
| Persistent Naming | Vedvarende navngivning |
| Face | Flade |
| Edge | Kant |
| Index | Indeks |
| History | Historik |
| Lineage | Afstamning |
| Provenance | Oprindelse |
| Exact | Præcis / verificeret |
| Deterministic | Deterministisk |
| Geometry Sampling | Geometrisk sampling |
| Unverified | Ikke verificeret |
| Split Face | Opdelt flade |
| Merged Face | Sammenlagt flade |
| Alias | Alternativt navn |
| Lookup | Opslag |
| Reference | Reference |
| Fall Back | Brug reservemetode |
| Profile | Profil |
| Outer Loop | Ydre kontur |
| Certificated Face | Certificeret flade |
| Legacy Design | Ældre design |
| Migration | Konvertering af ældre data |
| `model.faces` | Vis flader med navne |
| `exact: false` | Navnet er ikke verificeret |
