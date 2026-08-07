import { test, expect } from './fixtures';

test('install and authenticate an MCP server from chat via preset test prompt', async ({ page, goblePage }) => {
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

test('search, install and authenticate an MCP server from natural language', async ({ page, goblePage }) => {
  await goblePage.resetState();
  await goblePage.configureProvider({
    schema: 'openai',
    name: 'E2E OpenAI',
    apiKey: 'sk-e2e-test',
    model: 'gpt-4o-mini',
  });
  await goblePage.goto();

  await goblePage.sendChat('find mcp github');
  // Wait for an AssistantDelta to appear before expecting the install prompt variant card.
  await goblePage.expectAssistantText('Found');

  // Assistant asks to install via quick reply card.
  await goblePage.expectCustomCard('variant');
  await expect(page.locator('[data-testid="variant-card"]')).toContainText('Install');
  await page.click('[data-testid="variant-card"] button:has-text("Install")');

  // After clicking Install, a new assistant message (secretCard) should appear in the same chat.
  await expect(page.locator('[data-testid="secret-card"]')).toBeVisible({ timeout: 15000 });
  await expect(page.locator('[data-testid="secret-card"]')).toContainText('API key');
  await page.fill('[data-testid="secret-card"] [data-field="api_key"]', 'ghp-nl-test-key');
  await page.fill('[data-testid="secret-card"] [data-field="scope"]', 'repo');
  await page.click('[data-testid="secret-card-submit"]');

  await goblePage.expectAssistantText('Installed');

  await page.goto('/main/connectors');
  await page.waitForURL('/main/connectors');
  await expect(page.locator('.mcp-installed-card')).toHaveCount(1);
  await expect(page.locator('.mcp-installed-card')).toContainText('github');
});

test('uninstall an MCP server from the connectors page', async ({ page, goblePage }) => {
  await goblePage.resetState();
  await goblePage.configureProvider({
    schema: 'openai',
    name: 'E2E OpenAI',
    apiKey: 'sk-e2e-test',
    model: 'gpt-4o-mini',
  });
  await goblePage.goto();

  await goblePage.sendChat('[test:mcp] slack');
  await goblePage.expectCustomCard('secret');
  await page.fill('[data-testid="secret-card"] [data-field="api_key"]', 'xoxb-test-token');
  await page.fill('[data-testid="secret-card"] [data-field="scope"]', 'chat:write');
  await page.click('[data-testid="secret-card-submit"]');
  await goblePage.expectAssistantText('Installed');

  await page.goto('/main/connectors');
  await page.waitForURL('/main/connectors');
  await expect(page.locator('.mcp-installed-card')).toHaveCount(1);

  page.on('dialog', (dialog) => dialog.accept());
  await page.click('.mcp-installed-card button:has-text("Manage")');
  await expect(page.locator('[data-testid="delete-mcp-drawer-button"]')).toBeVisible();
  await page.click('[data-testid="delete-mcp-drawer-button"]');
  await page.waitForTimeout(300);
  await expect(page.locator('.mcp-installed-card')).toHaveCount(0);
});
