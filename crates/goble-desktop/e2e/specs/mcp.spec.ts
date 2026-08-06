import { test, expect } from './fixtures';

test('install and authenticate an MCP server from chat', async ({ page, goblePage }) => {
  await goblePage.resetState();
  await goblePage.configureProvider({
    schema: 'openai',
    name: 'E2E OpenAI',
    apiKey: 'sk-e2e-test',
    model: 'gpt-4o-mini',
  });
  await goblePage.goto();

  await goblePage.sendChat('[test:mcp] github');

  // Assistant proposes the install and asks for credentials via a custom composer card.
  await goblePage.expectCustomCard('secret');
  await expect(page.locator('[data-testid="secret-card"]')).toContainText('Install github MCP server');

  // Fill credentials in the chat composer card.
  await page.fill('[data-testid="secret-card"] [data-field="api_key"]', 'ghp-e2e-test-key');
  await page.fill('[data-testid="secret-card"] [data-field="scope"]', 'repo');
  await page.click('[data-testid="secret-card-submit"]');

  // Assistant confirms installation.
  await goblePage.expectAssistantText('Installed github MCP server');

  // The installed server shows up on the Connectors page.
  await page.goto('/main/connectors');
  await page.waitForURL('/main/connectors');
  await expect(page.locator('.mcp-installed-card')).toHaveCount(1);
  await expect(page.locator('.mcp-installed-card')).toContainText('github');
  await expect(page.locator('.mcp-installed-card button:has-text("Manage")')).toBeVisible();
});
