# Updates and Privacy (opdateringer og privatliv)

SolidCraft giver besked, når en nyere version er udgivet. Programmet opdaterer aldrig sig selv uden at spørge og sender ingen oplysninger om dig eller dine design under opdateringskontrollen.

## Sådan vises opdateringer

Når en nyere stabil version findes, vises en lille farvet knap med teksten **Update to 0.14.0** (versionsnummeret varierer) på titellinjen ved siden af **Report a Bug**. Intet popper op automatisk. Klik på knappen for at se versionen, udgivelsesnoter og tre muligheder. I smallere vinduer forkortes teksten til **Update** og senere til kun ikonet for at give plads til dokumentfanerne.

- **Download (hent):** Åbner installationsfilen, der passer til dit system, i browseren (se tabellen nedenfor). Hvis ingen fil passer, åbnes udgivelsessiden.
- **Skip This Version (spring denne version over):** Skjuler opdateringen, indtil der udgives en endnu nyere version.
- **Remind Me Later (påmind mig senere):** Skjuler opdateringen i tre dage.

SolidCraft kontrollerer ved hver opstart, så nye udgivelser opdages med det samme, og derefter en gang i døgnet, så længe programmet er åbent.

Under **Preferences ▸ General ▸ Updates** findes **Check for Updates Automatically (søg automatisk efter opdateringer)**, som er aktiv som standard, samt **Check Now (kontrollér nu)**. Den sidste udfører kontrollen med det samme og åbner vinduet, også hvis du tidligere har sprunget versionen over.

## Opdatering med ét klik

Når SolidCraft sikkert kan erstatte den installerede version, findes også knappen **Update Now (opdater nu)**:

| Installeret som | Hvad Update Now gør |
|---|---|
| AppImage | Henter den nye AppImage ved siden af den gamle, verificerer den, erstatter den gamle fil og tilbyder **Restart Now (genstart nu)** |
| Windows-installationsprogram (`.msi`) | **Install (Closes SolidCraft)** henter og verificerer `.msi`, lukker SolidCraft efter at have spurgt om ugemte design og starter installationen |
| Bærbar Windows-version (`.zip` eller `.exe`) | Henter og verificerer pakken, udskifter `solidcraft.exe` og eventuelt `solidcraft-cli.exe`, tilbyder **Restart Now** og fjerner den gamle fil ved næste start |
| macOS | **Download and Open** henter og verificerer `.dmg` og åbner den; træk SolidCraft til Applications for at erstatte den gamle app |
| Flatpak, Snap, `.deb`, `.rpm`, `.tar.gz` | Ingen direkte opdatering; pakkehåndteringen opdaterer programmet (kommandoen vises), eller arkivet pakkes ud manuelt |

