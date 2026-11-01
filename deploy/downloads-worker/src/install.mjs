// Installer scripts served by the Worker. Both are plain text built from the
// repository and the public base URL; neither carries a secret, and both ask
// GitHub for the latest release tag so the asset names stay versioned without
// the caller knowing the version.

/** Default repository, used when `GITHUB_REPO` is unset. */
export const DEFAULT_REPO = "AdrianTuci1/goble";

/**
 * The POSIX shell installer: picks the macOS disk image or the Linux AppImage
 * for the machine's architecture, downloads it through this Worker, and puts it
 * in place.
 * @param {string} repo `owner/repo`
 * @param {string} base public base URL without a trailing slash
 * @returns {string}
 */
export function shellInstaller(repo, base) {
  return `#!/bin/sh
# Goble installer. Detects the platform, then downloads the matching asset from
# the latest release through ${base}.
set -eu

REPO=${shellQuote(repo)}
BASE=${shellQuote(base)}

say() { printf '%s\\n' "$*"; }
die() { printf 'error: %s\\n' "$*" >&2; exit 1; }

os=$(uname -s)
machine=$(uname -m)
case "$machine" in
    arm64|aarch64) arch=arm64 ;;
    x86_64|amd64) arch=x64 ;;
    *) die "unsupported architecture: $machine" ;;
esac

tag=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \\
    | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\\([^"]*\\)".*/\\1/p' \\
    | head -n 1)
[ -n "$tag" ] || die "no published release was found for $REPO"
version=$(printf '%s' "$tag" | sed 's/^v//')

case "$os:$arch" in
    Darwin:arm64) asset="Goble-$version-arm64.dmg" ;;
    Darwin:x64)   asset="Goble-$version-x64.dmg" ;;
    Linux:x64)    asset="Goble-$version-x86_64.AppImage" ;;
    Linux:arm64)  asset="Goble-$version-aarch64.AppImage" ;;
    *) die "unsupported platform: $os $machine" ;;
esac

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

say "Downloading $asset"
curl -fL --progress-bar "$BASE/dl/latest/$asset" -o "$tmp/$asset"

if [ "$os" = Darwin ]; then
    mount="$tmp/mount"
    mkdir -p "$mount" "$HOME/Applications"
    hdiutil attach -nobrowse -quiet "$tmp/$asset" -mountpoint "$mount"
    cp -R "$mount/Goble.app" "$HOME/Applications/"
    hdiutil detach -quiet "$mount"
    say "Goble is installed at $HOME/Applications/Goble.app"
    say "Open it from Applications, or run: open -a Goble"
else
    bin_dir=\${XDG_BIN_HOME:-"$HOME/.local/bin"}
    mkdir -p "$bin_dir"
    mv "$tmp/$asset" "$bin_dir/goble"
    chmod 0755 "$bin_dir/goble"
    say "Goble is installed at $bin_dir/goble"
    case ":$PATH:" in
        *":$bin_dir:"*) ;;
        *) say "Add $bin_dir to PATH to run: goble" ;;
    esac
fi
`;
}

/**
 * The PowerShell installer for Windows: runs the per-user Inno Setup
 * executable for the machine's architecture with the flags the updater uses.
 * @param {string} repo `owner/repo`
 * @param {string} base public base URL without a trailing slash
 * @returns {string}
 */
export function powershellInstaller(repo, base) {
  return `# Goble installer for Windows. Detects the architecture, then runs the setup
# executable from the latest release through ${base}.
$ErrorActionPreference = 'Stop'

$repo = '${powershellQuote(repo)}'
$base = '${powershellQuote(base)}'
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'arm64' } else { 'x64' }

$headers = @{ 'User-Agent' = 'goble-installer' }
$release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/latest" -Headers $headers
if (-not $release.tag_name) { throw "no published release was found for $repo" }

$version = $release.tag_name.TrimStart('v')
$asset = "Goble-$version-win-$arch-setup.exe"
$out = Join-Path $env:TEMP $asset

Write-Host "Downloading $asset"
Invoke-WebRequest -Uri "$base/dl/latest/$asset" -OutFile $out
Start-Process -FilePath $out -ArgumentList '/SILENT', '/NORESTART', '/NOCLOSEAPPLICATIONS' -Wait
Remove-Item -LiteralPath $out -Force
Write-Host "Goble $version is installed."
`;
}

/**
 * Single-quote a value for POSIX shell, so a repository or base URL cannot
 * break out of its assignment.
 * @param {string} value
 * @returns {string}
 */
function shellQuote(value) {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

/**
 * Single-quote a value for PowerShell, where a literal quote is doubled.
 * @param {string} value
 * @returns {string}
 */
function powershellQuote(value) {
  return value.replace(/'/g, "''");
}
