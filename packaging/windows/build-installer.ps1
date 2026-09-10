# build-installer.ps1 - compile packaging/windows/goble.iss with Inno Setup 6.
#
# Responsibilities:
#   * locate ISCC.exe (env var, standard install dirs, then the registry)
#   * resolve the version (param > $env:GOBLE_VERSION > packaging/version.env)
#   * stage goble.iss + a rendered EULA.txt so LicenseFile=EULA.txt resolves
#   * pass every path in as an absolute /D define, so the staged copy compiles
#   * attach a SignTool definition only when signing env vars are present
#
# Signing environment (all optional; absent => unsigned build + warning):
#   WINDOWS_SIGN_CERT_PFX          path to a .pfx
#   WINDOWS_SIGN_CERT_PASSWORD     password for the .pfx
#   WINDOWS_SIGN_CERT_THUMBPRINT   certificate thumbprint in the user's store
#   WINDOWS_SIGN_TIMESTAMP_URL     RFC 3161 timestamp URL (default DigiCert)
#
# This script never builds Rust. Run `cargo build --release -p goble-app`
# first, or point -SourceBinary at an existing exe.

[CmdletBinding()]
param(
    [string]$Version,
    [string]$SourceBinary,
    [string]$OutDir,
    [string]$AppName = 'Goble',
    [ValidateSet('x64', 'arm64')]
    [string]$Arch = 'x64',
    [switch]$SkipSign
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Step([string]$Message) { Write-Host "==> $Message" }
function Write-Warn([string]$Message) { Write-Warning $Message }

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$PackagingDir = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$IssPath = Join-Path $PSScriptRoot 'goble.iss'
$EulaPath = Join-Path $PackagingDir 'EULA.txt'
$IconPath = Join-Path $RepoRoot 'crates\goble-desktop-native\resources\icons\icon.ico'
$LicensePath = Join-Path $RepoRoot 'LICENSE'
$NoticesPath = Join-Path $RepoRoot 'THIRD-PARTY-NOTICES.md'

if (-not $OutDir) { $OutDir = Join-Path $RepoRoot 'dist' }
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }
$OutDir = (Resolve-Path $OutDir).Path

# --- version ---------------------------------------------------------------
function Resolve-Version {
    param([string]$Explicit)

    if ($Explicit) { return $Explicit }
    if ($env:GOBLE_VERSION) { return $env:GOBLE_VERSION }

    $versionEnv = Join-Path $PackagingDir 'version.env'
    if (Test-Path $versionEnv) {
        foreach ($line in Get-Content $versionEnv) {
            if ($line -match '^\s*VERSION\s*=\s*"?([0-9]+\.[0-9]+\.[0-9]+[^"\s]*)"?\s*$') {
                return $Matches[1]
            }
        }
    }
    Write-Warn 'No version supplied and packaging/version.env has no VERSION; using 0.0.0'
    return '0.0.0'
}
$Version = Resolve-Version -Explicit $Version

# --- source binary ---------------------------------------------------------
if (-not $SourceBinary) {
    $SourceBinary = Join-Path $RepoRoot 'target\release\goble-app.exe'
}
if (-not (Test-Path $SourceBinary)) {
    throw "source binary not found: $SourceBinary`nbuild it first: cargo build --release -p goble-app"
}
$SourceBinary = (Resolve-Path $SourceBinary).Path

foreach ($required in @($IssPath, $EulaPath, $IconPath, $LicensePath)) {
    if (-not (Test-Path $required)) { throw "required file missing: $required" }
}
$hasNotices = Test-Path $NoticesPath
if (-not $hasNotices) { Write-Warn "THIRD-PARTY-NOTICES.md not found; it will be omitted from the installer" }

