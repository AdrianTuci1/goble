import { test, expect } from './fixtures';

test('onboarding: missing model shows configure link', async ({ page, goblePage }) => {
  await goblePage.resetState();
  await goblePage.goto();

  await goblePage.sendChat('What can you help me with?');
  await expect(page.locator('.message.system').filter({ hasText: /don't have a model configured/i }).first()).toBeVisible();
  await expect(page.locator('.configure-link').filter({ hasText: /click here/i })).toBeVisible();
});

test('onboarding: configuring a provider enables assistant replies', async ({ page, goblePage }) => {
  await goblePage.resetState();
  await goblePage.goto();
  await goblePage.sendChat('What can you help me with?');
  await expect(page.locator('.message.system').filter({ hasText: /don't have a model configured/i }).first()).toBeVisible();

  await goblePage.openSettings('providers');
  await goblePage.configureProvider({
    schema: 'openai',
    name: 'E2E OpenAI',
    apiKey: 'sk-e2e-test',
    model: 'gpt-4o-mini',
    alias: 'E2E Model',
  });

  // After setup the chat can receive a real assistant response (behaviour, not exact wording).
  await goblePage.goto();
  await goblePage.sendChat('[test:hello]');
  await goblePage.expectAssistantText('Hello!');
  await expect(page.locator('.message.system').filter({ hasText: /don't have a model configured/i }).first()).toHaveCount(0);
});
