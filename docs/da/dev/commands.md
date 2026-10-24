# The Command Engine (kommandosystemet)

Alle handlinger, som brugeren kan udføre, er **commands (kommandoer)**: en `CommandSpec` registreret i `crates/engine/src/cmd/` med et punktumadskilt ID med små bogstaver, f.eks. `solid.extrude`, `sketch.create` eller `timeline.rollback`. Værktøjslinjen, dialoger, Marking Menu, S Box, scripts, CLI, kontrolkanalen og MCP udfører alle kommandoer gennem `Session::execute(id, params)`. Kommandoerne modtager JSON-parametre og åbner aldrig dialoger. En dialog er blot en formular i brugerfladen, som sammensætter parametrene.

```rust
CommandSpec::new("solid.fillet", "Fillet", fillet)
    .at("SOLID", "MODIFY")      // toolbar tab and panel ("" = not on the toolbar)
    .icon("fillet")             // drawn in code, assets/icons/ui/source/icons.rs
    .key("F")                   // default shortcut
    .params("edges: [[x,y,z] | {body, index}]; radius: expr; …"),
```

`run` har typen `fn(&mut Session, &Value) -> Result<Value>`, og `enabled` har typen `fn(&Session) -> Result<(), String>`. Fejlteksten fra `enabled` bruges som tooltip for en deaktiveret knap. Strengen `params` er den korte dokumentation, der vises af `solidcraft-cli commands`, MCP-værktøjet `list_commands` og [kommandoreferencen](../reference/commands.md).

En kommando, der ændrer designet, kan som standard fortrydes. Sessionen tager et snapshot af dokumentet før udførelsen, og en fejlet kommando efterlader dokumentet uændret. Feature-kommandoer tilføjer en Feature til tidslinjen og lader dokumentlaget beregne den. Input valideres først (enheder, intervaller og referencer), så ugyldige input giver `BadParams` frem for en defekt Feature.

Kun kommandoregistrets egne punktumadskilte ID'er er gyldige. Store og små bogstaver ignoreres ved sammenligning. Ukendte ID'er giver fejlen `unknown command` med op til tre lignende navne, f.eks. ved skrivefejlen `solid.extrud` eller ved kommandoer med samme sidste led (`file.open`: mente du `doc.open`?).

## Tilføjelse af en kommando

1. Tilføj en `CommandSpec` i den relevante fil under `crates/engine/src/cmd/`, og implementér `run`.
2. Skriv engine-tests, der måler resultatet: volumen, areal og antal flader, kanter og hjørnepunkter mod værdier, der kan beregnes manuelt.
3. Testen for ugyldige input (`hostile_params_never_panic` i `crates/engine/src/tests.rs`) kører automatisk din kommando med ugyldige værdier i alle dokumenterede parametre. Kommandoen skal returnere en fejl, aldrig panic.
4. Ved arbejde med brugerfladen skal du tilføje et testscenario (se [Testmiljøer](testing.md)).

## Ordliste – engelsk til dansk

| Engelsk udtryk | Dansk betydning |
|---|---|
| Command Engine | Kommandosystem |
| Command | Kommando |
| CommandSpec | Definition af en kommando |
| Command Registry | Kommandoregister |
| Parameters | Parametre |
| Dialog | Dialogboks |
| Front End | Brugerflade |
| Tooltip | Hjælpetekst |
| Snapshot | Øjebliksbillede af dokumenttilstanden |
| Undoable | Kan fortrydes |
| Feature | Modelleringsfunktion |
| Validation | Validering |
| BadParams | Fejl ved ugyldige parametre |
| Hostile Input | Bevidst ugyldigt input |
| Panic | Uventet programfejl |
| Test Harness | Testmiljø |
