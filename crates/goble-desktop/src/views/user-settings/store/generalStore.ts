import { create } from 'zustand';
import { persist } from 'zustand/middleware';

const ADJECTIVES = [
  'amber', 'brisk', 'calm', 'clear', 'curly', 'daring', 'eager', 'fancy', 'gentle', 'happy',
  'honey', 'jolly', 'kind', 'lively', 'lucky', 'merry', 'mild', 'noble', 'open', 'proud',
  'quick', 'quiet', 'rapid', 'royal', 'sharp', 'silent', 'sleek', 'smart', 'sunny', 'swift',
  'tender', 'vivid', 'warm', 'wild', 'wise', 'witty', 'young', 'zesty',
];

const NOUNS = [
  'anchor', 'bloom', 'brook', 'canyon', 'cinder', 'coral', 'crisp', 'dawn', 'dune', 'falcon',
  'flame', 'frost', 'grove', 'harbor', 'hollow', 'meadow', 'mirth', 'nebula', 'oasis', 'ocean',
  'orbit', 'pebble', 'plume', 'ripple', 'sage', 'shell', 'spark', 'spruce', 'storm', 'summit',
  'swan', 'thistle', 'tide', 'violet', 'wander', 'willow', 'wren', 'zenith',
];

function randomInt(max: number): number {
  return Math.floor(Math.random() * max);
}

export function generateRandomName(): string {
  const adjective = ADJECTIVES[randomInt(ADJECTIVES.length)];
  const noun = NOUNS[randomInt(NOUNS.length)];
  const digits = `${randomInt(10)}${randomInt(10)}${randomInt(10)}`;
  return `${adjective}${noun}${digits}`;
}

export interface GeneralState {
  displayName: string;
  email: string;
  avatarSeed: string;
  setDisplayName: (name: string) => void;
  setEmail: (email: string) => void;
  setAvatarSeed: (seed: string) => void;
  getEffectiveName: () => string;
}

export const useGeneralStore = create<GeneralState>()(
  persist(
    (set, get) => ({
      displayName: '',
      email: '',
      avatarSeed: generateRandomName(),

      setDisplayName: (displayName) => set({ displayName }),
      setEmail: (email) => set({ email }),
      setAvatarSeed: (avatarSeed) => set({ avatarSeed }),

      getEffectiveName: () => {
        const { displayName, avatarSeed } = get();
        return displayName.trim() || avatarSeed;
      },
    }),
    {
      name: 'goble-general',
      partialize: (state) => ({
        displayName: state.displayName,
        email: state.email,
        avatarSeed: state.avatarSeed,
      }),
    }
  )
);
