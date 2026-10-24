# Extensible Libraries: Materials, Appearances and Components (udvidelige biblioteker)

Den første [implementering af lokale biblioteker med simple værdier](local-libraries.md) kopierer kun farve/opacitet og definitioner af densitet og udseende. Dens separate v1-katalog er ikke det komplette system til ressourcepakker, oprindelse og opdatering, som foreslås nedenfor. Beskrivelser af teksturer og miljøer afstemmes fortsat med vedligeholderens arbejde med rendering.

Dette er et forslag til [#785](https://github.com/bherbruck/solidcraft/issues/785), [#786](https://github.com/bherbruck/solidcraft/issues/786) og [#787](https://github.com/bherbruck/solidcraft/issues/787). Siden beskriver planlagte kontrakter og godkendelseskriterier, **ikke** et implementeret pakkeformat eller nye kommandoer. De udskudte arbejdsområder Simulation og Render er ikke omfattet.

## Genbrug af eksisterende funktioner

Fysiske materialer har allerede indbyggede densiteter og designlokale definitioner (`material.create`, `material.edit`, `material.delete`, `material.list`, `material.assign`). De bruges i `component.properties`, bevares ved genåbning og indgår i konfigurationer. Udseender har et indbygget bibliotek med farver og opacitet samt tilsidesættelser pr. emne, flade og komponent. `parts.library`, `parts.insert` og `parts.resize` opretter allerede syv familier af bolte, stifter og lejeomrids som redigerbare komponenter.

Udvid disse eksisterende veje. Et fælles katalog giver identitet, søgning og oprindelsesoplysninger. Hver posttype beholder sine egne data og regler for anvendelse. Et fysisk materiale er ikke en tekstur, og en genbrugelig komponent er ikke eksekverbar pluginkode.

## Bærbare og uafhængigt udvidelige pakker

Brug én versionsstyret, offline pakkeramme til tre typer: fysisk materiale, udseende og komponentfamilie. Både projektbidragydere og brugere skal kunne oprette pakker uden at genbygge SolidCraft. Start med en mappe og et JSON-manifest, ikke et nyt arkivformat eller en online markedsplads. Det præcise skema hører til PR'en for den begrænsede indlæser og ledsages af maskinkontrollerede eksempler. Følgende er designkravene:

| Posttype | Påkrævede oplysninger |
|---|---|
| Pakke | Skemaversion, stabilt navnerums-ID, uforanderlig revision, visningsnavn, forfatter, kilde og licensmetadata |
| Katalogpost | ID i pakken, uforanderlig revision, type, navn, kategori/tags, typede data og oprindelsesoplysninger |
| Ressource | Relativ sti, indholdshash, byteantal, medietype, forfatter, kilde, licens og eventuel påkrævet licenstekst |
| Afhængighed | Eksplicit post-/revisionsreference; begrænset, cykelfri opløsning blandt valgte pakker |
| Valg | Pakke-ID + post-ID + præcis revision; etiketter og filstier er ikke identiteter |

Pakkerevisioner og skemaversioner er forskellige: En revision identificerer indholdet, mens skemaversionen fortæller, hvordan det læses. Ændring af indhold under en eksisterende revision er en fejl. Ukendte skemaversioner, posttyper og påkrævede funktioner afvises klart frem for stiltiende at blive fortolket som et ældre format. Valgfrie udvidelser skal have et eksplicit navnerum og regler for bevarelse før accept.

Tre uafhængige scopes (omfang) er nødvendige:

- **Bundled (indbygget):** Gennemgået sammen med SolidCraft og skrivebeskyttet i bibliotekseditoren.
- **Project (projekt):** Pakker åbnet eksplicit for et bestemt projekt eller hold. Filer ved siden af designet må ikke automatisk opdages, udføres eller betros.
- **User (bruger):** Lokale pakker, som brugeren opretter, importerer, redigerer eller eksporterer. Desktopappen håndterer lokale filer; browserbrugere bruger webappens lagrings- og filarbejdsgang.

Søgerækkefølgen skal være deterministisk. Ens navne må forekomme, når deres kilde vises. Gentagne fulde ID'er/revisioner med forskelligt indhold er fejl, ikke overskrivninger valgt efter scanningsrækkefølge. Redigering af indbyggede poster kræver **Save a Copy (gem en kopi)** i brugerens navnerum. Opdatering af en pakke er eksplicit og ændrer ikke eksisterende instanser i designet.

## Designet ejer sine valgte definitioner

Ved tildeling eller indsættelse kopieres definition, valgt revision, parametre, oprindelsesoplysninger og nødvendige ressourcebytes ind i designets eksisterende container- og ressourcehåndtering. Kildeidentiteten bevares til inspektion og en senere eksplicit Update-handling. Et gemt design skal kunne åbnes, genberegnes og rendere understøttede udseender uden originalpakken, netværket eller en anden brugers hjemmemappe. Installation af en pakke må ikke ændre design eller Undo-historik.

Tildeling, indsættelse og opdatering er almindelige engine-kommandoer, atomiske og med Undo. Preview ændrer ikke dokumentet. Fejlet import, manglende afhængigheder, navnekollision eller ikke-understøttet opskrift bevarer dokumentet og Undo/Redo-historikken. Eksisterende materialnavne og prioriteten mellem udseender bevarer deres tidligere betydning. Stabile ID'er introduceres med en dokumenteret migration frem for at genfortolke gamle navnebaserede tildelinger. Ændringer i designformat kræver egen formatversion, migration og genåbningstests adskilt fra pakkens skemaversion.

## Fysiske egenskaber (#785)

Valg af fysisk materiale og udseende holdes adskilt med mulighed for et standardudseende. Importeret densitet omregnes eksplicit til det eksisterende g/cm³-format. Geometri bruger fortsat millimeter og radianer. Enheder må aldrig gættes ud fra tallets størrelse. Egenskaber bør omfatte kilde, udgave/dato, materialekvalitet, behandlingstilstand og relevant temperatur eller fugtighed. Ukendte egenskaber forbliver ukendte, ikke nul eller gættede tal.

Begynd med densitet og masse gennem de eksisterende kommandoer. Reservér typede udvidelser til elasticitetskonstanter, flyde-/brudstyrke, termiske egenskaber og ortotrope akser, men vis dem ikke som klar til FEA, før model, enheder og validering er implementeret. En generisk trædensitet er ikke en verificeret teknisk materialekvalitet. Målinger fra brugeren kan gemmes med egen oprindelse og redigeres uden at ændre det indbyggede bibliotek.

## Udseender til træ og pladematerialer (#786)

Begynd med mapping lokalt på emnet, eksplicit fysisk gentagelsesstørrelse og en kornretningsramme, brugeren kan redigere. Foreslå én gang emnets længste **lokale** akse, med X/Y/Z som prioritet ved ens mål. Gem den valgte ramme frem for at genberegne den ved hver størrelsesændring. Manuel rotation og skalering skal bevares ved parameterændringer, placering af instanser, Undo og genåbning. Fælles komponentinstanser skal transformere samme mapping i komponentens lokale koordinater efter deres placering. Uendelige, ugyldige eller degenererede rammer afvises; automatisk retning er kun et forslag.

Visuel træstruktur må ikke dreje emnet, dets geometriske Bounding Box eller råmaterialets bearbejdningsramme. Strukturel fiberretning og fiberbevidst Nesting/Cut Lists er separate Features, som koordineres med [#137](https://github.com/bherbruck/solidcraft/issues/137). En anisotrop skyggeeffekt betyder heller ikke automatisk mekanisk anisotropi.

Efter simpel træmapping tilføjes særskilte udseender til over-, under- og kantflader på MDF, spånplade, krydsfiner og kompaktlaminat; senere melamin, laminat, linoleum og finer. Lagene beskriver i første omgang kun det visuelle. Fysisk belægningstykkelse eller masse kræver eksplicit geometri- eller materialeinput og må ikke være en skjult effekt af farvetildeling. Tildelinger på flader og kanter kræver vedvarende identiteter og skal afvise mistede referencer frem for at farve en tilfældig naboflade.

Teksturrolle, farverum, konvention for normal maps og fysisk skala angives eksplicit. Følg [kontrakten for renderkapaciteter](https://github.com/bherbruck/solidcraft/blob/main/docs/render-backends.md): En eksisterende Viewport med farve/opacitet beviser ikke PBR- eller teksturunderstøttelse. Rendereren skal fortælle, hvad den understøtter. Manglende maps bruger en dokumenteret Preview-fallback med synlig besked. Eksport skal bevare understøttede ressourcer og mapping eller tydeligt rapportere tab.

## Standard- og brugerdefinerede komponentfamilier (#787)

Behold den eksisterende arbejdsgang til at indsætte og ændre størrelse på befæstelsesdele. Tilføj først originale parametriske opskrifter for BAR, SHS, RHS og CHS med eksplicitte længdeenheder, parametergrænser, tværsnitsrammer og indsættelses-/forbindelsesrammer. Kontrollér vægtykkelse mod udvendigt mål, og angiv om hjørner er skarpe eller afrundede. Tilnærmet geometri må ikke kaldes en certificeret standardprofil. UB/UC kræver reelle flangeradier, vægradier og hældninger. En standardbetegnelse giver ikke automatisk ret til at viderefordele dens dimensionstabeller.

En familieres revision definerer et begrænset parameterskema, en tilladt opskrift og eventuelt en gennemgået størrelsestabel. En størrelsesbetegnelse identificerer en række, ikke et tilfældigt filnavn. Ændring af størrelse skal genberegne samme komponentidentitet og bevare understøttede forbindelser. De første opskrifter må kun bruge en kort positivliste med almindelige modelleringsoperationer. Ingen indlejret native-kode, shellkommandoer, scripts, vilkårlige kommando-ID'er eller netværkshooks. Cirkler, ubegrænset udvidelse og ikke-understøttede operationer afvises før ændring af designet.

Brugerdefinerede samlinger følger samme principper: Begrænsede afhængigheder, navngivne forbindelsesrammer, eksplicitte regler for delte/uafhængige instanser og indsættelse som ét Undo-trin. Gem valgte parametre og snapshots af definitioner, så manglende eller ændrede kataloger aldrig stiltiende erstatter en del. Geometriske omslagsmodeller, inklusive eksisterende lejeomrids, skal kunne skelnes fra detaljerede modeller, der er egnede til fremstilling.

## Kilder og licenser

SolidCraft er licenseret MIT OR Apache-2.0. Kode og genererede opskrifter skal følge Clean Room-reglerne: Kopiér aldrig materialer, skabeloner, geometri, ikoner eller kataloger fra kommercielle CAD-programmer. Tredjepartsressourcer beholder deres egne licenser. Hver medfølgende ressource skal have forfatter, kilde, licens og en række i `ATTRIBUTION.md`, også hvis kreditering er frivillig ved CC0. Registrér hashes og hentningsdatoer ved indførelse af faktiske filer. Et manifests påståede licens er en oplysning, der skal kontrolleres, ikke automatisk bevis på tilladelse.

Kilder kontrolleret den 13. oktober 2026. Forslaget medtager ingen eksterne filer eller datasæt:

| Kilde | Egnet anvendelse / betingelser |
|---|---|
| [ambientCG license](https://docs.ambientcg.com/license/) | Downloadbare aktiver og materialeforhåndsvisninger er CC0. Mulig kilde til træ- og pladeteksturer; udvælg og kreditér konkrete filer. |
| [Poly Haven license](https://polyhaven.com/license) | Aktiver er CC0. Webstedstekst, logoer og andet indhold har andre rettigheder, og API-adgang har særskilte vilkår. Kopiér ikke udbyderens implementeringskode. |
| [Wikidata structured-data license](https://www.wikidata.org/wiki/Wikidata:Copyright) | Strukturerede entitets- og egenskabsdata er CC0; andre tekster har andre vilkår. Mulig kilde til konkrete tal og ID'er, ikke uverificerede tekniske standardværdier. Bevar kildehenvisninger, forbehold og enheder. |
| [USDA Wood Handbook, revised 2021](https://research.fs.usda.gov/fpl/wood-handbook) | Reference til fugtighed og træets fysiske og mekaniske egenskaber. Kontrollér licens, art, enheder og betingelser i hver tabel før import; ingen generel tilladelse til kopiering af hele datasættet. |
| [NIST data/publication rights](https://www.nist.gov/open/copyright-fair-use-and-licensing-statements-srd-data-software-and-technical-series-publications) og [repository terms](https://materialsdata.nist.gov/terms/) | Kun mulige kilder til materialeegenskaber. Uploadede data har udbydervalgte vilkår, og offentlig tilgængelighed er ikke automatisk en videredistributionslicens. Gennemgå hvert datasæt. |
| [Khronos PBR overview](https://www.khronos.org/gltf/pbr) | Offentlige konventioner for teksturroller og lysberegning. Designreference, ikke en kilde til CAD-materialedata. |
| Originale SolidCraft-opskrifter / brugermålinger | Foretrukne første komponentopskrifter og illustrative procedurale forhåndsvisninger. Bevar ophav og skeln mellem illustrationer og målinger. |

Foretræk CC0 til den første teksturpakke og originale opskrifter til de første komponentfamilier. Udskyd uklare eller restriktive data frem for at scrape leverandørhjemmesider, kopiere standardpublikationer eller antage, at en downloadbar CAD-fil frit kan videredistribueres.

## Faser og godkendelseskriterier

Hver implementerings-PR skal henvise til de relevante issues uden at lukke dem for tidligt.

| Fase | Leverance | Krav til dokumentation, før fasen kaldes færdig |
|---|---|---|
| 0 | Forslag, kildegennemgang og roadmap | Kontrakt klar til vedligeholdergennemgang; adskil eksisterende og planlagte funktioner; ingen runtime- eller formatændringer. |
| 1 | Fælles begrænset manifest-parser, resolver og pakkevalidering/import/eksport | Projekt- og brugermanifester bruger samme skema. Konflikter, ugyldige versioner/ID'er, manglende ressourcer/afhængigheder, cykler, sti-traversering og overskredne ressourcegrænser afvises atomisk. Native og wasm skal bestå. |
| 2 | Materiale-/udseendekatalog og browserbrugerflade | Opret, kopiér, redigér, importér og eksportér fra UI med eksisterende kommandoer. Test analytisk masse af Box før og efter tildeling, konfigurationer, Undo, offline genåbning og dokumentér med rigtige skærmbilleder. |
| 3 | Træmapping og lille gennemgået startpakke | Drej/skalér fiberretning på en fri testblok og placeret instans uden uventet vending ved størrelsesændring. Genåbn uden pakken. Test rendererens kapaciteter og fallback og vis skærmbilleder. Kreditér alle medfølgende aktiver. |
| 4 | Parametriske tværsnitsfamilier og kontrollerede standardmål | BAR: areal = bredde × højde; skarpe RHS/SHS: ydre minus indre areal; CHS: π(udvendig diameter² − indvendig diameter²)/4. Test volumen = areal × længde, hulprofil, indsættelsesramme, resize, forbindelser, Undo, offline genåbning og skærmbilleder. |
| 5 | Brugerdefinerede samlinger, pladelag til flader/kanter og flere kataloger | Begrænset indlejret indsættelse, delte/uafhængige instanser, vedvarende fladereferencer, eksplicitte opdateringer og rapportering af tab. Tests og skærmbilleder for hver arbejdsgang. |

Efter fase 1 kan integration af materialer/udseender og tværsnitsfamilier udvikles uafhængigt. Træmapping afhænger af reel understøttelse i rendereren, ikke af tværsnitsfamilier. Fase 5 deles i separate PR'er efter de tidligere arbejdsgange frem for én stor ændring, der kobler indsættelse af samlinger sammen med farvelægning af flader.

Parsing og filadgang skal ligge i `io`, gemte valgte definitioner i `doc`, anvendelses- og valideringskommandoer i `engine`, renderer-specifik fortolkning i `render` eller dens adaptere og søgning/redigering i `ui-egui`. Lavere lag må ikke afhænge af brugerflade eller platformsfildialoger. Genbrug containerformatets fjernelse af identiske ressourcebytes frem for at oprette endnu et bloblager.

Håndhæv før allokering/afkodning grænser for manifestets byte, poster, strenge og afhængigheder, samlet ressourcestørrelse, billeddimensioner/dekodede bytes samt udvidelse af opskrifter. Stier skal være relative, normaliserede og begrænset til den valgte pakke, også ved symbolske links. Fase 1 skal offentliggøre præcise grænser og teste dem. Manglende filer må ikke hentes automatisk, og eksekverbare udvidelser må ikke installeres. Eksplicit import/eksport er tilstrækkeligt første gang.

Kør `cargo xtask ci` for hver implementeringsdel, og kontrollér PR-resultaterne på GitHub. Geometritests skal bygge på analytiske værdier, og ændringer i synlig CAD-funktionalitet eller UI dokumenteres med faktiske appresultater. De tre issues holdes åbne, indtil hver arbejdsgang opfylder kriterierne.

## Ordliste – engelsk til dansk

| Engelsk udtryk | Dansk betydning |
|---|---|
| Extensible Libraries | Udvidelige biblioteker |
| Material | Materiale |
| Physical Material | Fysisk materiale |
| Appearance | Udseende |
| Component Family | Komponentfamilie |
| Pack | Pakke |
| Manifest | Manifest / pakkeoversigt |
| Schema | Dataskema |
| Entry | Katalogpost |
| Revision | Revision |
| Resource | Ressource |
| Dependency | Afhængighed |
| Provenance | Oprindelse |
| Bundled | Indbygget |
| Project Scope | Projektomfang |
| User Scope | Brugeromfang |
| Preview | Forhåndsvisning |
| Clean Room | Uafhængig implementering |
| Wood Grain | Træets fiberretning |
| Texture Mapping | Teksturanbringelse |
| PBR | Fysisk baseret rendering |
| Acceptance Gate | Godkendelseskriterium |
| Roadmap | Udviklingsplan |
| Bounding Box | Afgrænsningskasse |
| Stock Frame | Referenceramme for råmateriale |
| Nested Assembly | Indlejret samling |
| Offline Reopening | Genåbning uden netværksadgang |
