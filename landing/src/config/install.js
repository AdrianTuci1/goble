import { downloadLinks } from './actionLinks';

// One-line installers served by the downloads worker. The shell script and the
// PowerShell script both detect the architecture themselves; the landing only
// picks which one to show for the selected platform.
export const releaseHost = 'https://get.octomus.dev';

export const platformOrder = ['mac', 'linux', 'windows'];

export const platformLabels = {
  mac: 'macOS',
  linux: 'Linux',
  windows: 'Windows'
};

export const installCommands = {
  mac: `curl -fsSL ${releaseHost}/install.sh | sh`,
  linux: `curl -fsSL ${releaseHost}/install.sh | sh`,
  windows: `irm ${releaseHost}/install.ps1 | iex`
};

// The prompt shown in front of the command, per shell.
export const platformPrompts = {
  mac: '$',
  linux: '$',
  windows: '>'
};

/**
 * Narrow a detected platform to one the download page can offer. An unknown or
 * mobile platform falls back to macOS, the first platform listed.
 */
export const normalizePlatform = (os) =>
  platformOrder.includes(os) ? os : 'mac';

/**
 * The download that matches the platform and CPU architecture. Architectures
 * the browser cannot report fall back to the most common one for that
 * platform.
 */
export const primaryDownload = (os, arch) => {
  const arm = arch === 'arm64';
  switch (normalizePlatform(os)) {
    case 'linux':
      return arm ? downloadLinks.linux.debArm64 : downloadLinks.linux.debX64;
    case 'windows':
      return arm ? downloadLinks.windows.arm64 : downloadLinks.windows.x64;
    case 'mac':
    default:
      return (
        (arm ? downloadLinks.mac.arm64 : downloadLinks.mac.x64) ||
        downloadLinks.mac.universal
      );
  }
};

/** Human label for the architecture, used beside a download card. */
export const archLabel = (arch) => (arch === 'arm64' ? 'ARM64' : 'x64');
