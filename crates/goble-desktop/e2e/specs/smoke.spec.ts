import { test, expect } from './fixtures';

test('mock backend responds and chat view loads', async ({ page, goblePage }) => {
  await goblePage.goto();
  await expect(page.locator('#app')).toBeVisible();
  await expect(page.locator('.composer-input')).toBeVisible();
});
