import { test, expect, Page } from '@playwright/test';
import { publishToWebsocket } from './fixtures/websocket';
import { setupAndLogin } from './fixtures/test-setup';

test.describe('Device Info', () => {
  async function mockWifiAvailable(page: Page, response: any) {
    await page.route('**/wifi/available', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify(response),
      });
    });
  }

  test('displays system info via WebSocket', async ({ page }) => {
    await mockWifiAvailable(page, { state: 'unavailable', socket_present: false, version_info: null });
    await setupAndLogin(page);

    const systemInfo = {
      os: {
        name: 'Omnect OS',
        version: '1.2.3',
      },
      azure_sdk_version: '0.1.0',
      omnect_device_service_version: '4.5.6',
      boot_time: new Date().toISOString(),
      hostname: 'omnect-device',
    };

    // Publish to WebSocket
    await publishToWebsocket('SystemInfoV1', systemInfo);

    // Assert values appear on dashboard
    // Adjust selectors based on actual UI
    await expect(page.getByText('Omnect OS')).toBeVisible();
    await expect(page.getByText('1.2.3')).toBeVisible();
    await expect(page.getByText('4.5.6')).toBeVisible();
    await expect(page.getByText('omnect-device')).toBeVisible();
  });

  test('hides WiFi commissioning service version when socket is missing', async ({ page }) => {
    await mockWifiAvailable(page, { state: 'unavailable', socket_present: false, version_info: null });
    await setupAndLogin(page);
    await expect(page.getByText('WiFi commissioning service version')).not.toBeVisible();
  });

  test('displays WiFi commissioning service version with hint when incompatible', async ({ page }) => {
    await mockWifiAvailable(page, {
      state: 'unavailable',
      socket_present: true,
      version_info: { required: '>=0.2.1', current: '0.2.0', mismatch: true },
    });
    await setupAndLogin(page);
    await expect(page.getByText('WiFi commissioning service version')).toBeVisible();
    await expect(page.getByText('0.2.0 (required: >=0.2.1)')).toBeVisible();
  });

  test('shows unknown version when the service does not answer the probe', async ({ page }) => {
    await mockWifiAvailable(page, { state: 'unavailable', socket_present: true, version_info: null });
    await setupAndLogin(page);
    await expect(page.getByText('WiFi commissioning service version')).toBeVisible();
    await expect(page.getByText('unknown', { exact: true })).toBeVisible();
  });

  test('displays WiFi commissioning service version without hint when compatible', async ({ page }) => {
    await mockWifiAvailable(page, { state: 'available', version: "0.2.1", interface_name: "wlan0" });
    await setupAndLogin(page);
    await expect(page.getByText('WiFi commissioning service version')).toBeVisible();
    await expect(page.getByText('0.2.1', { exact: true })).toBeVisible();
    await expect(page.getByText('required:')).not.toBeVisible();
  });
});
