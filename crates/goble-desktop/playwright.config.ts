import { defineConfig, devices } from '@playwright/test';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const isCI = !!process.env.CI;
const useRealBackend = !!process.env.GOBLE_E2E_REAL_BACKEND;

export default defineConfig({
  testDir: './e2e/specs',
  outputDir: './e2e/results',
  timeout: useRealBackend ? 120000 : 30000,
  expect: {
    timeout: 10000,
  },
  fullyParallel: !useRealBackend,
  forbidOnly: isCI,
  retries: isCI ? 2 : 0,
  workers: useRealBackend ? 1 : undefined,
  reporter: [['list'], ['html', { outputFolder: './e2e/report' }]],
  use: {
    baseURL: 'http://localhost:1450',
    trace: 'on-first-retry',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chromium'] },
    },
  ],
  webServer: useRealBackend
    ? undefined
    : {
        command: 'npm run e2e:serve',
        url: 'http://localhost:1450/health',
        reuseExistingServer: !isCI,
        timeout: 120000,
        env: {
          GOBLE_E2E_MOCK: '1',
          GOBLE_E2E_DIST: resolve(__dirname, 'dist'),
        },
      },
});
