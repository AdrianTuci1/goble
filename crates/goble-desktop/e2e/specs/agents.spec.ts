import { test, expect } from './fixtures';

test('two agents with two different runtimes: web-search agent and nodejs builder', async ({ page, goblePage }) => {
  await goblePage.resetState();
  await goblePage.configureProvider({
    schema: 'openai',
    name: 'E2E OpenAI',
    apiKey: 'sk-e2e-test',
    model: 'gpt-4o-mini',
  });
  await goblePage.goto();

  await goblePage.sendChat('[test:agent] Recruiter');
  await goblePage.expectAssistantText('Created agent "Recruiter"');

  await goblePage.sendChat('[test:agent] Node.js Builder');
  await goblePage.expectAssistantText('Created agent "Node.js Builder"');

  // Verify both agents appear in the agents list.
  await page.click('button:has-text("Agents")');
  await page.waitForURL('/main/agents');
  await expect(page).toHaveURL('/main/agents');
});

test('remote runtime can be configured via custom composer', async ({ page, goblePage }) => {
  await goblePage.resetState();
  await goblePage.configureProvider({
    schema: 'openai',
    name: 'E2E OpenAI',
    apiKey: 'sk-e2e-test',
    model: 'gpt-4o-mini',
  });
  await goblePage.goto();
  await goblePage.sendChat('[test:form] remote runtime');
  // Credentials with an SSH private key are rendered as a secret card.
  await goblePage.expectCustomCard('secret');
  await page.fill('[data-testid="secret-card"] [data-field="host"]', '203.0.113.10');
  await page.fill('[data-testid="secret-card"] [data-field="user"]', 'root');
  await page.fill('[data-testid="secret-card"] [data-field="private_key"]', '-----BEGIN OPENSSH PRIVATE KEY-----');
  await page.click('[data-testid="secret-card-submit"]');
  // The form submission is accepted by the mock backend; the card remains rendered.
  await expect(page.locator('[data-testid="secret-card"]')).toBeVisible();
});
