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
| Anonymous usage statistics | `UsageStatistics` | `REG_DWORD` | `0`: SolidCraft sends no usage statistics, keeps none to send later and shows no notice about them, and people cannot turn them on. `1`: each person chooses. SolidCraft 0.46 or later. |
| Organization name | `OrganizationName` | `REG_SZ` | The name shown as **Managed by** and the name, up to 64 characters on one line. It changes no setting. |

On Windows the values live in the key `Software\Policies\SolidCraft`:

- under `HKEY_LOCAL_MACHINE` for everyone on the computer, or
- under `HKEY_CURRENT_USER` for one person.

When both hold a value, the computer's value wins. A value of another type, a number other
than the ones listed, or an empty name counts as not set.

SolidCraft reads the policy when it starts. A change applies the next time SolidCraft starts.
When a policy is removed, the person's own earlier choice is back.

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

Open Preferences, General, Updates. With `CheckForUpdates` set, **Check at start and once a day**
is greyed out, shows the organization's choice, and says **Set by your organization**. With the
value `0`, **Check now** is unavailable too.

An agent or a script sees the same through the control channel: the reply of `ui.prefs` has a
`policy` object that lists the locked settings and the organization's name, and a request that changes a locked setting is
refused ([control protocol](https://github.com/tangency-labs/solidcraft/blob/main/docs/control-protocol.md)).

## A policy is not a launcher

[Managed by a launcher](updates.md#managed-by-a-launcher) is about a program that installs and
updates SolidCraft for one person. A policy is an organization's setting. The two do not affect
each other.
