import { test, expect } from '@playwright/test';
import { setupAndLogin } from './fixtures/test-setup';
import { mockPortalAuth, mockSetPasswordSuccess } from './fixtures/mock-api';
import { publishToWebsocket } from './fixtures/websocket';

test.describe('Device Factory Reset', () => {
  test.beforeEach(async ({ page }) => {
    await setupAndLogin(page);
  });

  test('user can initiate factory reset from the device actions menu', async ({ page }) => {
    // Mock the factory-reset endpoint
    let resetCalled = false;
    await page.route('**/factory-reset', async (route) => {
      resetCalled = true;
      const request = route.request();
      const postData = await request.postDataJSON();
      // Verify payload
      expect(postData.mode).toBe(1);
      expect(postData.preserve).toEqual([]);
      await route.fulfill({ status: 200, body: '' });
    });

    // Locate and click the Factory Reset button (it's in DeviceActions)
    const resetBtn = page.getByRole('button', { name: 'Factory Reset' }).first(); 
    await resetBtn.click();

    // Verify dialog appears
    await expect(page.getByText('Factory reset', { exact: true })).toBeVisible();
    
    // Click Reset in the dialog
    const dialog = page.getByRole('dialog');
    const confirmBtn = dialog.getByRole('button', { name: 'Reset' });
    await confirmBtn.click();

    // Verify API call
    await page.waitForTimeout(100);
    expect(resetCalled).toBe(true);

    // Verify UI feedback
    await expect(page.getByText('The device is resetting')).toBeVisible({ timeout: 10000 });
  });

  test('user can cancel the factory reset dialog', async ({ page }) => {
    // Open dialog
    const resetBtn = page.getByRole('button', { name: 'Factory Reset' }).first(); 
    await resetBtn.click();

    // Verify dialog appears
    await expect(page.getByText('Factory reset', { exact: true })).toBeVisible();

    // Click Cancel
    const cancelBtn = page.getByRole('button', { name: 'Cancel' });
    await cancelBtn.click();

    // Verify dialog disappears
    await expect(page.getByText('Factory reset', { exact: true })).not.toBeVisible();
    
    await expect(page.getByText('The device is resetting')).not.toBeVisible();
  });

  test('displays timeout message when device does not come back online', async ({ page }) => {
    // Mock the factory-reset endpoint
    await page.route('**/factory-reset', async (route) => {
      await route.fulfill({ status: 200, body: '' });
    });

    // Mock healthcheck to ALWAYS fail (simulating offline device)
    await page.route('**/healthcheck', async (route) => {
      await route.abort('failed');
    });

    // Initiate factory reset
    const resetBtn = page.getByRole('button', { name: 'Factory Reset' }).first(); 
    await resetBtn.click();
    
    const confirmBtn = page.getByRole('dialog').getByRole('button', { name: 'Reset' });
    await confirmBtn.click();

    // Verify initial state
    await expect(page.getByText('The device is resetting')).toBeVisible();

    // Wait for Core-driven timeout (capped to 500ms by VITE_RECONNECTION_POLL_INTERVAL_MS TIMER_CAP_MS)
    await expect(page.getByText('Device did not come back online. You may need to re-accept the security certificate.')).toBeVisible({ timeout: 4000 });
  });
});

