import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { resolve } from 'path';

export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'happy-dom',
    globals: true,
    setupFiles: ['./src/tests/setup.ts'],
  },
  resolve: {
    alias: {
      '@tauri-apps/api/core': resolve(__dirname, 'src/__mocks__/tauri-core.ts'),
      '@tauri-apps/api/event': resolve(__dirname, 'src/__mocks__/tauri-event.ts'),
    },
  },
});
