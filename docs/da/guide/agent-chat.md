# Agent Chat (chat med AI-agent)

Agent Chat er et sidepanel, hvor du kan kommunikere med en kodeagent, du allerede bruger (Claude Code, Codex, Google Antigravity, OpenCode, Grok Build m.fl.), eller med en model via en OpenAI-kompatibel API-endpoint, du selv vælger (Ollama eller LM Studio lokalt, OpenAI eller en anden URL). Agenten kan arbejde på det design, der er åbent. En kodeagent kører på din computer under **din egen konto** hos agentudbyderen. SolidCraft sælger eller videresender ikke modeller og kræver ingen API-nøgle til en kodeagent. En eventuel nøgle til et endpoint gemmes i operativsystemets sikre nøglelager, ikke i SolidCrafts indstillinger.

Kun til desktopprogrammet.

## Slå Agent Chat til eller fra

Alle indstillinger findes under **Preferences → AI**.

- **Show AI Features (vis AI-funktioner)** er som standard aktiveret og er hovedkontakten. Slås den fra, skjules AI-knappen, genveje, menupunkter og panelet. SolidCraft søger ikke efter eller starter agenter. Indstillingssiden forbliver tilgængelig, så funktionen kan aktiveres igen.
- **Show Button in Toolbar (vis knap på værktøjslinjen)** er aktiveret som standard. AI-knappen vises yderst til højre på værktøjslinjen uanset vinduets bredde. Slå den fra for kun at beholde genvejen og menupunktet.

Når AI-funktioner er aktiveret, åbnes panelet via:

- **AI**-knappen yderst til højre på værktøjslinjen.
- **Ctrl+Shift+A** (Cmd+Shift+A på Mac), også fra chattens eget tekstfelt.
- **View Settings (visningsindstillinger)**, sidste knap på navigationslinjen under visningen → **Agent Chat**.

Intet starter, før det er nødvendigt. SolidCraft søger efter installerede agenter i baggrunden med et hurtigt `--version`-kald første gang panelet eller AI-indstillingerne åbnes. En agent startes først, når du sender en besked eller trykker **Connect (forbind)**.

## Panelet

- **Docking (fastgør panelet):** Træk i panelets overskrift ved det prikkede håndtag for at flytte det til den anden side af vinduet. Den mulige placering fremhæves. Du kan også vælge **Left (venstre)** eller **Right (højre)** under Preferences → AI → Panel. Valget huskes.
- **Width (bredde):** Træk panelets inderkant for at ændre bredden. Den huskes.
- **Own Window (eget vindue):** Knappen med vindue og pil i overskriften flytter chatten til et separat vindue med designets navn. Vinduet kan flyttes til en anden skærm. Lukning med vinduets egen krydsknap lukker chatten, som næste gang åbnes i det separate vindue. **Dock** flytter den tilbage i sidepanelet. Indstillingen findes også under Preferences → AI → Panel. Placering og størrelse huskes.
- **Minimise (minimér):** Knappen med stregen i overskriften folder panelet sammen til en smal stribe med chatikon, agentstatus og antal spørgsmål, der afventer svar. Klik på ikonet for at åbne igen. **Close (luk)** skjuler panelet. Ingen af delene stopper agenten; brug **Stop** til det.
- **Agent Switcher (agentskifter):** Vælg agenten for samtalen i listen under titlen. Skift stopper den tidligere agent. Hvis agenten har egne tilstande, f.eks. Claude Codes Default, Accept Edits og Plan, vises endnu en liste. Rapporterer agenten sine modeller via ACP eller endpointets `/models`, vises en modelliste, hvor du kan skifte model. SolidCraft husker det seneste modelvalg pr. agent og gendanner det i næste session. For endpoint-agenter bliver modellen også standardmodel. Agenter uden modelliste viser ikke dette valg.

## Filer og billeder

En besked kan indeholde op til otte filer, så agenten kan se f.eks. et konceptbillede, en skitse, en opgavebeskrivelse eller en komponent, som andre dele skal passe omkring.

