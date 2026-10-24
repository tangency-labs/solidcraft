# Parameters and Expressions (parametre og udtryk)

Alle mål og input til Features (modelleringsfunktioner) er **Model Parameters (modelparametre)**, f.eks. `d1`, `d2` osv. **User Parameters (brugerdefinerede parametre)** er parametre, du selv opretter med en enhed, et udtryk og eventuelt en kommentar. De ændres under **Change Parameters (rediger parametre)** (`parameters.change`). Timeline (tidslinjen) genberegnes fra den første Feature, der bruger en ændret parameter.

Når du opretter eller redigerer et skitsemål, viser værdifeltet navnet på modelparameteren (`d1`, `d2` osv.). Klik på et andet styrende mål (**Driving Dimension**) for at bruge dets parameternavn. Hvis værdien kun er et tal, f.eks. `10` eller `10 mm`, erstattes den af navnet. Ellers indsættes navnet ved markøren og erstatter eventuelt markeret tekst, mens resten af det, du har skrevet, bevares. Skriver du f.eks. `10 + ` og klikker på `d1`, bliver udtrykket `10 + d1`. Tilføj f.eks. `/ 2` for at gøre målet afhængigt af et andet mål. Tryk **Enter** for at anvende udtrykket eller **Escape** for at annullere. Opret og rediger brugerdefinerede parametre med **Change Parameters**.

I panelet **CONFIGURE** under værktøjslinjen **SOLID** åbner knappen **Parameters** editoren for brugerdefinerede parametre og modelparametre. Den separate knap **Configurations** åbner konfigurationstabellen. **Configure** findes fortsat i dropdown-menuen til at konfigurere kolonner.

Tryk **Enter** i feltet for en ny parameters navn, udtryk eller kommentar for at tilføje parameteren. Hvis oplysningerne er gyldige, flyttes fokus tilbage til navnefeltet, så du kan oprette den næste parameter. Ugyldige indtastninger forbliver redigerbare.

I felter til udtryk kan du begynde at skrive et parameter- eller målnavn for at få forslag via autocomplete (autofuldførelse). Tryk **Ctrl+Space** for at åbne forslagene, også i et tomt felt. **Pil op/ned** vælger et forslag, **Enter/Tab** accepterer det, og **Escape** lukker forslagslisten. Valg af et forslag gemmer ikke samtidig parameteren.

Udtryk tager højde for måleenheder, f.eks. `2 * width + 5 mm`, `angle / 2`, `sqrt(area)` og `1 in + 3 mm`. Du kan bruge operatorerne `+ - * / ^`, parenteser, funktionerne `sin cos tan asin acos atan atan2 sqrt abs min max floor ceil round mod trunc` og enhedskonvertering.

Decimalkomma understøttes uden for parenteser: `2,5 mm` svarer til `2.5 mm`. Inde i en funktions parenteser adskiller kommaet argumenter, så skriv `max(2.5, 3)`.

Blandede tal i tommer behandles som én værdi. Eksempelvis svarer `2 1/8"` til 2,125 tommer og `3 7/16 in` til 3,4375 tommer. Formatet er et helt tal efterfulgt af et mellemrum og en ægte brøk med hele tal.

Hvis et udtryk giver en længde, hvor der forventes en vinkel, eller omvendt, vises en fejl i stedet for en automatisk konvertering. **Cyclic Dependencies (cirkulære afhængigheder)** vises med deres kæde, f.eks. `a → b → a`. Hvis du omdøber en parameter, opdateres alle udtryk, mål og Feature-input, der bruger den. En parameter i brug kan ikke slettes; fejlmeddelelsen viser, hvor den anvendes.

Designets måleenheder vælges under **Document Settings (dokumentindstillinger)** i designtræet: mm, cm, m, in eller ft. Enhederne bestemmer, hvordan tal uden angivet enhed fortolkes i en Feature. I et design med tommer betyder `2` derfor 2 in.

Dialogerne for Box, Cylinder, Sphere, Torus og Coil viser som udgangspunkt størrelser i designets enheder med enhedsangivelsen, f.eks. `20 mm` eller `20 in`. Hvis du ændrer enhederne, flyttes eksisterende geometri ikke.

