# Builds the Windows installer (.msi) from the full Windows package (scripts/package-full.sh).
#
#   scripts/package-msi.ps1 -Full <ekos-...-x86_64-pc-windows-msvc-full.zip> -Out <dir>
#                           -Version <file version: X.Y.Z or dev> [-MsiVersion <X.Y.Z>]
#
# Needs WiX v5 as a .NET tool, with its UI and Util extensions:
#   dotnet tool install --global wix --version 5.0.2
#   wix extension add -g WixToolset.UI.wixext/5.0.2 WixToolset.Util.wixext/5.0.2
# What the package installs and why: packaging/wix/ekos.wxs. Prints the .msi's path.
param(
  [Parameter(Mandatory)] [string] $Full,
  [Parameter(Mandatory)] [string] $Out,
  [Parameter(Mandatory)] [string] $Version,
  [string] $MsiVersion = ''
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$assets = Join-Path $repo 'packaging\wix'

# An MSI version is three numbers. A tag gives them (v1.5.5-rc1 → 1.5.5); a manual run takes the
# workspace version.
if (-not $MsiVersion) {
  if ($Version -eq 'dev') {
    $m = Select-String -Path (Join-Path $repo 'ekos\Cargo.toml') -Pattern '^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)' |
         Select-Object -First 1
    $MsiVersion = $m.Matches[0].Groups[1].Value
  } else {
    $MsiVersion = $Version -replace '-.*$', ''
  }
}
if ($MsiVersion -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$') { throw "cannot derive an MSI version from '$Version'" }

$work = Join-Path ([IO.Path]::GetTempPath()) ("ekos-msi-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Force $work | Out-Null
try {
  Expand-Archive $Full -DestinationPath $work
  $src = Get-ChildItem $work -Directory | Select-Object -First 1
  if (-not (Test-Path "$($src.FullName)\bin\ekos.exe")) { throw "$Full has no bin\ekos.exe" }

  # The licence page reads RTF: the MIT text, escaped, a paragraph per line.
  $text = Get-Content (Join-Path $repo 'LICENSE') -Raw
  $text = $text -replace '\\', '\\' -replace '\{', '\{' -replace '\}', '\}'
  $text = $text -replace "`r?`n", "\par`n"
  "{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}\f0\fs18 $text}" |
    Set-Content -Encoding ascii (Join-Path $work 'license.rtf')
  Copy-Item (Join-Path $assets '*') $work

  New-Item -ItemType Directory -Force $Out | Out-Null
  $msi = Join-Path (Resolve-Path $Out) "ekos-$Version-x86_64-pc-windows-msvc.msi"
  # ICE38 / ICE64 are suppressed on purpose: in a per-user package the console's harvested files keep
  # file key paths and their directories have no RemoveFolder rows each; uninstall removes the
  # whole program directory with util:RemoveFolderEx instead (packaging/wix/ekos.wxs).
  # wix's own messages go to the host, not the pipeline: callers keep only this script's last line.
  & wix build (Join-Path $assets 'ekos.wxs') -arch x64 `
      -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext `
      -d "Version=$MsiVersion" -d "Source=$($src.FullName)" -d "Assets=$work" `
      -sice ICE38 -sice ICE64 `
      -o $msi | Out-Host
  if ($LASTEXITCODE -ne 0) { throw "wix build failed ($LASTEXITCODE)" }
  Write-Output $msi
} finally {
  Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