- **+** ved siden af **Send (send)** tilbyder **Choose a File… (vælg fil)** og **Paste a Picture (indsæt billede)** fra udklipsholderen.
- **Drop Files on the Chat (træk filer ind):** Filer sluppet på panelet eller dets sammenfoldede stribe vedhæftes næste besked og åbnes ikke samtidig i appen. Under træk vises dette i chatten. Vindueshåndteringen angiver ikke en præcis position under alle træk; derfor regnes et slip som chat, hvis markøren sidst blev set over chatten. **+** virker altid.
- Vedhæftninger vises som små felter over tekstboksen med miniaturebillede, filnavn og × til fjernelse. Billeder læses på en baggrundstråd, så store filer ikke fryser vinduet. En spinner vises imens, og **Send** venter. Filer kan sendes uden tekst.
- **Pictures (billeder)** i PNG, JPEG, GIF og WebP sendes uændret, når den længste side er højst 2048 px og størrelsen højst 3,75 MB. Så er Base64 under 5 MB pr. billede, og otte kan være i én prompt. Større billeder skaleres til 2048 px og genkodes som PNG, hvis det passer, ellers JPEG uden gennemsigtighed, med yderligere halvering efter behov. GIF sendes som første frame. BMP og TIFF omdannes til PNG. HEIC kan ikke læses og skal først eksporteres som JPEG eller PNG. Hvis en agent ikke understøtter billeder, udelades de med en besked.
- **Text Files (tekstfiler)** i UTF-8 op til 256 KB, f.eks. Markdown, tekst, JSON, CSV, YAML, TOML, XML og SVG, indsættes i beskedens tekst op til grænsen på 100.000 tegn. Resten af en fil afkortes med angivelse af mængden eller nævnes kun ved navn.
- **CAD Files (CAD-filer)** som STEP, IGES, FreeCAD, Rhino, STL, 3MF, OBJ og glTF importeres straks i designet som egne komponenter. De knyttes først til beskeden efter importen, og **Send** venter, mens en spinner vises. Agenten får besked om en vellykket import. Fejl vises til brugeren uden at sende den fejlede fil til agenten. Fjernes filens vedhæftningsfelt, bliver komponenten stående i designet; Undo kan fjerne den.
- Alle filer nævnes ved navn i beskeden. Vedhæftede billeder vises som miniaturer under beskeden; hold markøren over for større preview, eller klik for at åbne billedviseren. Her vises filnavnet, og ‹ › skifter mellem beskedens billeder og eventuelt billedet af visningen. Andre filer listes ved navn. Billeder afkodes i baggrunden med spinner imens. En fil, som ikke kan vedhæftes, giver en forklaring.

Filer følger deres besked. En besked sat i kø, mens agenten arbejder, beholder sine vedhæftninger, og filer sendes igen efter login, hvis beskeden sendes igen. Flere købeskeder samles til én prompt med højst otte vedhæftede filer; resten nævnes kun i chatten. **New Chat** rydder også filerne i vedhæftningsfelterne.

## Skriv, mens agenten arbejder

Tekstfeltet kan bruges under en aktiv agentbesked. **Enter** sætter din besked i kø som en **Queued (i kø)**-boble, der kan **Edit (redigeres)** eller fjernes med ×, indtil den sendes. Flere beskeder i kø sendes samlet og i rækkefølge, når den aktuelle opgave slutter normalt.

- **Send Now (send nu)** på boblen eller **Ctrl+Enter** / **Cmd+Enter** i tekstfeltet stopper det aktive trin, venter på agentens bekræftelse og sender derefter køen som næste prompt med en kort note om afbrydelsen. Ændringer fra den afbrudte besked kan fortrydes som ét trin ligesom andre beskeder.
- **Stop** (firkanten ved tekstfeltet eller **Esc**) stopper agenten, men bevarer beskeder i kø. De afventer **Send**.
- **New Chat (ny chat)** i overskriften eller **Ctrl+Shift+L** / **Cmd+Shift+L** med fokus på chatten stopper agenten, rydder samtale, kø og vedhæftede filer og starter en ny session med samme agent og tilstand. Der spørges først, hvis noget er aktivt eller afventer. Designets Undo-historik ændres ikke.

## Agenter

**Preferences → AI → Agents** viser alle tilgængelige agenter med type (program eller endpoint), version eller model, mulig loginstatus, **Test**, **Default (standard)**, agentens egne tilladelser og **Remove** for agenter, du selv har tilføjet.

### Automatisk fundne agenter

