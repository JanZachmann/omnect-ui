import { test, expect } from '@playwright/test';
import { mockConfig, mockRequireSetPassword } from './fixtures/mock-api';

test.describe('Version Mismatch', () => {
  test('shows fullscreen error dialog when omnect-device-service version mismatches', async ({ page }) => {
    await mockConfig(page);
    await mockRequireSetPassword(page);

    // Mock healthcheck with version mismatch
    await page.route('**/healthcheck', async (route) => {
      await route.fulfill({
        status: 503,
        contentType: 'application/json',
        body: JSON.stringify({
          versionInfo: {
            required: '>=0.46.0',
            current: '0.45.2',
            mismatch: true,
          },
          updateValidationStatus: {
            status: 'valid',
          },
          networkRollbackOccurred: false,
        }),
      });
    });

    // Navigate to page
    await page.goto('/');

    // The version mismatch dialog should appear
    await expect(page.getByText('omnect-device-service version mismatch')).toBeVisible({ timeout: 10000 });
    await expect(page.getByText('Current version: 0.45.2')).toBeVisible();
    await expect(page.getByText('Required version >=0.46.0')).toBeVisible();
    await expect(page.getByText('Please consider to update omnect Secure OS')).toBeVisible();

    // Dialog should be persistent (no close button, can't dismiss)
    // The dialog is fullscreen and blocks the entire UI
    await expect(page.locator('.v-dialog--fullscreen')).toBeVisible();
  });

  test('does not show error dialog when version matches', async ({ page }) => {
    await mockConfig(page);
    await mockRequireSetPassword(page);

    // Mock healthcheck with matching version
    await page.route('**/healthcheck', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          versionInfo: {
            required: '>=0.46.0',
            current: '0.46.0',
            mismatch: false,
          },
          updateValidationStatus: {
            status: 'valid',
          },
          networkRollbackOccurred: false,
        }),
      });
    });

    // Navigate to page
    await page.goto('/');

    // Wait for the login form to appear (indicates no error dialog blocking)
    await expect(page.getByPlaceholder(/enter your password/i)).toBeVisible({ timeout: 10000 });

    // The version mismatch dialog should NOT appear
    await expect(page.getByText('omnect-device-service version mismatch')).not.toBeVisible();
  });
});