test.describe('Device Factory Reset - Reconnection', () => {
  test('device returns online after factory reset, prompts set-password, and shows success modal', async ({ page }) => {
    // factoryResetResultAcked: false prevents sessionStorage suppression, so the factory reset
    // modal can fire when the WebSocket message arrives after reconnection.
    // mockPortalAuth must be called before setupAndLogin so its addInitScript populates
    // localStorage on page.goto('/'), satisfying the requiresPortalAuth guard on /set-password.
    await mockPortalAuth(page);
    await setupAndLogin(page, { factoryResetResultAcked: false });

    await page.route('**/factory-reset', async (route) => {
      await route.fulfill({ status: 200, body: '' });
    });

    // After factory reset the password is cleared → require-set-password returns true.
    await page.route('**/require-set-password', async (route) => {
      await route.fulfill({ status: 200, contentType: 'application/json', body: 'true' });
    });

    // Stateful healthcheck: first call fails (device offline), subsequent succeed (back online).
    let healthcheckCount = 0;
    await page.route('**/healthcheck', async (route) => {
      if (route.request().method() !== 'GET') { await route.continue(); return; }
      healthcheckCount++;
      if (healthcheckCount <= 1) {
        await route.abort('failed');
      } else {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            versionInfo: { current: '1.0.0', required: '1.0.0', mismatch: false },
            updateValidationStatus: { status: 'NoUpdate' },
            networkRollbackOccurred: false,
          })
        });
      }
    });

    // Mock set-password so the user can authenticate after resetting.
    await mockSetPasswordSuccess(page);

    // Mock ack-factory-reset-result (called when user dismisses the modal).
    await page.route('**/ack-factory-reset-result', async (route) => {
      await route.fulfill({ status: 200 });
    });

    await page.getByRole('button', { name: 'Factory Reset' }).first().click();
    await page.getByRole('dialog').getByRole('button', { name: 'Reset' }).click();

    await expect(page.getByText('The device is resetting')).toBeVisible({ timeout: 10000 });

    // Session invalidated → /login → requiresPasswordSet=true → /set-password.
    await expect(page).toHaveURL(/\/set-password/, { timeout: 15000 });
    await expect(page.getByRole('heading', { name: 'Set Password' })).toBeVisible({ timeout: 5000 });

    // Set new password → authenticate → dashboard.
    await page.locator('input[type="password"]').nth(0).fill('new-password');
    await page.locator('input[type="password"]').nth(1).fill('new-password');
    await page.getByRole('button', { name: /set password/i }).click();
    await expect(page.getByText('Common Info')).toBeVisible({ timeout: 10000 });

    // ODS publishes the factory reset result via WebSocket after republishing.
    // status=0 is Success; error and context are null when nothing went wrong.
    await publishToWebsocket('FactoryResetV1', {
      keys: ['network'],
      result: { status: 0, error: null, context: null, paths: ['network'], data_wiped: true },
    });

    await expect(page.getByText('Factory Reset Completed', { exact: true })).toBeVisible({ timeout: 5000 });
    await expect(page.getByText('The factory reset completed successfully.')).toBeVisible();

    await page.getByRole('button', { name: 'OK' }).click();
    await expect(page.getByText('Factory Reset Completed', { exact: true })).not.toBeVisible();
  });

  test('success modal title does not flash to error state when dismissed', async ({ page }) => {
    await mockPortalAuth(page);
    await setupAndLogin(page, { factoryResetResultAcked: false });

    await page.route('**/ack-factory-reset-result', async (route) => {
      await route.fulfill({ status: 200 });
    });

    // Trigger the factory reset success modal via WebSocket (status: 0 = Success)
    await publishToWebsocket('FactoryResetV1', {
      keys: ['network'],
      result: { status: 0, error: null, context: null, paths: ['network'], data_wiped: true },
    });

    await expect(page.getByText('Factory Reset Completed', { exact: true })).toBeVisible({ timeout: 5000 });

    // Install mutation observer BEFORE clicking OK to catch the flash during close animation
    await page.evaluate(() => {
      (window as any).__factoryResetErrorFlash = false;
      const observer = new MutationObserver(() => {
        if (document.body.textContent?.includes('Factory Reset Failed')) {
          (window as any).__factoryResetErrorFlash = true;
        }
      });
      observer.observe(document.body, { subtree: true, childList: true, characterData: true });
      (window as any).__factoryResetObserver = observer;
    });

    await page.getByRole('button', { name: 'OK' }).click();
    await expect(page.getByText('Factory Reset Completed', { exact: true })).not.toBeVisible({ timeout: 5000 });

    const flashDetected = await page.evaluate(() => {
      (window as any).__factoryResetObserver?.disconnect();
      return (window as any).__factoryResetErrorFlash;
    });
    expect(flashDetected).toBe(false);
  });

  test('warning status shows the success modal', async ({ page }) => {
    await mockPortalAuth(page);
    await setupAndLogin(page, { factoryResetResultAcked: false });

    // status=4 is Warning: the reset worked, a partition just needed a second
    // format attempt.
    await publishToWebsocket('FactoryResetV1', {
      keys: ['network'],
      result: {
        status: 4,
        error: null,
        context: 'second format attempt',
        paths: ['network'],
        data_wiped: true,
      },
    });

    await expect(page.getByText('Factory Reset Completed', { exact: true })).toBeVisible({ timeout: 5000 });
  });

  test('failure after data was wiped warns the device needs another reset', async ({ page }) => {
    await mockPortalAuth(page);
    await setupAndLogin(page, { factoryResetResultAcked: false });

    await publishToWebsocket('FactoryResetV1', {
      keys: ['network'],
      result: {
        status: 2,
        error: 'format failed',
        context: 'factory partition',
        paths: [],
        data_wiped: true,
      },
    });

    await expect(page.getByText('Factory Reset Failed', { exact: true })).toBeVisible({ timeout: 5000 });
    await expect(page.getByText('format failed')).toBeVisible();
    await expect(page.getByTestId('factory-reset-data-wiped')).toBeVisible();
  });

  test('failure before any wipe does not warn about wiped data', async ({ page }) => {
    await mockPortalAuth(page);
    await setupAndLogin(page, { factoryResetResultAcked: false });

    // status=1 is Invalid — rejected before the destructive phase started.
    await publishToWebsocket('FactoryResetV1', {
      keys: ['network'],
      result: {
        status: 1,
        error: 'unknown preserve key',
        context: null,
        paths: [],
        data_wiped: false,
      },
    });

    await expect(page.getByText('Factory Reset Failed', { exact: true })).toBeVisible({ timeout: 5000 });
    await expect(page.getByTestId('factory-reset-data-wiped')).not.toBeVisible();
  });

  test('unknown status code still shows a result modal', async ({ page }) => {
    await mockPortalAuth(page);
    await setupAndLogin(page, { factoryResetResultAcked: false });

    // ODS reports a status it does not know itself as u32::MAX. The result must
    // still reach the user instead of being swallowed as "no result".
    await publishToWebsocket('FactoryResetV1', {
      keys: ['network'],
      result: {
        status: 4294967295,
        error: null,
        context: null,
        paths: [],
        data_wiped: false,
      },
    });

    await expect(page.getByText('Factory Reset Failed', { exact: true })).toBeVisible({ timeout: 5000 });
  });
});
