# SolidCraft

SolidCraft er et open source-program til parametrisk 3D-CAD, skrevet udelukkende i Rust.

Opret en **Sketch (skitse)** på et plan, tilføj **Constraints (geometriske bindinger)** og **Dimensions (mål)**, og omdan profiler til solide emner ved hjælp af **Features (modelleringsfunktioner)** på en **Timeline (tidslinje)**. Ændr en parameter, og se hele tidslinjen blive genberegnet.

![Eksempelpladen i SolidCraft](generated/sample-plate.png)

SolidCraft kan bruges som:

- Et **skrivebordsprogram** til Windows og Linux (egui + wgpu).
- En **webapp** i browseren (wasm, WebGPU eller WebGL2). [Prøv den her](app/).
- Et **kommandolinjeværktøj (CLI)** uden grafisk brugerflade, `solidcraft-cli`, som kan udføre kommandoscripts, foretage målinger og eksportere filer.
- En **MCP-server**, der gør det muligt for AI-agenter at oprette og redigere modeller, enten uden grafisk brugerflade eller ved at styre det kørende program.

Alt, hvad brugeren kan gøre, er en *Command (kommando)* med parametre i JSON-format. Værktøjslinjen, dialogboksene, scripts, kommandolinjeværktøjet, kontrolkanalen og MCP benytter alle de samme kommandoer. Det betyder, at alt, hvad du kan udføre med musen, også kan automatiseres med scripts, testes eller overlades til en AI-agent.

SolidCraft er stadig på et tidligt udviklingsstadie. **Sketching (skitsering)**, **Solid Modeling (solidmodellering)**, parametre, **Components (komponenter)** med **Joints (ledforbindelser)**, **Sheet Metal (pladekonstruktion)** og en delmængde af funktionerne til plastkabinetter fungerer allerede. **Surface Modeling (overflademodellering)**, **Drawings (tekniske tegninger)**, **CAM (computerstøttet fremstilling)** og **Simulation (simulering)** er udskudt til senere udvikling.

Du kan læse mere i [udviklingsplanen](https://github.com/bherbruck/solidcraft/blob/main/ROADMAP.md). [Funktionsoversigten](dev/features.md) viser alle kommandoer opdelt efter område, mens [referencetestene](dev/oracle.md) kontrollerer modelleringsresultater ved at sammenligne dem med analytiske beregninger og værdier fra FreeCAD.

Eksisterende `.solidcraft`-designfiler og kommandoer i `solidcraft-cli` bevarer deres navne og funktionalitet.

Denne dokumentation er opdelt i to hoveddele: **User Guide (brugervejledning)** til dem, der arbejder med modeller i SolidCraft, og **Developer Guide (udviklervejledning)** til dem, der arbejder på selve programmet. [Command Reference (kommandoreference)](reference/commands.md) genereres automatisk fra kildekoden ved hver build.

SolidCraft er licenseret under MIT eller Apache-2.0. Programmet er udviklet uafhængigt, og dets funktionalitet er baseret på offentligt tilgængelig dokumentation, almindelige CAD-principper og feedback fra brugerne. Referencetestene benytter analytiske beregninger og open source-værktøjer.

SolidCraft har ingen tilknytning til Autodesk, Dassault Systèmes, PTC eller andre CAD-producenter. Produktnavne som Fusion, SolidWorks og Onshape er varemærker tilhørende deres respektive ejere og nævnes udelukkende for at beskrive kompatibilitet.