SolidCraft søger efter agenter, der understøtter [Agent Client Protocol](https://agentclientprotocol.com), via `PATH` og de almindelige installationsmapper, som desktopprogrammers `PATH` ofte mangler: npm global, nvm, Volta, bun, `~/.local/bin`, `~/.grok/bin`, Homebrew og `%APPDATA%\npm`. For hver agent vises **version** fra `--version` med en tidsgrænse på tre sekunder og mulig **signed in (logget ind)**-status. Versionskontroller og agentsessioner bruger de samme mapper, så adaptere kan finde Node m.m., selv når SolidCraft åbnes fra Finder eller Dock. Eksisterende absolutte `PATH`-poster bevarer deres rækkefølge; shellens opstartsfiler køres ikke. SolidCraft kontrollerer kun, om agentens egen loginfil (`~/.claude/.credentials.json`, `~/.codex/auth.json`, `~/.gemini/oauth_creds.json` osv.) eller en relevant nøglevariabel findes. Indholdet læses ikke, og agenten startes ikke for at undersøge login. På macOS kan Claude Code bruge Keychain, så SolidCraft forsøger ikke at gætte status.

| Agent | Hvad der skal installeres |
|---|---|
| Claude Code | `npm install -g @agentclientprotocol/claude-agent-acp` (ACP-adapter til Claude Code) |
| Codex | `npm install -g @agentclientprotocol/codex-acp` |
| Google Antigravity | Googles ACP-server downloades separat fra `agy`, som ikke har ACP-tilstand. Hent ZIP-filen til dit system fra [ACP Registry](https://agentclientprotocol.com/registry), pak den ud, og tilføj `agy_acp_server.par` (`agy_acp_server.exe` på Windows) til `PATH`. Behold `localharness_external` ved siden af. Login foregår i browseren. |
| OpenCode | `npm install -g opencode-ai` (`opencode acp`) |
| XAI Grok | [Grok Build-installationsprogrammet](https://docs.x.ai/build/overview), derefter `grok login` (`grok agent stdio` understøtter ACP) |
| GitHub Copilot CLI, Qwen Code, goose | Vises med installationskommandoer, hvis de mangler |

Panelet viser ikke-installerede agenter med installationskommando og knappen **Copy (kopiér)**. **Refresh (opdatér)** søger igen efter installation. **Test** starter kortvarigt agenten, opretter en session og afslutter den igen.

### XAI Grok

Installér **Grok Build**, og kør `grok login` i terminalen for at logge ind. Vælg derefter **AI → Refresh**, **XAI Grok** og **Connect** i SolidCraft. Du kan gøre den til standardagent under **Preferences → AI → Agents**.

SolidCraft genbruger Grok Builds login fra `~/.grok/auth.json`. Programmet kontrollerer kun, om filen findes, og læser eller kopierer ikke tokens. Hvis login er nødvendigt, tilbyder panelet Groks egne loginmetoder. Du kan også køre `grok login` igen og forbinde på ny. Grok styrer selv kontoadgang til modeller og forbrugsgrænser. Ingen API-nøgle kræves i SolidCraft til denne forbindelse. Grok Build understøtter også `XAI_API_KEY` for brugere, der selv vælger API-godkendelse.

### Add Agent… (tilføj agent)

- **Installed Agent (installeret agent):** Opret din egen post for en installeret agent, f.eks. med andre argumenter.
- **Custom Command (brugerdefineret kommando):** Tilføj et andet program, der understøtter ACP på stdin og stdout, f.eks. `my-agent --acp`. SolidCraft starter programmet direkte og ikke gennem en shell.
- **OpenAI-compatible API (OpenAI-kompatibelt API):** Vælg en standardopsætning (Ollama `http://localhost:11434/v1`, LM Studio `http://localhost:1234/v1`, OpenAI) eller en brugerdefineret URL, et modelnavn og eventuelt en API-nøgle. Nøglen gemmes i systemets nøglelager (Keychain på macOS, Credential Manager på Windows eller Secret Service på Linux) og sendes kun via HTTPS eller til den lokale computer. **Test** henter endpointets modeller. For disse agenter styrer SolidCraft samtalen med samme værktøjer, grænser og godkendelser som ved kodeagenter og højst 24 modelkald pr. besked.

## Login

Hvis en kodeagent ikke er logget ind, vises beskeden **Sign in to the Agent (log ind på agenten)** med agentens egne muligheder:

- **En terminalkommando**, f.eks. `claude-agent-acp --cli auth login --claudeai`, med en **Copy (kopiér)**-knap. Kør kommandoen, log ind, og tryk **I've Signed In: Connect Again (jeg er logget ind: forbind igen)**.
- **En knap** til agenter, der selv håndterer login. Codex kan f.eks. åbne ChatGPT-login i browseren.
- **En miljøvariabel**, hvis agenten tager en nøgle fra miljøet. Panelet har med vilje intet felt til kodeagentens API-nøgle.

Det, du skrev før login, sendes, når agenten er klar.

## Hvad agenten kan gøre

Agenten får kun adgang til det åbne design gennem SolidCrafts egne værktøjer ([MCP-serveren](mcp.md)), som overdrages, når chatten starter. Serveren hedder `solidcraft-app`, så den erstattes ikke af en MCP-server, du selv har oprettet under navnet `solidcraft` (se [klientkonfiguration](mcp.md#client-configuration)):

- **Read the Design (læs designet):** Vis kommandoer, undersøg Features, skitser og parametre, mål geometri og tag billeder af visningen.
- **Change the Design (ændr designet):** Kør modelleringskommandoer enkeltvis eller som Batch. Ændringer vises i vinduet under udførelsen.
- **One Undo per Message (én fortrydelse pr. besked):** Alt, agenten ændrer under ét svar, samles i ét Undo-trin opkaldt efter beskeden. Chatten viser *N changes, one undo step* med **Undo These Changes**, så længe intet andet er foretaget siden.
- **Tool Cards (værktøjskort):** Hvert værktøjskald vises på et kort med status. Klik for at se svaret.
- **Formatted Text (formateret tekst):** Agentsvar vises som Markdown med overskrifter, lister, citater, kodeblokke med Copy, fed, kursiv, inline-kode og web-links. Links vises som knapper, der åbner i browseren. Brugerens beskeder og agentens tanker vises som almindelig tekst.
- **Attach View (vedhæft visning):** Send et billede af den aktuelle visning sammen med beskeden til en agent, der understøtter billeder. Billedet tages, når agenten starter på beskeden, og vises som miniature. Hold markøren over for større preview, eller klik for at åbne viseren med billeder fra beskeden, bladring med ‹ › eller piletasterne og lukning med Esc eller klik udenfor. Lange beskeder ruller inden i deres område, så Send og naboknapperne forbliver synlige.

## Tilladelser

Under **Preferences → AI → Permissions** kan du angive, hvilke handlinger agenten må udføre uden at spørge. Hver agent kan have sin egen indstilling.

- **Ask Every Time (spørg hver gang):** Alle designændringer og filhandlinger kræver et valg mellem **Allow**, **Always Allow** og **Deny**.
- **Auto-allow Model Edits (tillad modelændringer automatisk)** er standard. Modelleringskommandoer udføres straks, men lagring, åbning, eksport, import, oprettelse af nyt design, kommandoer med filnavne og Undo tilbage før beskedens start kræver godkendelse.
- **Allow Everything (tillad alt):** Intet kræver godkendelse. Siden advarer om, at agenten kan gemme, overskrive og erstatte dit design.

**Always Allow (tillad altid)** husker godkendelsen af den pågældende kommando for agenten. Under indstillingerne vises listen over sådanne godkendelser pr. agent med mulighed for **Reset (nulstil)**.

Indstillingerne gælder SolidCrafts egne værktøjer. En kodeagents egne værktøjer som shell og filredigering styres af agentens tilstand, der vælges i panelets overskrift. Hvis agenten spørger om lov til et SolidCraft-værktøj, svarer SolidCraft på brugerens vegne, fordi programmet selv kontrollerer tilladelsen, og viser **Auto-allowed** i chatten. Ved Allow Everything eller efter Always Allow besvares agentens øvrige spørgsmål også automatisk og noteres.

En kommando regnes som en kommando med filnavn, når en parameter indeholder en fil eller mappe (`file`, `dir`, `output` osv.), når `path` eller `source` ligner en filsti (separator, `~`, indledende punktum eller kendt filendelse), eller når en sådan værdi bruges til eksport, import, gemning, åbning eller indsættelse. `path` for Sweep og Pattern (kurve-ID'er) eller en forekomst (ID'er) regnes derimod som geometri og kræver ikke en ekstra godkendelse.

## Hvad agenten ikke kan gøre

- **Filer kræver din godkendelse**, medmindre Allow Everything er valgt. **Cancel** eller **Stop** afviser afventende anmodninger; ubesvarede spørgsmål afvises efter fire minutter. [MCP-serverens mappebegrænsninger](mcp.md) gælder stadig efter godkendelse: designmappen, Recovery-mappen og serverens Scratch-mappe.
- **Grænse pr. besked:** Som standard må agenten udføre 24 ændringer pr. besked. Antallet kan ændres under AI-indstillingerne. Læsning tæller ikke. Ved grænsen får agenten besked om at stoppe og rapportere det resterende arbejde. Send en ny besked for at fortsætte.
- **Ingen nedbrudsrapporter:** Uanset tilladelser har agenten ikke adgang til Crash Reports, som kan indeholde andre design og filstier.
- **Ikke resten af appen:** Agenten kan ikke klikke i brugerfladen, ændre indstillinger, afslutte appen eller styre selve chatten.
- **Egne værktøjer deaktiveres, hvor det er muligt:** SolidCraft giver ikke agenten filsystem eller terminal, starter den i en tom arbejdsmappe og beder Claude Code-adapteren om at deaktivere shell-, fil- og webværktøjer for sessionen.

Når agenten stoppes, en anden agent vælges, eller SolidCraft afsluttes, stoppes både agenten og de programmer, den startede, herunder CLI og MCP-server.

## Privatliv

Intet sendes andre steder end til den agent, du kører, eller det endpoint, du selv har tilføjet. SolidCraft starter en kodeagent som et program på din computer og kommunikerer over standardind- og uddata. Hvad agenten sender videre til sin tjeneste under din konto, følger den pågældende tjenestes vilkår — ligesom når du bruger agenten i en terminal. SolidCraft ser, gemmer eller videresender aldrig kodeagentens adgangskode, token eller API-nøgle. Et endpoints nøgle gemmes i operativsystemets sikre nøglelager og sendes kun til det endpoint. SolidCrafts egen forbindelse til agentens værktøjer bruger et tilfældigt token pr. opstart i en fil, som kun du kan læse. Tokenet sendes ikke gennem agenten.

## Afprøv uden konto

I kildekoden findes en scriptstyret testagent, der opretter en blok, afrunder den og måler den uden at bruge en AI-model:

```sh
cargo build --release -p solidcraft-ui-egui --example acp_test_agent
```

Tilføj den under **Preferences → AI → Add Agent… → Custom Command** med stien `target/release/examples/acp_test_agent`. Skriv "save" i beskeden for at få vist et Allow/Deny-kort eller "ask" for at se agenten stille sit eget spørgsmål.

## Ordliste – engelsk til dansk

| Engelsk udtryk | Dansk oversættelse |
|---|---|
| Agent Chat | Chat med AI-agent |
| Coding Agent | Kodeagent |
| Agent Client Protocol (ACP) | Protokol til kommunikation med agenter |
| OpenAI-compatible Endpoint | OpenAI-kompatibelt API-endpoint |
| Model | Model |
| Connect | Forbind |
| Show AI Features | Vis AI-funktioner |
| Show Button in Toolbar | Vis knap på værktøjslinjen |
| Preferences | Indstillinger |
| Dock | Fastgør panelet |
| Own Window | Eget vindue |
| Pop-out | Åbn i separat vindue |
| Minimise | Minimér |
| Close | Luk |
| Agent Switcher | Agentskifter |
| Default | Standard |
| Accept Edits | Tillad redigeringer |
| Plan | Planlægningstilstand |
| Send | Send |
| Choose a File | Vælg fil |
| Paste a Picture | Indsæt billede |
| Attach View | Vedhæft visningen |
| Thumbnail | Miniaturebillede |
| Spinner | Ventemarkør |
| Queued | Sat i kø |
| Edit | Rediger |
| Send Now | Send nu |
| Stop | Stop |
| New Chat | Ny chat |
| Refresh | Opdatér |
| Test | Test |
| Add Agent | Tilføj agent |
| Installed Agent | Installeret agent |
| Custom Command | Brugerdefineret kommando |
| Sign In | Log ind |
| Permissions | Tilladelser |
| Ask Every Time | Spørg hver gang |
| Allow | Tillad |
| Always Allow | Tillad altid |
| Deny | Afvis |
| Auto-allow Model Edits | Tillad modelændringer automatisk |
| Allow Everything | Tillad alt |
| Reset | Nulstil |
| Read the Design | Læs designet |
| Change the Design | Ændr designet |
| One Undo per Message | Ét Undo-trin pr. besked |
| Tool Card | Værktøjskort |
| Markdown | Tekstformat med simpel formatering |
| Privacy | Privatliv |
| Keychain | Sikkert nøglelager |
| API Key | API-nøgle |
| Environment Variable | Miljøvariabel |
| Crash Report | Nedbrudsrapport |
| Scratch Folder | Midlertidig arbejdsmappe |
| Recovery Folder | Gendannelsesmappe |

### Tekniske navne

| Navn | Dansk betydning |
|---|---|
| `PATH` | Systemets søgesti til programmer |
| `--version` | Vis programversion |
| `stdin` | Standardinddata |
| `stdout` | Standarduddata |
| `/models` | API-rute, der viser modeller |
| `XAI_API_KEY` | API-nøgle til xAI |
| `~/.codex/auth.json` | Loginfil til Codex |
| `~/.grok/auth.json` | Loginfil til Grok Build |
| `Ctrl+Shift+A` | Åbn AI-panel |
| `Ctrl+Shift+L` | Start ny chat |