Hver download verificeres før ændringer. SHA-256-kontrolsummen skal matche `SHA256SUMS.txt` fra samme udgivelse hentet via HTTPS. Når projektet begynder at signere udgivelser, skal listen også have en gyldig signatur fra SolidCrafts udgivelsesnøgle, som navngiver udgivelsen (se [releasing](https://github.com/bherbruck/solidcraft/blob/main/docs/releasing.md#update-signing)). Fejler verificeringen, slettes den hentede fil, og installationen bevares. **Download** kan stadig hente filen til manuel installation. På macOS åbnes diskaftrykket frem for automatisk udskiftning, fordi appen er ad hoc-signeret og ikke notariseret; baggrundsudskiftning ville installere kode, som macOS ikke kan verificere på normal vis.

Update Now kræver skriverettigheder til installationsfilen. Ligger en AppImage i en skrivebeskyttet mappe, vises en fejl, og du må bruge **Download** i stedet.

## Opdatering fra 0.11 til 0.14

SolidCraft 0.11.0–0.14.0 accepterer kun installationspakker fra projektets tidligere GitHub-adresse. Efter flytningen til `github.com/tangency-labs/solidcraft` kan disse versioner se, at en nyere udgivelse findes, men ikke installere den via **Update Now**. **Download** åbner i stedet udgivelsessiden.

Hent selv version 0.15.0 eller nyere én gang fra udgivelsessiden eller [downloadlinkene](https://github.com/tangency-labs/solidcraft#download), og installér den oven på den gamle. Fra 0.15.0 accepterer SolidCraft begge adresser, så **Update Now** virker igen. Pakkehåndteringer som Flatpak, Snap, WinGet, `.deb` og `.rpm` er ikke påvirket.

## Hvilke oplysninger sendes?

Ved opstart og derefter en gang i døgnet, mens programmet er åbent, sendes én HTTPS-forespørgsel:

```text
GET https://api.github.com/repos/bherbruck/solidcraft/releases/latest
User-Agent: SolidCraft-update-check
Accept: application/vnd.github+json
X-GitHub-Api-Version: 2022-11-28
```

**Update Now** henter derefter `SHA256SUMS.txt` (og en eventuel signatur) samt den relevante installationspakke fra udgivelsens egne links under `github.com/bherbruck/solidcraft/releases/download/…` med samme User-Agent. Det er alt: Ingen versionsnumre, computer- eller brugeridentifikatorer, brugsdata eller cookies sendes. GitHub registrerer dog besøget som andre API-kald med bl.a. IP-adresse og tidspunkt; se [GitHubs privatlivspolitik](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement). SolidCraft sammenligner svaret med den installerede version lokalt.

- **Offline eller intet svar fra GitHub inden for 15 sekunder:** Intet vises, og kontrollen gentages en time senere eller ved næste opstart, alt efter hvad der kommer først.
- **GitHubs begrænsning for anonyme forespørgsler:** Maksimalt 60 pr. time pr. adresse respekteres. Hvis GitHub beder klienten vente, ventes der også ved Check Now og opstartskontrol.
- **Check for Updates Automatically slået fra:** Kun Check Now sender en forespørgsel.
- **`SOLIDCRAFT_UPDATE_CHECK=0`:** Deaktiverer alle kontroller, f.eks. på administrerede computere.

Selvkompilerede builds, Flatpak og browserappen (som opdaterer sig selv) udfører ikke denne opdateringskontrol.

## Hvilken fil hentes?

Udgivelserne indeholder installationsfiler med faste navne (se [releasing](https://github.com/bherbruck/solidcraft/blob/main/docs/releasing.md)). SolidCraft vælger den fil, der passer til installationstypen:

| Installeret som | Genkendes ved | Download | Sådan opdateres |
|---|---|---|---|
| AppImage | `APPIMAGE` er angivet | `solidcraft-linux-<arch>.AppImage` | Erstat den gamle AppImage med den nye |
| Flatpak | `FLATPAK_ID` er angivet | Kontrollerer ikke (se nedenfor) | `flatpak update ai.storyteller.solidcraft` |
| Snap fra Snap Store | `SNAP` er angivet | Udgivelsesside | `sudo snap refresh solidcraft` (snapd opdaterer også selv) |
| Snap fra downloadet fil | `SNAP_REVISION` begynder med `x` | `solidcraft-linux-<arch>.snap` | `sudo snap install --dangerous ./solidcraft-linux-<arch>.snap` |
| `.deb` | Under `/usr`, dpkg kender `solidcraft` | `solidcraft-linux-<arch>.deb` | `sudo apt install ./solidcraft-linux-<arch>.deb` |
| `.rpm` | Under `/usr`, en rpm-database findes | `solidcraft-linux-<arch>.rpm` | `sudo dnf install ./solidcraft-linux-<arch>.rpm` |
| `.tar.gz` | Andre placeringer på Linux | `solidcraft-<version>-linux-<arch>.tar.gz` | Pak ud over den gamle kopi |
| Windows-installer | Under Program Files eller i registreret installationsmappe | `solidcraft-windows-<arch>.msi` | Kør installationsprogrammet |
| Windows Portable | `solidcraft-cli.exe` ligger ved siden af | `solidcraft-windows-<arch>-portable.zip` | Pak ud over den gamle kopi |
| Windows `.exe` | Andre placeringer på Windows | `SolidCraft-<version>-windows-x64.exe` | Erstat den gamle `.exe` |
| macOS | Inde i `SolidCraft.app` | `solidcraft-macos-universal.dmg` | Træk den nye app til Applications |

Vinduet viser opdateringskommandoen for Snap, `.deb` og `.rpm` med en Copy-knap. Flatpak har ikke netværkstilladelse til denne kontrol; Flatpak selv eller softwarecentret tilbyder opdateringer. Udgivelser sammenlignes med [semver](https://semver.org): Et nightly build regnes som den udgivelse, det bygger på, og får derfor kun tilbudt en nyere stabil version. Release Candidates tilbydes den endelige udgivelse.

## Ordliste – engelsk til dansk

| Engelsk udtryk | Dansk oversættelse |
|---|---|
| Updates | Opdateringer |
| Privacy | Privatliv |
| Release | Udgivelse |
| Stable Release | Stabil udgivelse |
| Release Notes | Udgivelsesnoter |
| Update Now | Opdater nu |
| Download | Hent |
| Skip This Version | Spring denne version over |
| Remind Me Later | Påmind mig senere |
| Check Now | Kontrollér nu |
| Check for Updates Automatically | Søg automatisk efter opdateringer |
| Restart Now | Genstart nu |
| Portable | Bærbar version |
| Installer | Installationsprogram |
| Package Manager | Pakkehåndtering |
| SHA-256 Checksum | SHA-256-kontrolsum |
| Signature | Digital signatur |
| Verification | Verificering |
| HTTPS Request | HTTPS-forespørgsel |
| User-Agent | Klientidentifikation |
| Rate Limit | Begrænsning af antal forespørgsler |
| Nightly Build | Natlig udviklingsversion |
| Release Candidate | Udgivelseskandidat |
| Semver | Semantisk versionsnummerering |
| Flatpak | Flatpak-pakke |
| Snap | Snap-pakke |
| AppImage | AppImage-programfil |
| Disk Image | Diskaftryk |
| Environment Variable | Miljøvariabel |
| `SOLIDCRAFT_UPDATE_CHECK=0` | Slå alle opdateringskontroller fra |
| `SHA256SUMS.txt` | Fil med kontrolsummer |
