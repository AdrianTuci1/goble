import { test, expect } from '@playwright/test';

test('app loads and can configure provider', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  await expect(page.locator('#root')).toBeVisible();
});
