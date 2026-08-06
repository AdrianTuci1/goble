import { test, expect } from './fixtures';

test('custom composer: multi-variant selection is rendered', async ({ page, goblePage }) => {
  await goblePage.resetState();
  await goblePage.configureProvider({
    schema: 'openai',
    name: 'E2E OpenAI',
    apiKey: 'sk-e2e-test',
    model: 'gpt-4o-mini',
  });
  await goblePage.goto();
  await goblePage.sendChat('[test:variant] choose an approach');
  await goblePage.expectCustomCard('variant');
  await expect(page.locator('[data-testid="variant-card"] [data-option="Use web search"]')).toBeVisible();
  await expect(page.locator('[data-testid="variant-card"] [data-option="Use local files"]')).toBeVisible();
  await expect(page.locator('[data-testid="variant-card"] [data-option="Ask for clarification"]')).toBeVisible();
});

test('custom composer: secret/authorization card for MCP is rendered', async ({ page, goblePage }) => {
  await goblePage.resetState();
  await goblePage.configureProvider({
    schema: 'openai',
    name: 'E2E OpenAI',
    apiKey: 'sk-e2e-test',
    model: 'gpt-4o-mini',
  });
  await goblePage.goto();
  await goblePage.sendChat('[test:secret] authorize MCP');
  await goblePage.expectCustomCard('secret');
  await expect(page.locator('[data-testid="secret-card"] [data-field="mcp_id"]')).toBeVisible();
  await expect(page.locator('[data-testid="secret-card"] [data-field="api_key"]')).toBeVisible();
  await expect(page.locator('[data-testid="secret-card"] [data-field="scope"]')).toBeVisible();
  await expect(page.locator('[data-testid="secret-card-submit"]').first()).toBeVisible();
});

test('custom composer: remote runtime credentials card is rendered', async ({ page, goblePage }) => {
  await goblePage.resetState();
  await goblePage.configureProvider({
    schema: 'openai',
    name: 'E2E OpenAI',
    apiKey: 'sk-e2e-test',
    model: 'gpt-4o-mini',
  });
  await goblePage.goto();
  await goblePage.sendChat('[test:form] remote runtime');
  // The UI classifies any card containing password/key fields as a secret card.
  await goblePage.expectCustomCard('secret');
  await expect(page.locator('[data-testid="secret-card"] [data-field="host"]')).toBeVisible();
  await expect(page.locator('[data-testid="secret-card"] [data-field="user"]')).toBeVisible();
  await expect(page.locator('[data-testid="secret-card"] [data-field="private_key"]')).toBeVisible();
});