En størrelse beregnet fra et tal uden enhed beholder den enhed, tallet blev fortolket med. En kugle med en diameter på 20 i et millimeterdesign får derfor radius `(20 mm) / 2`. Ved et udtryk uden enhed, f.eks. `2 * 5`, eller en parameter uden enhed forbliver tallet uden enhedsangivelse og fortolkes efter designets enheder.

Parametre kan eksporteres til og importeres fra CSV eller JSON med `parameters.export` og `parameters.import`.

**Configurations (konfigurationer)** (`config.*`) gemmer en tabel med rækker, hvor hver række har sine egne parameterværdier, deaktiverede Features, fysiske materialer og emnernes udseende. Skift konfiguration med `config.activate`.

Tabellens **Library Picker (biblioteksvælger)** bruges til at vælge materialer og udseender. Vælg **Follow Material (følg materiale)** for at fjerne en tilsidesættelse af udseendet. Brugerdefinerede udseender bevarer deres farve og opacitet ved aktivering, Undo samt gemning og genåbning. Se [kommandoer og eksempler til konfigurationer](https://github.com/bherbruck/solidcraft/blob/main/docs/configurations.md).

## Ordliste – engelske udtryk på dansk

De engelske betegnelser er de navne, du møder i SolidCraft og i scripts. Kommandoer og feltnavne skal fortsat skrives på engelsk.

| Engelsk udtryk | Dansk oversættelse |
|---|---|
| Parameters | Parametre |
| Expressions | Udtryk / formler |
| Model Parameter | Modelparameter |
| User Parameter | Brugerdefineret parameter |
| Dimension | Mål / målangivelse |
| Driving Dimension | Styrende mål |
| Feature | Modelleringsfunktion |
| Timeline | Tidslinje |
| Change Parameters | Rediger parametre |
| CONFIGURE | Konfiguration |
| SOLID | Solidmodellering |
| Configurations | Konfigurationer |
| Configure | Konfigurér |
| Autocomplete | Autofuldførelse |
| Expression Field | Felt til udtryk |
| Unit | Måleenhed |
| Unit Conversion | Enhedskonvertering |
| Decimal Comma | Decimalkomma |
| Mixed Number | Blandet tal |
| Cyclic Dependency | Cirkulær afhængighed |
| Document Settings | Dokumentindstillinger |
| Box | Kasse |
| Cylinder | Cylinder |
| Sphere | Kugle |
| Torus | Torus / ringformet emne |
| Coil | Spiral |
| Library Picker | Biblioteksvælger |
| Physical Material | Fysisk materiale |
| Appearance | Udseende |
| Follow Material | Følg materiale |
| Opacity | Opacitet / gennemsigtighed |
| Suppressed Feature | Midlertidigt deaktiveret Feature |
| Undo | Fortryd |
| Export | Eksportér |
| Import | Importér |

### Kommandoer og tekniske betegnelser

| Engelsk navn | Dansk betydning |
|---|---|
| `d1`, `d2` | Navne på modelparametre |
| `parameters.change` | Rediger en parameter |
| `parameters.export` | Eksportér parametre |
| `parameters.import` | Importér parametre |
| `config.*` | Kommandoer til konfigurationer |
| `config.activate` | Aktivér en konfiguration |
| `width` | Bredde |
| `angle` | Vinkel |
| `area` | Areal |
| `sqrt` | Kvadratrod |
| `abs` | Absolutværdi |
| `min` | Mindste værdi |
| `max` | Største værdi |
| `floor` | Rund ned |
| `ceil` | Rund op |
| `round` | Afrund |
| `trunc` | Fjern decimaldelen |
| `mod` | Rest ved division |
| `sin`, `cos`, `tan` | Trigonometriske funktioner |
| `asin`, `acos`, `atan`, `atan2` | Inverse trigonometriske funktioner |
| `mm` | Millimeter |
| `cm` | Centimeter |
| `m` | Meter |
| `in` | Tommer |
| `ft` | Fod |
