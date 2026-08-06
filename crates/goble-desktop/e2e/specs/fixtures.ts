import { test as base, expect, type Page } from '@playwright/test';

export type TestFixtures = {
  goblePage: ReturnType<typeof createGoblePage>;
};

export const goblePageFixture = {
  async goto(page: Page, path: string = '/') {
    await page.goto(path, { waitUntil: 'domcontentloaded' });
    await page.waitForSelector('#root', { timeout: 10000 });
    await page.waitForTimeout(500);
  },

  async sendChat(page: Page, text: string) {
    const composer = page.locator('.composer-input');
    await composer.fill(text);
    await composer.press('Enter');
  },

  async expectCustomCard(page: Page, kind: 'variant' | 'form' | 'secret') {
    const testId = kind === 'variant' ? 'variant-card' : kind === 'form' ? 'form-card' : 'secret-card';
    await expect(page.locator(`[data-testid="${testId}"]`).first()).toBeVisible({ timeout: 15000 });
  },

  async openSettings(page: Page, section: string) {
    await page.goto(`/settings/${section}`);
    await page.waitForURL(`/settings/${section}`);
  },

  async configureProvider(page: Page, opts: { schema: string; name: string; apiKey: string; baseUrl?: string; model: string; alias?: string }) {
    await page.goto('/settings/providers', { waitUntil: 'domcontentloaded' });
    await page.waitForURL('/settings/providers');
    const modalAlreadyOpen = await page.locator('.modal-overlay').isVisible().catch(() => false);
    if (!modalAlreadyOpen) {
      await page.click('button:has-text("Add model")');
    }
    await page.waitForSelector('.modal-overlay');
    await page.selectOption('.modal-field select', opts.schema);
    await page.locator('.modal-field').filter({ hasText: 'Endpoint name' }).locator('input').fill(opts.name);
    if (opts.baseUrl) {
      await page.locator('.modal-field').filter({ hasText: 'Endpoint URL' }).locator('input').fill(opts.baseUrl);
    }
    await page.locator('.modal-field').filter({ hasText: 'API key' }).locator('input').fill(opts.apiKey);
    await page.locator('.modal-model-row').first().locator('.modal-model-input').first().fill(opts.model);
    if (opts.alias) {
      await page.locator('.modal-model-row').first().locator('.modal-model-input').nth(1).fill(opts.alias);
    }
    await page.click('button:has-text("Add endpoint")');
    await expect(page.locator('.provider-card').filter({ hasText: opts.name })).toBeVisible();
  },

  async expectAssistantText(page: Page, text: string) {
    const locator = page.locator('.message.assistant .message-content').filter({ hasText: new RegExp(text, 'i') });
    await expect(locator.first()).toBeVisible({ timeout: 15000 });
  },

  async resetState(page: Page) {
    await page.goto('/');
    await page.waitForSelector('#root', { timeout: 10000 });
    await page.evaluate(() => {
      localStorage.clear();
      sessionStorage.clear();
    });
    await page.evaluate(() => (window as any).__goble_e2e_invoke__('reset_state', {}));
  },
};

function createGoblePage(page: Page) {
  return {
    page,
    async goto(path: string = '/') {
      await goblePageFixture.goto(page, path);
    },
    async sendChat(text: string) {
      await goblePageFixture.sendChat(page, text);
    },
    async expectAssistantText(text: string) {
      await goblePageFixture.expectAssistantText(page, text);
    },
    async expectCustomCard(kind: 'variant' | 'form' | 'secret') {
      await goblePageFixture.expectCustomCard(page, kind);
    },
    async openSettings(section: string) {
      await goblePageFixture.openSettings(page, section);
    },
    async configureProvider(opts: { schema: string; name: string; apiKey: string; baseUrl?: string; model: string; alias?: string }) {
      await goblePageFixture.configureProvider(page, opts);
    },
    async resetState() {
      await goblePageFixture.resetState(page);
    },
  };
}

export const test = base.extend<TestFixtures>({
  goblePage: async ({ page }, use) => {
    await use(createGoblePage(page));
  },
});

export { expect };
