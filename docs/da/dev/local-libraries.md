# Local Scalar Libraries (lokale biblioteker med simple værdier)

Den første del af den lokale biblioteksfunktion (#846) genbruger eksisterende kommandoer til udseende og fysiske materialer. Det er en lille anvendelse af det større [biblioteksforslag](libraries.md), ikke en komplet implementering af ressourcepakker. Den tilføjer hverken teksturkortlægning, træstruktur, PBR-shaders, HDRI-belysning, onlineopdagelse eller en ny version af dokumentformatet.

## Arbejdsgang i desktopprogrammet

1. Tildel et navngivet udseende med farve og opacitet under **Appearance**, eller definér et fysisk materiale med de eksisterende materialekommandoer. Et udseende ændrer ikke materialets densitet.
2. Vælg **Appearance → Local Library → Export Design Library… (eksportér designbibliotek)**. Øjebliksbilledet indeholder eksplicitte udseender på emner, flader og komponenter samt designlokale fysiske materialer med deres standardudseender. Det eksporterer ikke farver fra importeret kernel-geometri, ubrugte indbyggede katalogposter eller ikke-anvendte ændringer i farvevælgeren.
3. Åbn biblioteket i et andet design, søg efter et udseende, vælg det og tryk **Apply Library Appearance (anvend biblioteksudseende)**. De sædvanlige mål-emner, flader og komponenter genbruges. Et efterfølgende OK skaber ikke et ekstra Undo-trin.
4. Gem designet. De valgte enkle definitioner kopieres som almindelige designdata. Kildebiblioteket kan derfor flyttes eller slettes uden at ændre det gemte resultat.

Eksport ændrer ikke designet, dets status for ugemte ændringer eller historikken. En eksisterende fil kan kun overskrives, når **Replace Existing Library File (erstat eksisterende biblioteksfil)** vælges eksplicit. Eksport er et øjebliksbillede af designets definitioner og fletter ikke automatisk med eksisterende katalogposter. Redigering af et kopieret materiale eller udseende påvirker ikke kildefilen.

Den samlede render-integration bruger den asynkrone vælger fra main, PR #1127. Modelændringer og faneskift venter på dens svar, men tegning og ændring af vinduets størrelse fortsætter. Tilladelse til overskrivning indhentes inden åbning; eksporten tager øjebliksbilledet af designet, når `library.export` kører efter destinationsvalget. Dokumentidentiteten kontrolleres, så et forsinket biblioteksresultat ikke overskriver en anden Appearance-editor. Annullering læser eller skriver ikke noget bibliotek. Den tidligere kandidat #1114 er ikke længere en runtime-afhængighed.

Desktop-panelet anvender udseender. Import og tildeling af fysiske materialer findes via `library.apply`, og importerede definitioner vises også i den eksisterende materialeliste. Browserappen kan eksportere et bibliotek via sin downloadtjeneste. Interaktiv åbning af lokale biblioteker er i denne første version kun en desktopfunktion. Browserens engine-kommandoer kan bruge et begrænset katalog, der allerede findes i VFS. At wasm kompilerer betyder ikke, at interaktiv browserimport og downloadarbejdsgange er implementeret.

## Kommandoer

Alle programmatiske kald bruger JSON og åbner aldrig en fildialog:

```json
{"command":"library.export","params":{"path":"my-library.solidcraft-library.json"}}
{"command":"library.inspect","params":{"path":"my-library.solidcraft-library.json"}}
```

Inspection/export returns a `catalog` with qualified entry IDs and exact revisions. Use
the returned values when applying; do not replace an unknown revision with a guessed one:

Inspektion og eksport returnerer et `catalog` med fuldt kvalificerede post-ID'er og præcise revisioner. Brug de returnerede værdier ved anvendelse; gæt aldrig en ukendt revision:

Appearance targets can be `bodies`, `components` or `faces: [{body, point: [x,y,z]}]`.
Points are world-space millimetres. At most 1,024 total targets are accepted. Missing
objects and invalid targets fail; the importer does not invent a nearby replacement.

Udseender kan tildeles `bodies`, `components` eller `faces: [{body, point: [x,y,z]}]`. Punkter angives i verdenskoordinater i millimeter. Højst 1.024 mål kan vælges samlet. Manglende objekter og ugyldige mål afvises; importøren finder ikke på en lignende erstatning.

En materialepost kan kopieres uden mål eller tildeles emner. Det valgfrie `name` angiver eksplicit et nyt navn på fysisk materiale under import. Identiske eksisterende definitioner genbruges. Modstridende definitioner og indbyggede navne afvises. Densitet, udseende og konfigurationsfelter overskrives aldrig uden videre. Oprettelse og tildeling deler én overordnet Undo-transaktion gennem de eksisterende materialekommandoer. Massen beregnes fortsat som volumen × densitet. En generisk eksempeldensitet er ikke en verificeret materialekvalitet.

`library.export` accepterer også `id` (standard `user.local`), `name` (standard `My library`), `provenance` og `overwrite` (standard false). Kun eksplicitte tildelinger og designlokale definitioner undersøges uden at evaluere geometri. Identiske definitioner samles; forskellige værdier med samme visningsnavn forbliver særskilte poster.

## Scalar Catalog v1 (katalog med simple værdier)

Det selvstændige JSON-format identificeres som `solidcraft-library/1` og har `id`, `name` og listen `entries`. Hver post indeholder `id`, `revision`, `provenance` og en typet `definition`:

- `appearance`: `value: {name, color: [r,g,b], opacity}` med sRGB-byteværdier og endelig opacitet fra 0 til 1.
- `material`: `value: {name, density_g_cm3, appearance?}` med endelig densitet over 0 og højst 1.000 g/cm³ samt eventuelt et udseende med samme simple struktur.

Ukendte felter afvises i alle objekter. Ikke-understøttede typer (herunder teksturer og miljødata) og formatversioner giver eksplicit fejl frem for at miste oplysninger eller fremstille et farve-preview som et tekstureret materiale. Katalog-ID'er er 1–64 ASCII-byte, begynder med et lille bogstav og fortsætter med små bogstaver, tal, `.`, `_` eller `-`. Et post-ID er `<catalog-id>/<local-id>` med samme regler for lokalt ID. Filstier og visningsnavne er ikke ID'er.

En posts revision er SHA-256 med små bogstaver af kompakt serde JSON v1 `[provenance, definition]`. Rækkefølgen af struct-felter, udeladelse af ikke-angivne valgfrie felter, UTF-8-escaping og f64-talformatering, der bevarer værdier ved genindlæsning, er del af formatkontrakten. Det er ikke vilkårlig JSON-kanonisering. Brug helst `library.export` eller Rust-API'et `Entry::new`/`content_revision` ved oprettelse af revisioner. En fremtidig inkompatibel writer kræver en ny katalogversion. Eksport-ID'er indeholder som standard type og 16 hextegn fra revisionen. Kollisioner afvises og bruges aldrig til at overskrive en anden definition. Eksplicit oprettede stabile lokale ID'er kan bevare identiteten på tværs af revisioner. Anvendelse kræver den præcist valgte revision; ændringer af andre poster ændrer ikke denne definitions identitet.

`provenance` indeholder påkrævet `license`, eventuelt `author` og `source_url`. Det er oplysninger leveret af afsenderen og ikke dokumentation for tilladelse eller en digital udgiversignatur. URL'er hentes aldrig. Standardværdien `LicenseRef-User-Provided` hævder ikke, at brugerdata har en åben licens. Ingen eksterne billeder eller data medfølger i denne del. Oprindelse og revisioner forbliver i kataloget; den første anvendelse kopierer kun simple værdier til eksisterende `Look`/`UserMaterial`-poster uden et vedvarende link tilbage til kildekatalog eller opdateringer. Indlejring af alle ressourcer, oprindelse ejet af designet, scopes og eksplicitte opdateringslinks er senere forslag.

## Grænser og fejlhåndtering

Indlæseren accepterer højst 4 MiB JSON og 1.024 poster. Afkodning stopper ved grænsen og bruger fejlhåndteret vækst af vektorer. Navne trimmes, må ikke være tomme og må højst være 128 UTF-8-byte. Grænserne for `license`, `author` og `source_url` er 128, 256 og 2.048 byte. Kontroltegn, ugyldige ID'er, dubletter, forkerte revisioner og ikke-endelige tal afvises. Eksport undersøger højst 100.000 kildereferencer og samler dem i højst 1.024 poster. Dette begrænser input og arbejdet, men garanterer ikke et bestemt hukommelsesforbrug eller svartid.

Native læsninger er begrænsede almindelige fillæsninger. Browseren kontrollerer VFS-størrelsen før kopiering. Native skrivning uden overskrivning genbruger atomisk oprettelse af nye filer. Eksplicit erstatning er atomisk uden først at læse eller sikkerhedskopiere en vilkårligt stor gammel fil. Alle biblioteksstier respekterer de gældende MCP-/kontrolkanalbegrænsninger, også ved overskrivning. Browserens VFS-udgivelse er synkron og bruger værtens downloadtjeneste. Der tilføjes ingen automatisk søgning, hentning af ressourcer, relative filreferencer, eksekverbare opskrifter eller plugins.

Fejlet anvendelse bevarer dokumentet og Undo/Redo-historikken. Kildebiblioteket læses og kontrolleres før kopiering. En forældet valgt revision kræver eksplicit genindlæsning og nyt valg. Det kopierede design forbliver uafhængigt efter vellykket tildeling, Undo/Redo og genåbning. Biblioteket afhænger ikke af den kommende renderstack og ændrer ikke dokumentformatet. Brugerrettet dokumentation og faktiske native-/wasm-resultater registreres i PR'en; geometri- og robusthedstests forbliver separate, og referencebaselines må ikke svækkes.

Hvis en udseendeindstilling på et emne, en komponent, en flade eller et materiales standardudseende indeholder PBR-parametre, afviser den opake PBR-forbruger eksport i v1-formatet. Hverken tilladelse til overskrivning eller en eksisterende outputfil gør det lovligt at nedgradere data med tab. Brug et rigere transportformat, når det findes. Import fra scalar catalog vælger fortsat eksplicit scalar-svaret.

## Ordliste – engelsk til dansk

| Engelsk udtryk | Dansk betydning |
|---|---|
| Local Library | Lokalt bibliotek |
| Scalar Catalog | Katalog med simple værdier |
| Appearance | Udseende |
| Physical Material | Fysisk materiale |
| Density | Densitet |
| Opacity | Opacitet |
| Provenance | Oprindelsesoplysninger |
| Revision | Revision |
| Entry | Katalogpost |
| Catalog | Katalog |
| Target | Målobjekt |
| Snapshot | Øjebliksbillede |
| Overwrite | Overskriv |
| Deduplication | Fjernelse af dubletter |
| Scalar | Enkel tal- eller farveværdi |
| Texture | Tekstur |
| PBR | Fysisk baseret rendering |
| Shader | Shader |
| Material Assignment | Materialetildeling |
| VFS | Virtuelt filsystem |
| Atomic Write | Atomisk skrivning |
| No-clobber | Opret uden overskrivning |
| Failure Behavior | Fejlhåndtering |
| Stale Revision | Forældet revision |
| Resource Pack | Ressourcepakke |
| `library.export` | Eksportér bibliotek |
| `library.inspect` | Undersøg bibliotek |
| `library.apply` | Anvend bibliotekspost |
