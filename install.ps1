# EKOS installer for Windows (RFC 0185).
#
#   powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/alexeyban/EKOS/main/install.ps1 | iex"
#
# Downloads the full package (the ekos command, the web console and the Python that runs it),
# verifies it against the release's SHA256SUMS before unpacking, copies it into the EKOS home
# (default %USERPROFILE%\.ekos) and opens the setup wizard in the browser. No administrator
# rights: everything goes under your user profile.
#
# Environment:
#   EKOS_VERSION    tag to install (default: the latest release)
#   EKOS_HOME       the EKOS home (default: %USERPROFILE%\.ekos)
#   EKOS_BASE_URL   download from here instead of GitHub (a mirror, or file:///C:/path)
#   EKOS_NO_SETUP=1 do not open the setup wizard

$ErrorActionPreference = 'Stop'
$Repo = 'alexeyban/EKOS'
$Target = 'x86_64-pc-windows-msvc'

if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') {
    throw "EKOS packages are built for 64-bit Intel/AMD Windows; this is $($env:PROCESSOR_ARCHITECTURE)."
}

$Home_ = if ($env:EKOS_HOME) { $env:EKOS_HOME } else { Join-Path $env:USERPROFILE '.ekos' }

$Version = $env:EKOS_VERSION
if (-not $Version) {
    Write-Host 'Resolving the latest release...'
    $Version = (Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest").tag_name
}
$Bare = $Version -replace '^v', ''
$Base = if ($env:EKOS_BASE_URL) { $env:EKOS_BASE_URL.TrimEnd('/') } else { "https://github.com/$Repo/releases/download/$Version" }

$Tmp = Join-Path ([IO.Path]::GetTempPath()) ("ekos-install-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $Tmp | Out-Null
try {
    function Fetch($name) {
        $dest = Join-Path $Tmp $name
        if ($Base -like 'file:*') {
            Copy-Item -LiteralPath ([Uri]"$Base/$name").LocalPath -Destination $dest
        } else {
            Invoke-WebRequest -UseBasicParsing "$Base/$name" -OutFile $dest
        }
        return $dest
    }

    $Asset = "ekos-$Bare-$Target-full.zip"
    $Sums = Fetch 'SHA256SUMS'
    $line = Select-String -Path $Sums -SimpleMatch $Asset | Select-Object -First 1
    if (-not $line) {
        throw "$Asset is not listed in SHA256SUMS for $Version - this release has no full package for Windows. Download the .zip of the command from https://github.com/$Repo/releases instead."
    }
    $expected = ($line.Line -split '\s+')[0].ToLower()

    Write-Host "Downloading $Asset ($Version)..."
    $zip = Fetch $Asset
    $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $expected) {
        throw "checksum mismatch for $Asset (expected $expected, got $actual) - refusing to install"
    }
    Write-Host 'Checksum verified.'

    Expand-Archive -LiteralPath $zip -DestinationPath $Tmp
    $pkg = Join-Path $Tmp "ekos-$Bare-$Target-full"

    # Replace the program, never the data: only bin\ and console\ are touched.
    New-Item -ItemType Directory -Force -Path (Join-Path $Home_ 'data') | Out-Null
    # Move the old program aside first. A running ekos.exe or console (an agent's MCP server, a
    # pipeline run) locks its directory: then nothing is changed and the message says why,
    # instead of a half-deleted program directory.
    $stamp = Get-Date -Format 'yyyyMMddHHmmss'
    $moved = @()
    foreach ($part in 'bin', 'console') {
        $dest = Join-Path $Home_ $part
        if (-not (Test-Path $dest)) { continue }
        try {
            Rename-Item -LiteralPath $dest -NewName "$part.old-$stamp"
            $moved += $part
        } catch {
            foreach ($back in $moved) {
                Rename-Item -LiteralPath (Join-Path $Home_ "$back.old-$stamp") -NewName $back
            }
            throw "EKOS is running from $Home_ (the console, a pipeline run or an agent's MCP server). Stop it - ekos console stop, and close the agents - then run this again. Nothing was changed."
        }
    }
    foreach ($part in 'bin', 'console') {
        Copy-Item -Recurse -LiteralPath (Join-Path $pkg $part) -Destination (Join-Path $Home_ $part)
    }
    foreach ($part in $moved) {
        Remove-Item -Recurse -Force (Join-Path $Home_ "$part.old-$stamp") -ErrorAction SilentlyContinue
    }
    $ekos = Join-Path $Home_ 'bin\ekos.exe'
    & $ekos --version | Out-Null
    Write-Host ""
    Write-Host "Installed EKOS $Version with its web console to $Home_"
} finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}

if ($env:EKOS_NO_SETUP -eq '1') {
    Write-Host "Next: & '$ekos' setup"
} else {
    Write-Host 'Opening the setup page in your browser...'
    & $ekos --home $Home_ setup
}
