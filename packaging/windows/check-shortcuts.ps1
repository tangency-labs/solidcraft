# A pinned shortcut must not depend on an icon cached under an MSI ProductCode.
param([Parameter(Mandatory)][string] $Path)
$ErrorActionPreference = 'Stop'
[xml] $source = Get-Content -LiteralPath $Path -Raw
$names = [Xml.XmlNamespaceManager]::new($source.NameTable)
$names.AddNamespace('w', 'http://wixtoolset.org/schemas/v4/wxs')
$shortcuts = @($source.SelectNodes('//w:Shortcut', $names))
if (!@($shortcuts | Where-Object { $_.GetAttribute('Id') -eq 'StartMenuShortcut' }).Count) {
    throw 'StartMenuShortcut is missing'
}
foreach ($shortcut in $shortcuts) {
    if ($shortcut.GetAttribute('Advertise') -eq 'yes') {
        throw 'An advertised shortcut changes identity with the MSI ProductCode'
    }
    if ($shortcut.HasAttribute('Icon')) {
        throw 'A shortcut must use its target executable icon, not an MSI-cached Icon'
    }
}
Write-Output "ok stable shortcut icon sources: $($shortcuts.Count) shortcut(s)"
