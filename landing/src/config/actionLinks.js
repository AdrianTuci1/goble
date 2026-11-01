export const githubUrl = import.meta.env.VITE_GITHUB_URL || 'https://github.com';
export const downloadUrl = import.meta.env.VITE_DOWNLOAD_URL || '#download';

export const downloadLinks = {
  mac: {
    universal: import.meta.env.VITE_DOWNLOAD_MAC_UNIVERSAL || '',
    arm64: import.meta.env.VITE_DOWNLOAD_MAC_ARM64 || '',
    x64: import.meta.env.VITE_DOWNLOAD_MAC_X64 || ''
  },
  windows: {
    x64: import.meta.env.VITE_DOWNLOAD_WINDOWS_X64 || '',
    arm64: import.meta.env.VITE_DOWNLOAD_WINDOWS_ARM64 || ''
  },
  linux: {
    debX64: import.meta.env.VITE_DOWNLOAD_LINUX_DEB_X64 || '',
    debArm64: import.meta.env.VITE_DOWNLOAD_LINUX_DEB_ARM64 || '',
    rpmX64: import.meta.env.VITE_DOWNLOAD_LINUX_RPM_X64 || '',
    rpmArm64: import.meta.env.VITE_DOWNLOAD_LINUX_RPM_ARM64 || '',
    tarX64: import.meta.env.VITE_DOWNLOAD_LINUX_TAR_X64 || '',
    tarArm64: import.meta.env.VITE_DOWNLOAD_LINUX_TAR_ARM64 || '',
    appImageX64: import.meta.env.VITE_DOWNLOAD_LINUX_APPIMAGE_X64 || '',
    appImageArm64: import.meta.env.VITE_DOWNLOAD_LINUX_APPIMAGE_ARM64 || ''
  }
};

const isExternalUrl = (url) => /^https?:\/\//.test(url);

export const getActionLinkProps = (url) => {
  if (!url || !isExternalUrl(url)) {
    return { href: url || '#' };
  }

  return {
    href: url,
    target: '_blank',
    rel: 'noopener noreferrer',
  };
};

