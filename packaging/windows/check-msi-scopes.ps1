<#
.SYNOPSIS
  Build the MSI around stand-in binaries and install it both ways: for all users and just for me.

.DESCRIPTION
  A check of solidcraft.wxs on a Windows machine where the caller is an administrator (the
  packaging-lint workflow's Windows job). It needs no Rust build: the two executables are copies
  of a system program, which is enough for Windows Installer.

    1. wix build of four versions (99.0.1 to 99.0.5, each its own ProductCode), then `wix msi validate` (every ICE).
    2. Prints the tables that decide the scope: Property, the Word Count summary, Component
       conditions, Registry roots (-1 is HKMU: HKLM for all users, HKCU just for me),
       Environment, and the scope actions in both sequences.
    3. Installs and checks, each with a verbose log in $Out:
       - all users, silent (the default, `/qn` with no properties), then a default upgrade;
       - just for me, silent (ALLUSERS=2 MSIINSTALLPERUSER=1), then an upgrade the way the in-app
         updater runs it (`/passive` with the same properties);
       - both side by side;
       - just for me as a standard user with no administrator rights;
       - (optional, -FromRelease) the latest published release, then the new MSI over it.
    4. (optional, -Ui) drives the full wizard with keystrokes and saves screenshots of its pages.

  Exits non-zero on the first failed check.

.EXAMPLE
  pwsh packaging/windows/check-msi-scopes.ps1 -Out target/msi-scopes -FromRelease -Ui
#>
param(
  [string] $Out = 'target/msi-scopes',
  [switch] $FromRelease,
  [switch] $Ui,
  # solidcraft.wxs as main has it: its ICE findings are the baseline.
  [string] $BaselineWxs,
  [ValidateSet('x64', 'x86', 'arm64')] [string] $Arch = 'x64'
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$script:Failures = 0

function Check([bool] $Ok, [string] $What) {
  if ($Ok) { Write-Output "  ok   $What" } else { Write-Output "::error::$What"; $script:Failures++ }
}

# ---- build ---------------------------------------------------------------------------------
$Stage = Join-Path $Out 'stage'
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
# Stand-ins: any PE file will do for the installer; the wizard's Launch box is left unticked.
Copy-Item "$env:SystemRoot\System32\where.exe" (Join-Path $Stage 'solidcraft.exe')
Copy-Item "$env:SystemRoot\System32\where.exe" (Join-Path $Stage 'solidcraft-cli.exe')
foreach ($f in 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'THIRD-PARTY-LICENSES.txt') { Copy-Item (Join-Path $Root $f) $Stage }
$Icon = Join-Path $Root 'assets/icons/app/windows/exports/solidcraft.ico'

function Build-Msi([string] $Version, [string] $Wxs, [string] $Name) {
  $msi = Join-Path $Out $Name
  $sortable = (($Version -split '\.') | ForEach-Object { $_.PadLeft(5, '0') }) -join '.'
  & wix build $Wxs -arch $Arch -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext `
    -culture en-US -loc (Join-Path $Root 'assets/platforms/windows/solidcraft.en-us.wxl') `
    -d "Version=$Version" -d "DisplayVersion=$Version" -d "SortableVersion=$sortable" -d "BinDir=$Stage" -d "IconPath=$Icon" -o $msi 2>&1 | Out-Host
  if ($LASTEXITCODE -ne 0) { throw "wix build $Name failed ($LASTEXITCODE)" }
  return $msi
}
$Wxs = Join-Path $Root 'assets/platforms/windows/solidcraft.wxs'
$V1 = Build-Msi '99.0.1' $Wxs 'v1.msi'
$V2 = Build-Msi '99.0.2' $Wxs 'v2.msi'
$V3 = Build-Msi '99.0.3' $Wxs 'v3.msi'
$V4 = Build-Msi '99.0.4' $Wxs 'v4.msi'
$V5 = Build-Msi '99.0.5' $Wxs 'v5.msi'

# Every ICE, for this MSI and for the one main builds: only findings main doesn't have fail.
function Get-IceFindings([string] $Msi, [string] $Name) {
  Write-Host "==> wix msi validate $Name (all ICEs)"
  $lines = @(& wix msi validate $Msi -intermediateFolder (Join-Path $Out "validate-$Name") 2>&1 | ForEach-Object { "$_" })
  $exit = $LASTEXITCODE
  $lines | Set-Content (Join-Path $Out "validate-$Name.txt")
  $lines | ForEach-Object { Write-Host "  $_" }
  # "file(line) : error WIX0204: ICE57: Component 'X' has ..." -> "error ICE57: Component 'X' has ..."
  $found = @($lines | ForEach-Object { if ($_ -match ' : (error|warning) WIX\d+: (ICE\d+: .*)$') { "$($Matches[1]) $($Matches[2])" } } | Sort-Object -Unique)
  # ICE errors make it exit non-zero; any other failure means validation didn't run.
  if ($exit -ne 0 -and -not ($found | Where-Object { $_ -like 'error *' })) { throw "wix msi validate $Name failed ($exit) without ICE findings" }
  return $found
}
$Findings = Get-IceFindings $V1 'this'
if ($BaselineWxs) {
  $Base = Build-Msi '99.0.1' (Resolve-Path $BaselineWxs).Path 'base.msi'
  $BaseFindings = Get-IceFindings $Base 'main'
  $New = @($Findings | Where-Object { $_ -notin $BaseFindings })
  $Gone = @($BaseFindings | Where-Object { $_ -notin $Findings })
  Write-Output '==> ICE findings main does not have:'
  $New | ForEach-Object { Write-Output "  $_" }
  Write-Output '==> ICE findings main has and this does not:'
  $Gone | ForEach-Object { Write-Output "  $_" }
  Check (-not ($New | Where-Object { $_ -like 'error *' })) 'no ICE error that main does not already have'
}

# ---- tables --------------------------------------------------------------------------------
$Installer = New-Object -ComObject WindowsInstaller.Installer
function Invoke-Com($Object, [string] $Name, [string] $Kind, [object[]] $Arguments) {
  $flags = [Reflection.BindingFlags]::$Kind
  # COM wants the plain values, not PowerShell's wrappers around them.
  if ($null -ne $Arguments) { $Arguments = [object[]]@($Arguments | ForEach-Object { if ($_ -is [psobject]) { $_.psobject.BaseObject } else { $_ } }) }
  return $Object.GetType().InvokeMember($Name, $flags, $null, $Object, $Arguments)
}
function Get-MsiRows([string] $Msi, [string] $Query) {
  $db = Invoke-Com $Installer 'OpenDatabase' 'InvokeMethod' @($Msi, 0)
  $view = Invoke-Com $db 'OpenView' 'InvokeMethod' @($Query)
  Invoke-Com $view 'Execute' 'InvokeMethod' $null | Out-Null
  $rows = @()
  while ($rec = Invoke-Com $view 'Fetch' 'InvokeMethod' $null) {
    $n = Invoke-Com $rec 'FieldCount' 'GetProperty' $null
    $rows += ((1..$n | ForEach-Object { Invoke-Com $rec 'StringData' 'GetProperty' @($_) }) -join ' | ')
  }
  Invoke-Com $view 'Close' 'InvokeMethod' $null | Out-Null
  [Runtime.InteropServices.Marshal]::ReleaseComObject($db) | Out-Null
  return $rows
}
$si = Invoke-Com $Installer 'SummaryInformation' 'GetProperty' @($V1, 0)
$WordCount = Invoke-Com $si 'Property' 'GetProperty' @(15)
Write-Output "==> Summary: Word Count = $WordCount (bit 8 clear: Windows asks for elevation as it always did)"
Check (($WordCount -band 8) -eq 0) 'Word Count keeps elevation for the default per-machine install'
foreach ($q in @(
    "SELECT ``Property``, ``Value`` FROM ``Property`` WHERE ``Property``='ALLUSERS' OR ``Property``='MSIINSTALLPERUSER' OR ``Property``='SolidcraftScope' OR ``Property``='REINSTALLMODE'",
    'SELECT `Component`, `Directory_`, `Condition`, `KeyPath` FROM `Component`',
    'SELECT `Root`, `Key`, `Name`, `Value`, `Component_` FROM `Registry`',
    'SELECT `Environment`, `Name`, `Value`, `Component_` FROM `Environment`',
    'SELECT `Signature_`, `Root`, `Key`, `Name` FROM `RegLocator`',
    "SELECT ``Action``, ``Condition``, ``Sequence`` FROM ``InstallUISequence`` WHERE ``Sequence`` < 1300",
    "SELECT ``Action``, ``Condition``, ``Sequence`` FROM ``InstallExecuteSequence`` WHERE ``Sequence`` < 1300",
    'SELECT `Action`, `Type`, `Source`, `Target` FROM `CustomAction`')) {
  Write-Output "==> $q"
  Get-MsiRows $V1 $q | ForEach-Object { Write-Output "  $_" }
}
$roots = Get-MsiRows $V1 'SELECT `Root`, `Key` FROM `Registry`'
Check (-not ($roots | Where-Object { $_ -match '^2 \|' })) 'no Registry row is pinned to HKLM (root 2); they are HKMU (-1) or HKCR (0)'
$props = Get-MsiRows $V1 "SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ALLUSERS'"
Check ($props -eq '1') 'ALLUSERS=1 in the Property table: per-machine by default, as before'

# ---- install helpers -----------------------------------------------------------------------
$Log = 0
function Invoke-Msi([string[]] $Arguments, [string] $What) {
  $script:Log++
  $logFile = Join-Path $Out ("{0:D2}-{1}.log" -f $script:Log, ($What -replace '[^A-Za-z0-9]+', '-'))
  $p = Start-Process msiexec -ArgumentList ($Arguments + @('/l*v', "`"$logFile`"")) -Wait -PassThru
  Write-Host "==> msiexec $($Arguments -join ' ')  ->  $($p.ExitCode)  ($What)"
  return $p.ExitCode
}
$MachineDir = if ($Arch -eq 'x86') { "${env:ProgramFiles(x86)}\SolidCraft" } else { "$env:ProgramFiles\SolidCraft" }
$UserDir = "$env:LOCALAPPDATA\Programs\SolidCraft"
$MachineMenu = "$env:ProgramData\Microsoft\Windows\Start Menu\Programs\SolidCraft.lnk"
$UserMenu = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\SolidCraft.lnk"
# The installed SolidCraft versions in one Windows Installer context: 4 all users, 2 this user
# (unmanaged). Windows lists both in Programs and Features, and keeps a per-user product's
# uninstall entry under HKLM too, so the context is what tells them apart.
function Get-Installed([int] $Context) {
  # PowerShell unrolls the returned collection into its items.
  $found = @()
  foreach ($product in @(Invoke-Com $Installer 'ProductsEx' 'GetProperty' @('', '', $Context))) {
    if ($null -eq $product) { continue }
    if ((Invoke-Com $product 'InstallProperty' 'GetProperty' @('ProductName')) -eq 'SolidCraft') {
      $found += Invoke-Com $product 'InstallProperty' 'GetProperty' @('VersionString')
    }
  }
  return ,$found
}
function Get-PathEntries([string] $Scope, [string] $Dir) {
  $path = [Environment]::GetEnvironmentVariable('PATH', $Scope)
  return @(($path -split ';') | Where-Object { $_.TrimEnd('\') -ieq $Dir.TrimEnd('\') }).Count
}
function Get-Command-Value([string] $Key) {
  return (Get-ItemProperty -LiteralPath $Key -ErrorAction SilentlyContinue).'(default)'
}
function Assert-Machine([string] $Version) {
  Check (Test-Path "$MachineDir\solidcraft.exe") "all users: $MachineDir\solidcraft.exe"
  Check (Test-Path "$MachineDir\solidcraft-cli.exe") 'all users: solidcraft-cli.exe'
  Check (Test-Path $MachineMenu) 'all users: Start menu shortcut in ProgramData'
  Check ((Get-Command-Value 'HKLM:\Software\Classes\solidcraft\shell\open\command') -like '*--open-url*') 'all users: solidcraft:// under HKLM\Software\Classes'
  Check ((Get-Command-Value 'HKLM:\Software\Classes\.sc') -eq 'Solidcraft.Design') 'all users: .sc under HKLM\Software\Classes'
  Check ((Get-ItemProperty 'HKLM:\Software\SolidCraft' -ErrorAction SilentlyContinue).InstallDir.TrimEnd('\') -ieq $MachineDir) 'all users: HKLM\Software\SolidCraft InstallDir'
  Check ((Get-PathEntries 'Machine' $MachineDir) -eq 1) 'all users: install folder once on the system PATH'
  $v = Get-Installed 4
  Check ($v.Count -eq 1 -and $v[0] -eq $Version) "all users: one installed product, version $Version (found $($v -join ', '))"
}
function Assert-NoMachine {
  Check (-not (Test-Path "$MachineDir\solidcraft.exe")) 'no all-users files'
  Check (-not (Test-Path 'HKLM:\Software\Classes\solidcraft')) 'no solidcraft:// under HKLM'
  Check (-not (Test-Path 'HKLM:\Software\SolidCraft')) 'no HKLM\Software\SolidCraft'
  Check ((Get-PathEntries 'Machine' $MachineDir) -eq 0) 'not on the system PATH'
  Check ((Get-Installed 4).Count -eq 0) 'no all-users product'
}
function Assert-User([string] $Version) {
  Check (Test-Path "$UserDir\solidcraft.exe") "just for me: $UserDir\solidcraft.exe"
  Check (Test-Path "$UserDir\solidcraft-cli.exe") 'just for me: solidcraft-cli.exe'
  Check (Test-Path $UserMenu) "just for me: Start menu shortcut in the user's profile"
  Check ((Get-Command-Value 'HKCU:\Software\Classes\solidcraft\shell\open\command') -like '*--open-url*') 'just for me: solidcraft:// under HKCU\Software\Classes'
  Check ((Get-Command-Value 'HKCU:\Software\Classes\.sc') -eq 'Solidcraft.Design') 'just for me: .sc under HKCU\Software\Classes'
  Check ((Get-ItemProperty 'HKCU:\Software\SolidCraft' -ErrorAction SilentlyContinue).InstallDir.TrimEnd('\') -ieq $UserDir) 'just for me: HKCU\Software\SolidCraft InstallDir'
  Check ((Get-Command-Value 'HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\solidcraft.exe') -like "$UserDir*") 'just for me: App Paths under HKCU'
  Check ((Get-PathEntries 'User' $UserDir) -eq 1) "just for me: install folder once on the user's PATH"
  $v = Get-Installed 2
  Check ($v.Count -eq 1 -and $v[0] -eq $Version) "just for me: one installed product, version $Version (found $($v -join ', '))"
}
function Assert-NoUser {
  Check (-not (Test-Path "$UserDir\solidcraft.exe")) 'no per-user files'
  Check (-not (Test-Path 'HKCU:\Software\Classes\solidcraft')) 'no solidcraft:// under HKCU'
  Check (-not (Test-Path 'HKCU:\Software\Classes\.sc')) 'no .sc under HKCU'
  Check (-not (Test-Path 'HKCU:\Software\Classes\Solidcraft.Design')) 'no Solidcraft.Design under HKCU'
  Check (-not (Test-Path $UserMenu)) "no Start menu shortcut in the user's profile"
  Check (-not (Test-Path 'HKCU:\Software\SolidCraft')) 'no HKCU\Software\SolidCraft'
  Check ((Get-PathEntries 'User' $UserDir) -eq 0) "not on the user's PATH"
  Check ((Get-Installed 2).Count -eq 0) 'no per-user product'
}

# ---- all users (the default) ---------------------------------------------------------------
Check ((Invoke-Msi @('/i', "`"$V1`"", '/qn') 'all users install') -eq 0) 'all users: silent install with no properties'
Assert-Machine '99.0.1'
Assert-NoUser
Check ((Invoke-Msi @('/i', "`"$V2`"", '/qn') 'all users upgrade') -eq 0) 'all users: silent upgrade'
Assert-Machine '99.0.2'
Assert-NoUser
Check ((Invoke-Msi @('/x', "`"$V2`"", '/qn') 'all users uninstall') -eq 0) 'all users: uninstall'
Assert-NoMachine

# ---- just for me ---------------------------------------------------------------------------
$PerUser = @('ALLUSERS=2', 'MSIINSTALLPERUSER=1')
Check ((Invoke-Msi (@('/i', "`"$V1`"", '/qn') + $PerUser) 'per-user install') -eq 0) 'just for me: silent install'
Assert-User '99.0.1'
Assert-NoMachine
# What the in-app updater runs for a per-user install.
Check ((Invoke-Msi (@('/i', "`"$V2`"", '/passive', 'REBOOT=ReallySuppress') + $PerUser) 'per-user upgrade') -eq 0) 'just for me: upgrade as the in-app updater runs it'
Assert-User '99.0.2'
Assert-NoMachine

# ---- both side by side -------------------------------------------------------------------------
# A version not installed for this user yet (a version installed in one context is that context's
# for this user: running the same MSI again only repairs it there).
Check ((Invoke-Msi @('/i', "`"$V1`"", '/qn') 'all users beside per-user') -eq 0) 'all users install beside a per-user one'
Assert-Machine '99.0.1'
Assert-User '99.0.2'
Check ((Invoke-Msi @('/i', "`"$V3`"", '/qn') 'all users upgrade beside per-user') -eq 0) 'a default upgrade updates the all-users install'
Assert-Machine '99.0.3'
Assert-User '99.0.2'
Check ((Invoke-Msi (@('/i', "`"$V4`"", '/qn') + $PerUser) 'per-user upgrade beside all users') -eq 0) 'a per-user upgrade updates the per-user install'
Assert-User '99.0.4'
Assert-Machine '99.0.3'
# `/x` takes the version's ProductCode, which is installed in one context only.
Check ((Invoke-Msi @('/x', "`"$V4`"", '/qn') 'per-user uninstall') -eq 0) 'just for me: uninstall'
Assert-NoUser
Assert-Machine '99.0.3'
Check ((Invoke-Msi @('/x', "`"$V3`"", '/qn') 'all users uninstall 2') -eq 0) 'all users: uninstall'
Assert-NoMachine

# ---- a standard user, no administrator ---------------------------------------------------------
Write-Output '==> just for me as a standard user'
$Shared = 'C:\sc-scope-test'
New-Item -ItemType Directory -Force -Path $Shared | Out-Null
& icacls $Shared /grant 'Users:(OI)(CI)M' | Out-Null
Copy-Item $V5 (Join-Path $Shared 'v1.msi')
# Windows Server turns Windows Installer off for non-administrators unless the DisableMsi policy
# says otherwise ("Non-assigned apps are disabled for non-admin users", 1625); Windows 10 and 11
# let them install per-user packages. This build machine is a Server: set the desktop default.
$InstallerPolicy = 'HKLM:\Software\Policies\Microsoft\Windows\Installer'
$HadPolicy = Test-Path $InstallerPolicy
$DisableMsi = (Get-ItemProperty $InstallerPolicy -ErrorAction SilentlyContinue).DisableMsi
New-Item -Force -Path $InstallerPolicy -ErrorAction SilentlyContinue | Out-Null
Set-ItemProperty $InstallerPolicy -Name DisableMsi -Value 0 -Type DWord
$userScript = Join-Path $Shared 'as-user.ps1'
@'
$ErrorActionPreference = 'Continue'
$r = [ordered]@{ user = [Security.Principal.WindowsIdentity]::GetCurrent().Name }
$r.admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$p = Start-Process msiexec -ArgumentList '/i', 'C:\sc-scope-test\v1.msi', '/qn', 'ALLUSERS=2', 'MSIINSTALLPERUSER=1', '/l*v', 'C:\sc-scope-test\user-install.log' -Wait -PassThru
$r.perUserExit = $p.ExitCode
$r.perUserExe = Test-Path "$env:SystemDrive\Users\$([Environment]::UserName)\AppData\Local\Programs\SolidCraft\solidcraft.exe"
$r.perUserHandler = Test-Path 'HKCU:\Software\Classes\solidcraft\shell\open\command'
$r.machineHandler = Test-Path 'HKLM:\Software\Classes\solidcraft'
$p = Start-Process msiexec -ArgumentList '/x', 'C:\sc-scope-test\v1.msi', '/qn', 'ALLUSERS=2', 'MSIINSTALLPERUSER=1' -Wait -PassThru
$r.perUserUninstallExit = $p.ExitCode
# The default (all users) still needs an administrator: without one it must fail, not install.
$p = Start-Process msiexec -ArgumentList '/i', 'C:\sc-scope-test\v1.msi', '/qn', '/l*v', 'C:\sc-scope-test\user-machine.log' -Wait -PassThru
$r.perMachineExit = $p.ExitCode
$r | ConvertTo-Json | Set-Content 'C:\sc-scope-test\result.json'
'@ | Set-Content $userScript
# A new account each run (sc + 8 hex digits, within the 20-character limit), removed afterwards
# together with the policy change, whatever happens in between.
$name = 'sc' + [guid]::NewGuid().ToString('N').Substring(0, 8)
$password = 'Sc!' + [guid]::NewGuid().ToString('N').Substring(0, 20) + 'a1'
$secure = ConvertTo-SecureString $password -AsPlainText -Force
try {
  New-LocalUser -Name $name -Password $secure -AccountNeverExpires -PasswordNeverExpires | Out-Null
  Add-LocalGroupMember -Group 'Users' -Member $name -ErrorAction SilentlyContinue
  $cred = [Management.Automation.PSCredential]::new($name, $secure)
  $p = Start-Process powershell -Credential $cred -LoadUserProfile -WorkingDirectory $Shared -Wait -PassThru `
    -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $userScript
} finally {
  Remove-LocalUser -Name $name -ErrorAction SilentlyContinue
  if ($null -ne $DisableMsi) { Set-ItemProperty $InstallerPolicy -Name DisableMsi -Value $DisableMsi }
  elseif ($HadPolicy) { Remove-ItemProperty $InstallerPolicy -Name DisableMsi } else { Remove-Item $InstallerPolicy }
}
$resultFile = Join-Path $Shared 'result.json'
if (Test-Path $resultFile) {
  $r = Get-Content $resultFile -Raw | ConvertFrom-Json
  Get-Content $resultFile | Write-Output
  Check (-not $r.admin) "standard user $($r.user) is not an administrator"
  Check ($r.perUserExit -eq 0) "standard user: just-for-me install succeeds without an administrator (exit $($r.perUserExit))"
  Check ($r.perUserExe -and $r.perUserHandler -and -not $r.machineHandler) 'standard user: files and solidcraft:// in the profile and HKCU only'
  Check ($r.perUserUninstallExit -eq 0) 'standard user: uninstall'
  Check ($r.perMachineExit -ne 0) "standard user: the default all-users install still needs an administrator (exit $($r.perMachineExit))"
} else {
  Check $false "the standard-user run wrote no result (powershell exit $($p.ExitCode))"
}
foreach ($f in 'user-install.log', 'user-machine.log') { Copy-Item (Join-Path $Shared $f) $Out -ErrorAction SilentlyContinue }

# ---- the latest release, then this MSI over it ------------------------------------------------
if ($FromRelease) {
  $released = Join-Path $Out 'released.msi'
  Invoke-WebRequest "https://github.com/tangency-labs/solidcraft/releases/latest/download/solidcraft-windows-$Arch.msi" -OutFile $released
  Check ((Invoke-Msi @('/i', "`"$released`"", '/qn') 'released install') -eq 0) 'the latest release installs'
  Check ((Invoke-Msi @('/i', "`"$V1`"", '/qn') 'upgrade from release') -eq 0) 'this MSI upgrades it by default'
  Assert-Machine '99.0.1'
  Assert-NoUser
  Check ((Invoke-Msi @('/x', "`"$V1`"", '/qn') 'uninstall after release upgrade') -eq 0) 'uninstall'
  Assert-NoMachine
}

# ---- the wizard ----------------------------------------------------------------------------
if ($Ui) {
  Write-Output '==> the wizard, driven with keystrokes'
  Add-Type -AssemblyName System.Windows.Forms, System.Drawing
  Add-Type @'
using System; using System.Runtime.InteropServices;
public static class Win {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string title);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
'@
  $shell = New-Object -ComObject WScript.Shell
  function Wait-Window {
    for ($i = 0; $i -lt 120; $i++) {
      $w = Get-Process msiexec -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne [IntPtr]::Zero } | Select-Object -First 1
      if ($w) { return $w.MainWindowHandle }
      Start-Sleep -Milliseconds 500
    }
    Write-Host "  msiexec windows: $((Get-Process msiexec -ErrorAction SilentlyContinue | ForEach-Object { "$($_.Id) '$($_.MainWindowTitle)' session $($_.SessionId)" }) -join '; ')"
    return [IntPtr]::Zero
  }
  function Save-Window([string] $Name) {
    Start-Sleep -Milliseconds 1500
    $h = Wait-Window
    if ($h -eq [IntPtr]::Zero) { throw 'no "SolidCraft Setup" window' }
    [Win]::SetForegroundWindow($h) | Out-Null
    $r = New-Object Win+RECT
    [Win]::GetWindowRect($h, [ref] $r) | Out-Null
    $w = $r.Right - $r.Left; $hgt = $r.Bottom - $r.Top
    $bmp = New-Object Drawing.Bitmap $w, $hgt
    $g = [Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
    $bmp.Save((Join-Path $Out "$Name.png"), [Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    Write-Output "  saved $Name.png ($w x $hgt)"
  }
  function Send([string] $Keys) { $h = Wait-Window; if ($h -eq [IntPtr]::Zero) { throw 'no "SolidCraft Setup" window' }; [Win]::SetForegroundWindow($h) | Out-Null; $shell.SendKeys($Keys) }

  # A first install through the wizard, choosing "just for me".
  $p = Start-Process msiexec -ArgumentList '/i', "`"$V1`"", 'WIXUI_EXITDIALOGOPTIONALCHECKBOX=0', '/l*v', "`"$(Join-Path $Out 'ui-install.log')`"" -PassThru
  if ((Wait-Window) -eq [IntPtr]::Zero) {
    # No desktop to show a window on (the runner is a service): nothing to drive.
    Write-Output "::warning::no interactive desktop here, so the wizard was not driven (session $([Diagnostics.Process]::GetCurrentProcess().SessionId))"
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    $Ui = $false
  }
}
if ($Ui) {
  Save-Window 'wizard-1-welcome'
  Send '{ENTER}'
  Save-Window 'wizard-2-scope'
  Send '%j'
  Save-Window 'wizard-3-scope-just-me'
  Send '{ENTER}'
  Save-Window 'wizard-4-folder'
  Send '{ENTER}'
  Save-Window 'wizard-5-ready'
  Send '{ENTER}'
  for ($i = 0; $i -lt 120 -and -not ((Test-Path "$UserDir\solidcraft.exe") -and (Get-Installed 2).Count); $i++) { Start-Sleep -Milliseconds 500 }
  Save-Window 'wizard-6-finish'
  Send '{ENTER}'
  $p.WaitForExit(60000) | Out-Null
  Write-Output "  wizard exit code $($p.ExitCode)"
  Assert-User '99.0.1'
  Assert-NoMachine

  # The newer MSI through the wizard: it finds the per-user install and updates it in place.
  $p = Start-Process msiexec -ArgumentList '/i', "`"$V2`"", 'WIXUI_EXITDIALOGOPTIONALCHECKBOX=0', '/l*v', "`"$(Join-Path $Out 'ui-upgrade.log')`"" -PassThru
  Save-Window 'wizard-7-upgrade-welcome'
  Send '{ENTER}'
  Save-Window 'wizard-8-upgrade-ready'
  Send '{ENTER}'
  for ($i = 0; $i -lt 120 -and ((Get-Installed 2) -notcontains '99.0.2'); $i++) { Start-Sleep -Milliseconds 500 }
  Save-Window 'wizard-9-upgrade-finish'
  Send '{ENTER}'
  $p.WaitForExit(60000) | Out-Null
  Assert-User '99.0.2'
  Assert-NoMachine
  Invoke-Msi (@('/x', "`"$V2`"", '/qn') + $PerUser) 'ui cleanup' | Out-Null
}

if ($script:Failures) { throw "$script:Failures check(s) failed; logs in $Out" }
Write-Output 'all MSI scope checks passed'
