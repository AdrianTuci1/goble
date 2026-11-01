import { useState, useEffect } from 'react';

/**
 * Read the CPU architecture from a user agent string. Browsers only expose it
 * on some platforms, so this is the fallback after the userAgentData API.
 */
const archFromUserAgent = (userAgent) => {
  if (/aarch64|arm64/.test(userAgent)) return 'arm64';
  if (/x86_64|win64|wow64|amd64|x64/.test(userAgent)) return 'x64';
  return 'unknown';
};

/**
 * The architecture to assume when the browser reports none. Apple Silicon is
 * the default Mac sold today; x64 is the default everywhere else.
 */
const defaultArch = (os) => (os === 'mac' ? 'arm64' : 'x64');

/**
 * Custom hook to detect the user's operating system, CPU architecture, and
 * mobile status. Combines modern navigator.userAgentData API with legacy
 * navigator.platform/userAgent checks.
 */
export const useDetectOS = () => {
  const [os, setOS] = useState('unknown'); // 'mac' | 'windows' | 'linux' | 'mobile' | 'unknown'
  const [arch, setArch] = useState('unknown'); // 'arm64' | 'x64' | 'unknown'
  const [isMobile, setIsMobile] = useState(false);

  useEffect(() => {
    const detect = async () => {
      let detectedOS = 'unknown';
      let detectedArch = 'unknown';
      let detectedMobile = false;

      // 1. Check navigator.userAgentData if available (modern browser standard)
      if (navigator.userAgentData) {
        try {
          const highEntropyValues = await navigator.userAgentData.getHighEntropyValues([
            'platform',
            'mobile',
            'architecture',
            'bitness'
          ]);
          const platform = (highEntropyValues.platform || '').toLowerCase();
          const architecture = (highEntropyValues.architecture || '').toLowerCase();
          detectedMobile = highEntropyValues.mobile || false;

          if (architecture === 'arm') {
            detectedArch = 'arm64';
          } else if (architecture === 'x86') {
            detectedArch = 'x64';
          }

          if (detectedMobile) {
            detectedOS = 'mobile';
          } else if (platform.includes('mac') || platform.includes('ios') || platform.includes('ipod') || platform.includes('ipad')) {
            detectedOS = 'mac';
          } else if (platform.includes('win')) {
            detectedOS = 'windows';
          } else if (platform.includes('linux')) {
            detectedOS = 'linux';
          } else if (platform.includes('android')) {
            detectedOS = 'mobile';
          }
        } catch (e) {
          // Fallback if promise fails or is blocked
        }
      }

      // 2. Fallback to standard User Agent and Platform check if unknown or not fully resolved
      if (detectedOS === 'unknown') {
        const userAgent = window.navigator.userAgent.toLowerCase();
        const platform = window.navigator.platform?.toLowerCase() || '';

        // Mobile / Tablet detection
        const isMobileUA = /iphone|ipad|ipod|android|webos|blackberry|iemobile|opera mini/i.test(userAgent);
        // iPad Pro on Safari pretends to be macOS desktop but has multiple touch points
        const isIPadSafari = (platform.includes('mac') || platform === 'macintel') && navigator.maxTouchPoints > 1;

        detectedMobile = isMobileUA || isIPadSafari;

        if (detectedMobile) {
          detectedOS = 'mobile';
        } else if (/mac|ipad|iphone|ipod/.test(platform) || /macintosh|mac os x/.test(userAgent)) {
          detectedOS = 'mac';
        } else if (/win/.test(platform) || /windows/.test(userAgent)) {
          detectedOS = 'windows';
        } else if (/linux/.test(platform) || /linux/.test(userAgent)) {
          detectedOS = 'linux';
        }

        if (detectedArch === 'unknown') {
          detectedArch = archFromUserAgent(userAgent);
        }
      }

      if (detectedMobile) {
        detectedArch = 'unknown';
      } else if (detectedArch === 'unknown' && detectedOS !== 'unknown') {
        detectedArch = defaultArch(detectedOS);
      }

      setOS(detectedOS);
      setArch(detectedArch);
      setIsMobile(detectedMobile);
    };

    detect();
  }, []);

  return {
    os,
    arch,
    isMobile,
    osName: os === 'mac' ? 'macOS' : os === 'windows' ? 'Windows' : os === 'linux' ? 'Linux' : os === 'mobile' ? 'Mobile' : 'Unknown'
  };
};
