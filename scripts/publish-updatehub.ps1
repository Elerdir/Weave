<#
.SYNOPSIS
  Nahraje podepsany Windows instalator Weave do UpdateHubu (jako Draft).

.DESCRIPTION
  Bere vystup instalator.bat: target\release\bundle\nsis\Weave_<verze>_x64-setup.exe
  a vedle nej .sig. Verze se cte z src-tauri\tauri.conf.json.

  CI token se uklada pres DPAPI (Export-Clixml) do
  %USERPROFILE%\.updatehub\weave-token.clixml - rozsifruje ho jen tenhle
  uzivatel na tomhle stroji. Poprve spust se -SaveToken.

  Release se v UpdateHubu zalozi jako Draft; zverejni se rucne v admin UI
  (Publish). Teprve pak ho klienti uvidi.

.EXAMPLE
  .\scripts\publish-updatehub.ps1 -SaveToken
  .\scripts\publish-updatehub.ps1 -Notes "Lista s nabidkou aktualizaci, ..."
  .\scripts\publish-updatehub.ps1 -NotesFile notes.md
#>
[CmdletBinding()]
param(
    [string]$Notes,
    [string]$NotesFile,
    [ValidateSet("stable", "beta", "alpha")]
    [string]$Channel = "stable",
    [string]$Url = "https://updatehub.niderle.cz",
    [switch]$SaveToken
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$tokenFile = Join-Path $env:USERPROFILE ".updatehub\weave-token.clixml"

if ($SaveToken) {
    $secure = Read-Host "CI token aplikace weave z UpdateHubu" -AsSecureString
    New-Item -ItemType Directory -Force (Split-Path $tokenFile) | Out-Null
    $secure | Export-Clixml $tokenFile
    Write-Host "Token ulozen do $tokenFile"
}

if (-not (Test-Path $tokenFile)) {
    throw "Chybi token. Spust skript jednou s -SaveToken."
}
$bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR((Import-Clixml $tokenFile))
try { $token = [Runtime.InteropServices.Marshal]::PtrToStringAuto($bstr) }
finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }

$conf = Get-Content (Join-Path $root "src-tauri\tauri.conf.json") -Raw -Encoding UTF8 | ConvertFrom-Json
$version = $conf.version
$installer = Join-Path $root "target\release\bundle\nsis\Weave_${version}_x64-setup.exe"
$sigFile = "$installer.sig"

if (-not (Test-Path $installer)) { throw "Instalator verze $version nenalezen: $installer (spust instalator.bat)" }
if (-not (Test-Path $sigFile)) { throw "Chybi podpis $sigFile - instalator.bat nenasel klic weave-updater.key" }
if ((Get-Item $sigFile).LastWriteTime -lt (Get-Item $installer).LastWriteTime.AddMinutes(-5)) {
    throw "Podpis je starsi nez instalator - patri k jinemu buildu."
}

if ($NotesFile) { $Notes = Get-Content $NotesFile -Raw -Encoding UTF8 }
if (-not $Notes) { $Notes = "Weave $version" }

# Poznamky jdou pres docasny soubor: curl -F "pole=<soubor" je posle beze zmeny,
# vcetne diakritiky a uvozovek, ktere by PS 5.1 v argumentu rozbil.
$notesTmp = [IO.Path]::GetTempFileName()
[IO.File]::WriteAllText($notesTmp, $Notes, (New-Object Text.UTF8Encoding $false))

$sizeMb = [math]::Round((Get-Item $installer).Length / 1MB)
Write-Host "Nahravam Weave $version ($sizeMb MB, kanal $Channel) do $Url ..."

try {
    $response = & curl.exe --fail-with-body --silent --show-error `
        -X POST "$Url/api/ci/apps/weave/releases" `
        -H "X-UpdateHub-Token: $token" `
        -F "file=@$installer" `
        -F "signature=<$sigFile" `
        -F "version=$version" `
        -F "platform=windows" `
        -F "arch=x64" `
        -F "channel=$Channel" `
        -F "release_notes=<$notesTmp"
    if ($LASTEXITCODE -ne 0) { throw "Upload selhal (curl $LASTEXITCODE): $response" }
}
finally {
    Remove-Item $notesTmp -ErrorAction SilentlyContinue
}

Write-Host $response
Write-Host ""
Write-Host "Hotovo. Release je v UpdateHubu jako Draft - zverejni ho v admin UI (Publish)."