# --- locate ISCC -----------------------------------------------------------
function Find-Iscc {
    if ($env:INNO_SETUP_ISCC -and (Test-Path $env:INNO_SETUP_ISCC)) {
        return (Resolve-Path $env:INNO_SETUP_ISCC).Path
    }

    $candidates = @()
    if ($env:ProgramFiles) { $candidates += (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe') }
    if (${env:ProgramFiles(x86)}) { $candidates += (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe') }
    if ($env:LOCALAPPDATA) { $candidates += (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe') }
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) { return (Resolve-Path $candidate).Path }
    }

    $registryKeys = @(
        'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1',
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1',
        'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1'
    )
    foreach ($key in $registryKeys) {
        try {
            $location = (Get-ItemProperty -Path $key -ErrorAction Stop).InstallLocation
            if ($location) {
                $iscc = Join-Path $location 'ISCC.exe'
                if (Test-Path $iscc) { return (Resolve-Path $iscc).Path }
            }
        } catch {
            continue
        }
    }

    throw @"
ISCC.exe (Inno Setup 6) not found.
Install it with:  choco install innosetup --version=6.3.3
or download it from https://jrsoftware.org/isdl.php and either add ISCC.exe to
PATH or set INNO_SETUP_ISCC to its full path.
"@
}
$Iscc = Find-Iscc

# The Windows SDK does not put signtool.exe on PATH, so look in its install
# directories as well. Only needed when signing is configured.
function Find-SignTool {
    $cmd = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }

    $roots = @(
        (Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'),
        (Join-Path $env:ProgramFiles 'Windows Kits\10\bin')
    )
    foreach ($root in $roots) {
        if (-not $root -or -not (Test-Path $root)) { continue }
        $found = Get-ChildItem -Path $root -Filter 'signtool.exe' -Recurse -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match '\\x64\\' } |
            Sort-Object FullName -Descending |
            Select-Object -First 1
        if ($found) { return $found.FullName }
    }
    return $null
}

# --- signing ---------------------------------------------------------------
# One argument list, used twice: directly to sign the application executable,
# and as an Inno "SignTool" template so Inno signs the setup engine and the
# uninstaller it generates.
function Get-SignToolArgs {
    $timestampUrl = if ($env:WINDOWS_SIGN_TIMESTAMP_URL) { $env:WINDOWS_SIGN_TIMESTAMP_URL } else { 'http://timestamp.digicert.com' }

    if ($env:WINDOWS_SIGN_CERT_PFX -and $env:WINDOWS_SIGN_CERT_PASSWORD) {
        if (-not (Test-Path $env:WINDOWS_SIGN_CERT_PFX)) {
            throw "WINDOWS_SIGN_CERT_PFX points at a missing file: $($env:WINDOWS_SIGN_CERT_PFX)"
        }
        return @('sign', '/f', $env:WINDOWS_SIGN_CERT_PFX, '/p', $env:WINDOWS_SIGN_CERT_PASSWORD,
                 '/fd', 'sha256', '/tr', $timestampUrl, '/td', 'sha256')
    }
    if ($env:WINDOWS_SIGN_CERT_THUMBPRINT) {
        return @('sign', '/sha1', $env:WINDOWS_SIGN_CERT_THUMBPRINT,
                 '/fd', 'sha256', '/tr', $timestampUrl, '/td', 'sha256')
    }
    return $null
}

function Get-SignToolCommand {
    # Not $args: that is a PowerShell automatic variable.
    $toolArgs = Get-SignToolArgs
    if (-not $toolArgs) { return $null }
    # Inno substitutes $f with each file it signs, so the template is a command
    # line: quote every argument that could contain spaces.
    $quoted = $toolArgs | ForEach-Object { if ($_ -match '\s') { '"' + $_ + '"' } else { $_ } }
    return "$SignToolExe $($quoted -join ' ') `$f"
}

function Test-SignedFile {
    param([Parameter(Mandatory)][string]$Path)
    Write-Step "Verifying signature: $(Split-Path -Leaf $Path)"
    & $SignToolExe verify /pa /v $Path
    if ($LASTEXITCODE -ne 0) {
        throw "signature verification failed for $Path"
    }
}

$signArgs = $null
if (-not $SkipSign) { $signArgs = Get-SignToolArgs }

$SignToolExe = $null
if ($signArgs) {
    $SignToolExe = Find-SignTool
    if (-not $SignToolExe) {
        throw @"
WINDOWS_SIGN_* is set but signtool.exe was not found.

Install the Windows SDK (it ships in the "Windows SDK Signing Tools" component)
or add signtool.exe to PATH.
"@
    }
}
$signCommand = if ($signArgs) { Get-SignToolCommand } else { $null }

# A release is signed or it is not a release: an unsigned installer is flagged
# by SmartScreen, and the app it installs would carry no publisher at all.
$RequireSigning = ($env:GOBLE_REQUIRE_SIGNING -eq '1')
if (-not $signCommand -and $RequireSigning) {
    throw @"
GOBLE_REQUIRE_SIGNING=1 but no Windows signing certificate is configured.

Set WINDOWS_SIGN_CERT_PFX + WINDOWS_SIGN_CERT_PASSWORD (and optionally
WINDOWS_SIGN_TIMESTAMP_URL), or WINDOWS_SIGN_CERT_THUMBPRINT for a certificate
already in the user's store. See packaging/README.md, section Signing.

To build an unsigned artifact on purpose, leave GOBLE_REQUIRE_SIGNING unset (or
set it to 0) and pass -SkipSign.
"@
}
if ($signCommand) {
    Write-Step 'Signing enabled (WINDOWS_SIGN_* is set)'
} else {
    Write-Warn 'WINDOWS_SIGN_* is not set (or -SkipSign was passed): building an UNSIGNED installer.'
}

# --- stage the compile directory -------------------------------------------
$stage = Join-Path ([System.IO.Path]::GetTempPath()) ("goble-iss-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage -Force | Out-Null

try {
    Copy-Item $IssPath (Join-Path $stage 'goble.iss') -Force

    $eula = Get-Content -Raw -Path $EulaPath
    $eula = $eula.Replace('@VERSION@', $Version).Replace('@DATE@', (Get-Date -Format 'yyyy-MM-dd'))
    $utf8Bom = New-Object System.Text.UTF8Encoding($true)
    [System.IO.File]::WriteAllText((Join-Path $stage 'EULA.txt'), $eula, $utf8Bom)

    # Sign the application executable before it is packaged, so the file users
    # end up running carries the publisher too, not just the installer around
    # it. The copy is signed rather than target/release/'s file, so a build
    # output is never modified in place.
    $SourceToPackage = $SourceBinary
    if ($signArgs) {
        $SourceToPackage = Join-Path $stage (Split-Path -Leaf $SourceBinary)
        Copy-Item $SourceBinary $SourceToPackage -Force
        Write-Step "Signing application binary: $(Split-Path -Leaf $SourceToPackage)"
        & $SignToolExe @signArgs $SourceToPackage
        if ($LASTEXITCODE -ne 0) {
            throw "signtool failed to sign $SourceToPackage (exit $LASTEXITCODE)"
        }
        Test-SignedFile $SourceToPackage
    }

    $isccArgs = @(
        '/Q'
        "/DMyAppVersion=$Version"
        "/DMyAppName=$AppName"
        "/DSourceBinary=$SourceToPackage"
        "/DOutDir=$OutDir"
        "/DAppIconPath=$IconPath"
        "/DRepoLicense=$LicensePath"
    )
    if ($hasNotices) { $isccArgs += "/DNoticesSource=$NoticesPath" } else { $isccArgs += '/DNoNotices' }
    if ($Arch -eq 'arm64') { $isccArgs += '/DARM64_BUILD' }

    if ($signCommand) {
        $isccArgs += '/DSIGN_TOOL'
        $isccArgs += "/Ssigntool=$signCommand"
    }

    $isccArgs += (Join-Path $stage 'goble.iss')

    Write-Step "Compiling installer with $Iscc"
    & $Iscc @isccArgs
    if ($LASTEXITCODE -ne 0) {
        throw "ISCC.exe exited with code $LASTEXITCODE"
    }
} finally {
    Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
}

$expected = Join-Path $OutDir "$AppName-$Version-win-$Arch-setup.exe"
if (-not (Test-Path $expected)) {
    throw "ISCC reported success but the expected installer was not found: $expected"
}

# Inno signs the setup engine and the uninstaller from the /Ssigntool template;
# check it happened rather than trusting the exit code.
if ($signArgs) { Test-SignedFile $expected }

$info = Get-Item $expected
Write-Step ("Installer ready: {0} ({1:N0} bytes)" -f $info.FullName, $info.Length)
