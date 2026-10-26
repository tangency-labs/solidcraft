# Troubleshooting: SolidCraft Won't Start (fejlfinding ved opstart)

## 0.10.0 lukker få sekunder efter opstart (Windows)

SolidCraft 0.10.0 kan gå ned under opstart på Windows-computere, der bruger Direct3D 12 og systemets shader-compiler. Det gælder bl.a. ældre Intel-grafikkort, computere uden installeret grafikdriver ("Microsoft Basic Display Adapter"), virtuelle maskiner og Remote Desktop-sessioner. **Installér 0.10.1 eller nyere**, hvor fejlen er rettet. Versioner før 0.10.0 har ikke dette problem.

## Start i fejlsikret tilstand

Safe Mode (fejlsikret tilstand) starter med standardindstillinger uden at ændre dine egne gemte indstillinger til næste normale opstart. Den springer spørgsmålet "Recover unsaved design?" over og bruger software-rendering.

- **Windows:** Kør `solidcraft.exe --safe-mode` fra en terminal i installationsmappen, eller tilføj ` --safe-mode` til slutningen af en genvejs **Target (destination)**.
- **macOS:** `open -a SolidCraft --args --safe-mode`
- **Linux:** `solidcraft --safe-mode` (Flatpak: `flatpak run ai.storyteller.solidcraft --safe-mode`)

Miljøvariablen `SOLIDCRAFT_SAFE_MODE=1` gør det samme. Fra version 0.10.2 starter SolidCraft selv i Safe Mode ved tredje forsøg, hvis to opstarter i træk stopper, før vinduet vises. `SOLIDCRAFT_RENDERER=cpu` bruger udelukkende software-rendering og bevarer dine indstillinger.

## Nulstil indstillingerne

SolidCraft gemmer indstillinger og data i mapperne nedenfor. Alle installerede versioner deler dem. Programmet ændrer ikke systemindstillinger, drivere eller registreringsdatabase uden for disse mapper. Afslut SolidCraft, og **omdøb mapperne i stedet for at slette dem**. Ved næste opstart opretter programmet nye mapper med standardindstillinger. Omdøb dem tilbage for at gendanne indstillingerne, eller vedlæg dem i en fejlrapport.

| System | Indstillinger (`app.ron`, `graphics.json`) | Data (`recovery`, `crashes`, `control`, miniaturer) |
|---|---|---|
| Windows | `%APPDATA%\SolidCraft` | `%LOCALAPPDATA%\SolidCraft` |
| macOS | `~/Library/Application Support/SolidCraft` | (samme mappe) |
| Linux | `~/.local/share/solidcraft` | (samme mappe); miniaturer i `$XDG_CACHE_HOME/SolidCraft` eller `/tmp/SolidCraft` |
| Linux (Flatpak) | `~/.var/app/ai.storyteller.solidcraft/data/solidcraft` | (samme mappe) |

I Windows kan du indsætte `%APPDATA%` eller `%LOCALAPPDATA%` i Stifinders adresselinje. Før navneskiftet brugte ældre versioner mapperne `SolveCraft` ved siden af disse. SolidCraft læser dem kun, så de kan blive liggende.

- Mappen **`recovery`** indeholder automatisk gemte design, som aldrig blev gemt manuelt. Omdøb datamappen frem for at slette den, så du stadig kan åbne de automatisk gemte `.solidcraft`-filer via File > Open.
- Mappen **`crashes`** indeholder nedbrudsrapporter. Vedlæg gerne den nyeste i en fejlrapport.

Fra version 0.10.2 kontrollerer SolidCraft også indstillingsfilerne ved opstart. En fil, programmet ikke kan læse, omdøbes til `app.ron.bad-<time>` eller `graphics.json.bad-<time>`, og programmet starter med standardindstillinger.

## "No graphics adapter" eller intet sker

SolidCraft kræver Direct3D 12 eller Vulkan på Windows, Metal på macOS og Vulkan eller OpenGL på Linux. Uden grafikdriver forsøger det at bruge systemets software-renderer (WARP på Windows, llvmpipe eller lavapipe på Linux). Det er langsommere, men fungerer. Fra 0.10.2 forsøger programmet software-rendering, hvis et vindue ellers ikke kan åbnes, og viser en fejlmeddelelse, hvis det stadig mislykkes. Ældre versioner kunne lukke uden forklaring. Prøv følgende:

- Installér grafikdriveren direkte fra GPU-producenten (Intel, AMD eller NVIDIA) i stedet for Windows Update.
- Slå 3D-acceleration til i en virtuel maskine. Ved Remote Desktop kan du prøve at køre programmet direkte på computeren.
- Kør `solidcraft --crash-log` for at se de seneste nedbrudsrapporter, og vedlæg den nyeste i en issue.

## Skrivebordet hakker eller fryser, mens SolidCraft opbygger emner

Nogle grafikdrivere kan fryse under den shader, SolidCraft bruger til at opbygge emner på GPU'en, hvorefter systemet nulstiller GPU'en. SolidCraft bruger allerede CPU'en til dette på AMD FirePro D300/D500/D700 (Mac Pro 2013). Hvis et andet grafikkort giver problemet, skal du starte SolidCraft med miljøvariablen `SOLIDCRAFT_CPU_TESS=1`, så emner bygges på CPU'en, og oprette en issue med grafikkortets navn.

## SolidCraft lukker straks på Windows

En grafikdriver kan få programmet til at gå ned under opstart, før SolidCraft kan vise noget. En ældre Intel Vulkan-driver (`igvk64.dll`) har eksempelvis gjort dette. I Windows Event Viewer (Hændelsesfremviser) kan du finde driveren under **Windows Logs › Application** ved "Faulting module name".

Fra 0.11.1 forsøger SolidCraft først Direct3D 12 på Windows og indlæser ikke andre grafik-API'er, hvis Direct3D 12 har et grafikkort. Kun hvis dette ikke findes, forsøger programmet Vulkan og derefter OpenGL. Ældre versioner startede alle tre og foretrak Vulkan. Hvis du selv vil vælge grafik-API, kan du åbne Kommandoprompt og starte SolidCraft:

```
set WGPU_BACKEND=dx12
"C:\Program Files\SolidCraft\solidcraft.exe"
```

`vulkan` og `gl` er de øvrige muligheder. `setx WGPU_BACKEND dx12` gør indstillingen permanent for programmer startet fra Startmenuen efter næste login. Fra 0.11.1 kan indstillingen også stå i `graphics.json` i indstillingsmappen: `{"msaa": 4, "backend": "dx12"}` (`auto` bruger standardrækkefølgen). Safe Mode ignorerer dette og bruger altid software-rendering.

## Et design gemt med 0.10 kan ikke åbnes i en ældre version

Design gemt i en nyere SolidCraft-version kan bruge et nyere filformat. Ældre versioner afviser filen med beskeden "this design was saved by a newer SolidCraft" og ændrer ikke filen. Åbn den med den nyeste version.

## Ordliste – engelske udtryk på dansk

| Engelsk udtryk | Dansk oversættelse |
|---|---|
| Troubleshooting | Fejlfinding |
| Safe Mode | Fejlsikret tilstand |
| Renderer | Renderingsmotor |
| Software Renderer | Software-rendering |
| Graphics Driver | Grafikdriver |
| Graphics Adapter | Grafikkort / grafikadapter |
| Shader Compiler | Shader-compiler |
| Crash | Nedbrud |
| Crash Report | Nedbrudsrapport |
| Recovery | Gendannelse |
| Autosave | Automatisk lagring |
| Preferences | Indstillinger |
| Reset | Nulstil |
| Target | Destination for genvej |
| Event Viewer | Hændelsesfremviser |
| Faulting Module | Fejlbehæftet modul |
| Virtual Machine | Virtuel maskine |
| Remote Desktop | Fjernskrivebord |
| CPU | Processor |
| GPU | Grafikprocessor |
| Backend | Grafiksystem / backend |
| OpenGL | OpenGL-grafik-API |
| Vulkan | Vulkan-grafik-API |
| Direct3D 12 | Direct3D 12-grafik-API |
| Metal | Apples Metal-grafik-API |
| Issue | Fejlrapport på GitHub |

### Kommandolinje og miljøvariabler

| Kommando / variabel | Dansk betydning |
|---|---|
| `--safe-mode` | Start i fejlsikret tilstand |
| `SOLIDCRAFT_SAFE_MODE=1` | Aktiver fejlsikret tilstand |
| `SOLIDCRAFT_RENDERER=cpu` | Brug CPU-baseret rendering |
| `SOLIDCRAFT_CPU_TESS=1` | Byg emnegeometri på CPU |
| `solidcraft --crash-log` | Vis tidligere nedbrud |
| `WGPU_BACKEND` | Vælg grafik-backend |
| `dx12` | Direct3D 12 |
| `vulkan` | Vulkan |
| `gl` | OpenGL |
| `auto` | Automatisk valg |
