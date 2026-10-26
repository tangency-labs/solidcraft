# Kommandoreference

Her finder du SolidCrafts kommandoer med deres **engelske navn og danske betydning**.
Listen hentes ved hvert docs-build fra programmets kommandoregister med `solidcraft-cli commands`.
De danske navne kommer fra programmets danske oversættelsesfil, så dokumentationen og brugerfladen
bruger de samme ord. Hvis en ny kommando endnu ikke har en dansk oversættelse, vises det engelske navn.

Alle kommandoer udføres gennem det samme kommandosystem, uanset om du bruger værktøjslinjen,
et script, `solidcraft-cli exec <id> '<json>'`, kontrolkanalen eller MCP.
Læs også [kommandosystemet](../dev/commands.md), [kontrolprotokollen](../dev/control-protocol.md)
og [ordlisten](glossary.md).

## Sådan læser du listen

- **Kommando-ID:** det faste navn i kode, f.eks. `solid.extrude`. ID'et skal skrives uændret,
  også når du bruger dansk i programmet.
- **Navn:** det engelske navn efterfulgt af den danske oversættelse. Det gør det lettere at
  sammenholde en engelsk vejledning med den danske brugerflade.
- **Genvej:** en eventuel tastaturgenvej står i parentes efter navnet.
- **Parametre:** linjen under kommandoen beskriver dens input. Parameternavne, tilladte værdier,
  kode og den tekniske parametertekst er bevaret fra registret. De oversættes ikke.

Et `?` markerer en valgfri parameter. `expr` betyder et tal eller et udtryk med enheder.
JSON-nøgler, kommando-ID'er og faste tekstværdier skal altid følge den tekniske beskrivelse;
den danske forklaring er ikke et alternativt inputformat.

Kommandoer åbner ikke selv en dialogboks. Brugerfladens værktøjer samler først dine valg og
sender derefter parametrene til kommandoen. Nogle kommandoer kræver derfor et åbent design,
en aktiv skitse eller en gyldig markering, før de kan udføres.

## Ordliste – engelsk til dansk

| Engelsk eller teknisk udtryk | Dansk betydning |
|---|---|
| Command | Kommando |
| Command ID | Fast identifikation af en kommando |
| Label | Navn i brugerfladen |
| Shortcut | Tastaturgenvej |
| Parameters | Parametre: de input, kommandoen modtager |
| Optional / `?` | Valgfri |
| `expr` | Tal eller udtryk, eventuelt med enhed |
| `bool` | Sand eller falsk: `true` eller `false` |
| Body | Emne |
| Component | Komponent |
| Occurrence | Forekomst af en komponent |
| Sketch | Skitse |
| Constraint | Geometrisk binding |
| Dimension | Mål |
| Extrude | Ekstrudering |
| Revolve | Rotation af en profil omkring en akse |
| Sweep | Før en profil langs en bane |
| Loft | Opret en form gennem flere profiler |
| Torus | Ringformet emne |
| Fillet | Afrunding |
| Chamfer | Fasning |
| Pattern | Mønster af gentagne kopier |
| Face | Flade |
| Edge | Kant |
| Vertex | Hjørnepunkt |
| Plane | Plan |
| Path | Bane eller sti, afhængigt af parameteren |
| Toolbar tab / panel | Fane / gruppe på værktøjslinjen |

## Kommandoer fra programmet

Grupperne nedenfor følger det engelske kommandoregister, så faner og paneler kan genkendes
i engelske vejledninger. SOLID er massiv modellering, SKETCH er skitsering, SURFACE er
overflader, SHEET METAL er pladekonstruktion, og PLASTIC er plastemner.
CREATE betyder opret, MODIFY betyder rediger, og Other samler øvrige kommandoer efter ID-præfiks.

{{#command-reference}}
