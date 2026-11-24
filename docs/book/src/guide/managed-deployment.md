# Managing SolidCraft in an organization

An administrator can set some SolidCraft settings for everyone who uses a computer: with Group
Policy or Microsoft Intune on Windows, a configuration profile or MDM on macOS, and a file in
`/etc` on Linux and FreeBSD. A setting under policy shows in Preferences as **Set by your
organization**, and people cannot change it. While any policy is set, SolidCraft says it is
**Managed by** the organization.

A computer without a policy behaves exactly as before. SolidCraft only reads the policy: it
writes nothing, and it sends nothing about the policy anywhere.

## Install

The Windows installer (`solidcraft-windows-x64.msi`, also arm64 and x86) installs for all users.
`msiexec /i solidcraft-windows-x64.msi /qn` installs it without questions, so a deployment tool
can run it.

## The policies

| Policy | Registry value | Type | Values |
|---|---|---|---|
| Check for updates | `CheckForUpdates` | `REG_DWORD` | `1`: checks at start and once a day, and people cannot turn them off. `0`: no checks at all, as with `SOLIDCRAFT_UPDATE_CHECK=0` ([Updates and privacy](updates.md)). |
| Anonymous usage statistics | `UsageStatistics` | `REG_DWORD` | `0`: SolidCraft sends no usage statistics, keeps none to send later and shows no notice about them, and people cannot turn them on. `1`: each person chooses. SolidCraft 0.46 or later; what is sent is in [Privacy](https://github.com/tangency-labs/solidcraft/blob/main/docs/privacy.md). |
| Organization name | `OrganizationName` | `REG_SZ` | The name shown as **Managed by** and the name, up to 64 characters on one line. It changes no setting. |
| Units of new designs | `DefaultUnits` | `REG_SZ` | `mm`, `cm`, `m`, `in` or `ft` (Preferences, Design). Designs that already exist keep their units. SolidCraft 0.46 or later. |
| Interface font | `InterfaceFont` | `REG_SZ` | the name of a font family installed on the computer, such as `Segoe UI`: the interface font, which people cannot change or replace with a font file; a family the computer does not have leaves the built-in font showing, and a file path counts as not configured. SolidCraft 0.48 or later. |
| Interface language | `Language` | `REG_SZ` | `system` to follow the operating system, or the code of a language SolidCraft has, such as `en`, `de`, `fr`, `pt-BR` or `zh-TW` (Preferences, General, Language lists them). SolidCraft 0.46 or later. |
| Reopen designs at start | `ReopenDesigns` | `REG_DWORD` | `1`: reopens the designs that were open when SolidCraft closed. `0`: does not, for example on shared computers. SolidCraft 0.46 or later. |
| Show the Discord button | `ShowDiscordButton` | `REG_DWORD` | `1`: the application bar shows it and people cannot hide it. `0`: hidden. The Help menu keeps the link. SolidCraft 0.46 or later. |
| Show the Donate button | `ShowDonateButton` | `REG_DWORD` | `1`: the application bar shows it and people cannot hide it. `0`: hidden. The Help menu keeps the link. SolidCraft 0.46 or later. |
| Show AI features | `AiFeatures` | `REG_DWORD` | `1`: the Assistant button, shortcut, menu entry and chat panel show. `0`: no AI features, and SolidCraft looks for or starts no AI agent. SolidCraft 0.46 or later. |
| Show the Assistant button | `AiToolbarButton` | `REG_DWORD` | `1`: the application bar shows it and people cannot hide it. `0`: hidden; the shortcut and View menu stay. SolidCraft 0.46 or later. |
| AI agent permissions | `AiPermissions` | `REG_SZ` | `ask` (every change asks first), `edits` (modelling runs, files and destructive actions ask) or `all` (nothing asks). An agent's own permissions and the **Always allowed** answers don't apply while it is set. SolidCraft 0.46 or later. |
| Changes per message | `AgentTurnLimit` | `REG_DWORD` | `1` to `200`: the most changes an agent in the chat makes for one message. `0`: no limit. SolidCraft 0.46 or later. |
| Let AI agents connect | `AgentControl` | `REG_DWORD` | `1`: the control channel (MCP, this computer only) is on. `0`: SolidCraft opens no port for agents, not even one named with `--control` or `SOLIDCRAFT_CONTROL_PORT`. SolidCraft 0.46 or later. |
| Add SolidCraft to AI apps | `ConnectAiApps` | `REG_DWORD` | `1`: SolidCraft adds its MCP server to the AI apps on the computer at start. `0`: it adds none and takes out the entries it added. SolidCraft 0.46 or later. |
| Show labels on the toolbar | `RibbonLabels` | `REG_DWORD` | `1`: the toolbar shows a label under each button. `0`: icons only (Preferences, Layout, Window, Toolbar). SolidCraft 0.48 or later. |
| Show the status bar | `StatusBar` | `REG_DWORD` | `1`: the status bar shows and people cannot hide it. `0`: hidden (Preferences, Layout, Window, Status bar). SolidCraft 0.48 or later. |
| Workspace layout | `Layout` | `REG_SZ` | `floating` (panels float over the model) or `compact` (panels docked around it): the workspace layout, which people cannot change (Preferences, Layout, Window, Panel style). SolidCraft 0.48 or later. |
| Export hidden edges as dashed lines | `DxfDashedHiddenLines` | `REG_DWORD` | `1`: File, Export Current View as DXF writes hidden edges as dashed lines. `0`: it leaves them out. The export dialog shows the choice as set by the organization. SolidCraft 0.48 or later. |
| Zero point of DXF view exports | `DxfOrigin` | `REG_SZ` | `target` (the point the camera looks at) or `origin` (the model's origin): where File, Export Current View as DXF puts the drawing's 0,0. SolidCraft 0.48 or later. |
| Mesh export format | `MeshExportFormat` | `REG_SZ` | `stl`, `3mf` or `3mf-tagged` (3MF with face tags): the format File, Save As Mesh and Send to Slicer write; Export STL and Export 3MF open the dialog with it too. SolidCraft 0.48 or later. |
| Mesh export quality | `MeshExportQuality` | `REG_SZ` | `standard`, `fine` or `finest`: how closely a saved mesh follows curved faces. SolidCraft 0.48 or later. |
| Slicer | `Slicer` | `REG_SZ` | `prusa`, `bambu`, `orca`, `cura`, `superslicer`, `creality`, `elegoo`, `anycubic`, `ideamaker`, `lychee`, `qidi`, `chitubox`, `creatware`, `slic3r` or `easyprint` (EasyPrint, in the browser): the slicer File, Send to Slicer uses, which people cannot change; when it is not installed, Send to Slicer says no slicer was found. Open in EasyPrint uses it too. SolidCraft 0.48 or later. |
| Theme | `Theme` | `REG_SZ` | `dark`, `light` or the id of a built-in theme, such as `nord`, `high-contrast` or `solarized-light` (the file names in [`crates/look/themes`](https://github.com/tangency-labs/solidcraft/tree/main/crates/look/themes)): the interface theme, which people cannot change, import over or follow from a file; it does not switch between day and night. SolidCraft 0.48 or later. |

On Windows the values live in the key `Software\Policies\SolidCraft`:

- under `HKEY_LOCAL_MACHINE` for everyone on the computer, or
- under `HKEY_CURRENT_USER` for one person.

When both hold a value, the computer's value wins. A value of another type, a number or text
other than the ones listed, or an empty name counts as not set. Letter case and spaces around a
text value do not matter.

SolidCraft reads the policy when it starts. A change applies the next time SolidCraft starts.
SolidCraft keeps each person's own choice for a setting under policy, and saves it: when the
policy is removed, that choice is back.

The same value names under `Software\Policies\SolidCraft\Recommended` are reserved for defaults
that people may change. SolidCraft reads them and does not apply them yet.

## The templates

Each release has `solidcraft-<version>-windows-policy-templates.zip`, listed in
`SHA256SUMS.txt`. It holds the administrative template, `solidcraft.admx`, and its English text,
`en-US/solidcraft.adml` (they are also in `packaging/windows/policies/` in the source). Take the
zip from the release you deploy: it holds the policies that version reads. The policies appear in
the **SolidCraft** folder.

**Group Policy.** Copy `solidcraft.admx` into `PolicyDefinitions` and `solidcraft.adml` into
`PolicyDefinitions\en-US`, on one computer (`%SystemRoot%\PolicyDefinitions`) or in the domain's
central store. The policies are under both Computer Configuration and User Configuration,
Administrative Templates, SolidCraft.

**Intune.** In the Intune admin center choose Devices, Manage devices, Configuration, the
Import ADMX tab, Import, and upload the two files. Then choose Create, New policy, the platform
Windows 10 and later and the profile type Templates, Imported Administrative templates, and set
the policies. Intune takes one language file for a template, and only the English one. A later
version of the template has the same namespace, and Intune refuses a second upload of it: delete
the profiles that use the imported template and the template itself, then import the new files.
([Microsoft's instructions](https://learn.microsoft.com/mem/intune/configuration/administrative-templates-import-custom).)

**Without the templates.** Any tool that writes the registry values above gives the same
result, for example:

```
reg add HKLM\Software\Policies\SolidCraft /v CheckForUpdates /t REG_DWORD /d 0 /f
reg add HKLM\Software\Policies\SolidCraft /v OrganizationName /t REG_SZ /d "Example Corp" /f
reg add HKLM\Software\Policies\SolidCraft /v DefaultUnits /t REG_SZ /d in /f
reg add HKLM\Software\Policies\SolidCraft /v ShowDonateButton /t REG_DWORD /d 0 /f
reg add HKLM\Software\Policies\SolidCraft /v AiPermissions /t REG_SZ /d ask /f
```

## macOS

SolidCraft 0.46 and later read the same value names as managed preferences of the domain
`io.tangencylabs.solidcraft`, which a configuration profile installs: numbers as `integer`, the
name as `string`. `packaging/macos/policies/solidcraft.mobileconfig` in the source is a sample
profile; remove the keys you don't set and give it your own identifiers before you deploy it with
your MDM (or install it by opening it). A profile for the computer (`/Library/Managed
Preferences/io.tangencylabs.solidcraft.plist`) wins over one for the user, value by value.
Values under a `Recommended` dictionary are reserved, as on Windows.

## Linux and FreeBSD

SolidCraft 0.46 and later read `/etc/solidcraft/policy.json`, a JSON object of the same value
names (`packaging/linux/policies/policy.json` in the source is a sample):

```json
{ "CheckForUpdates": 0, "UsageStatistics": 0, "OrganizationName": "Example Corp" }
```

Make it owned by root and not writable by others (`chmod 644`): a file anyone may write is
ignored. A Flatpak or Snap sees its own `/etc`, not the computer's, so use the distribution's
package (deb, rpm, AppImage or the FreeBSD package) where you need the policy to apply.

## Check that a policy applies

While any policy is set, the application bar shows a briefcase and **Managed by** with the
organization's name (**Managed by your organization** without `OrganizationName`). A narrow
window shows **Managed** or the briefcase alone, and the tooltip has the whole text. Clicking it
opens Preferences, General. Preferences and About show the same line.

In Preferences, each setting a policy sets is greyed out, shows the organization's choice, and
says **Set by your organization**: Updates, Language, Saving and Community on the General page,
Units of new designs on the Design page, and the AI settings above on the Assistant page. With
`AiPermissions` set, the Assistant page also says that each agent's own permissions and the
**Always allowed** answers don't apply. With `CheckForUpdates` set to `0`, **Check now** is
unavailable too. A Donate or Discord button the policy shows has no way to hide it: its × says
**Set by your organization** instead.

An agent or a script sees the same through the control channel: the reply of `ui.prefs` has a
`policy` object that lists the locked settings, their values in force (`settings`) and the
organization's name. The settings themselves report each person's own stored choice, and a
request that changes a locked setting is refused ([control protocol](https://github.com/tangency-labs/solidcraft/blob/main/docs/control-protocol.md)).

## A policy is not a launcher

[Managed by a launcher](updates.md#managed-by-a-launcher) is about a program that installs and
updates SolidCraft for one person. A policy is an organization's setting. The two do not affect
each other.
