import { test, expect, Page } from '@playwright/test';
import { mockConfig, mockLoginSuccess, mockRequireSetPassword } from './fixtures/mock-api';
import { mockHealthcheck } from './fixtures/test-setup';
import { publishToWebsocket } from './fixtures/websocket';

// Run all tests in this file serially to avoid state interference
test.describe.configure({ mode: 'serial' });

// --- Mock helpers ---

async function mockWifiAvailable(page: Page, available = true, interfaceName = 'wlan0') {
  await page.route('**/wifi/available', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(available ? {
        state: "available",
        interface_name: interfaceName,
        version: "0.2.1"
      } : {
        state: "unavailable",
        socket_present: false,
        version_info: null
      }),
    });
  });
}

async function mockWifiStatus(page: Page, state = 'idle', ssid: string | null = null, ipAddress: string | null = null) {
  await page.route('**/wifi/status', async (route) => {
    if (route.request().method() === 'GET') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ status: 'ok', state, ssid, ip_address: ipAddress }),
      });
    } else {
      await route.continue();
    }
  });
}

async function mockWifiScanStart(page: Page) {
  await page.route('**/wifi/scan', async (route) => {
    if (route.request().method() === 'POST') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ status: 'ok', state: 'scanning' }),
      });
    } else {
      await route.continue();
    }
  });
}

async function mockWifiScanResults(page: Page, state = 'finished', networks: any[] = []) {
  await page.route('**/wifi/scan/results', async (route) => {
    if (route.request().method() === 'GET') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ status: 'ok', state, networks }),
      });
    } else {
      await route.continue();
    }
  });
}

async function mockWifiConnect(page: Page, succeed = true) {
  await page.route('**/wifi/connect', async (route) => {
    if (route.request().method() === 'POST') {
      if (succeed) {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ status: 'ok', state: 'connecting' }),
        });
      } else {
        await route.fulfill({
          status: 500,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'Connection refused' }),
        });
      }
    } else {
      await route.continue();
    }
  });
}

async function mockWifiDisconnect(page: Page) {
  await page.route('**/wifi/disconnect', async (route) => {
    if (route.request().method() === 'POST') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ status: 'ok' }),
      });
    } else {
      await route.continue();
    }
  });
}

async function mockWifiSavedNetworks(page: Page, networks: any[] = []) {
  await page.route('**/wifi/networks', async (route) => {
    if (route.request().method() === 'GET') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ status: 'ok', networks }),
      });
    } else {
      await route.continue();
    }
  });
}

async function mockWifiForget(page: Page) {
  await page.route('**/wifi/networks/forget', async (route) => {
    if (route.request().method() === 'POST') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ status: 'ok' }),
      });
    } else {
      await route.continue();
    }
  });
}


const defaultAdapters = [
  {
    name: 'eth0',
    mac: 'aa:bb:cc:dd:ee:f0',
    online: true,
    ipv4: { addrs: [{ addr: 'localhost', dhcp: true, prefix_len: 24 }], dns: ['8.8.8.8'], gateways: ['192.168.1.1'] },
  },
  {
    name: 'wlan0',
    mac: 'aa:bb:cc:dd:ee:f1',
    online: false,
    ipv4: { addrs: [{ addr: '192.168.1.50', dhcp: true, prefix_len: 24 }], dns: [], gateways: [] },
  },
];

const scanNetworks = [
  { ssid: 'HomeNetwork', mac: 'aa:bb:cc:dd:ee:01', ch: 6, rssi: -45 },
  { ssid: 'OfficeWiFi', mac: 'aa:bb:cc:dd:ee:02', ch: 11, rssi: -65 },
  { ssid: 'GuestNet', mac: 'aa:bb:cc:dd:ee:03', ch: 1, rssi: -80 },
];

const savedNetworks = [
  { ssid: 'HomeNetwork', flags: '[CURRENT]' },
  { ssid: 'OldNetwork', flags: '' },
];

/** Set up all route mocks before navigation */
async function setupMocks(page: Page, wifiAvailable = true) {
  await page.addInitScript(() => {
    sessionStorage.setItem('factoryResetResultAcked', 'true')
    sessionStorage.setItem('updateValidationAcked', 'true')
  });
  await mockConfig(page);
  await mockLoginSuccess(page);
  await mockRequireSetPassword(page);
  await mockHealthcheck(page);
  await mockWifiAvailable(page, wifiAvailable);
  await mockWifiStatus(page, 'idle');
  await mockWifiSavedNetworks(page, []);
}

/** Login, publish network adapters via WebSocket, navigate to Network page */
async function loginAndNavigateWith(page: Page, adapters = defaultAdapters) {
  await page.goto('/');
  await page.getByPlaceholder(/enter your password/i).fill('password');
  await page.getByRole('button', { name: /log in/i }).click();
  await expect(page.getByText('Common Info')).toBeVisible({ timeout: 10000 });

  // Publish network adapter data via WebSocket (after login so WebSocket is subscribed)
  await publishToWebsocket('NetworkStatusV1', { network_status: adapters });

  // Navigate to network page
  await page.getByRole('link', { name: /network/i }).click();
  await expect(page.locator('.text-h4', { hasText: 'Network' })).toBeVisible({ timeout: 5000 });

  // Wait for adapter tabs to appear from WebSocket data
  await expect(page.getByRole('tab', { name: /eth0/i })).toBeVisible({ timeout: 10000 });
}

// Static-IP adapters for tests that interact with the network config form
const staticAdapters = [
  {
    name: 'eth0',
    mac: 'aa:bb:cc:dd:ee:f0',
    online: true,
    ipv4: { addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }], dns: ['8.8.8.8'], gateways: ['192.168.1.1'] },
  },
  {
    name: 'wlan0',
    mac: 'aa:bb:cc:dd:ee:f1',
    online: true,
    ipv4: { addrs: [{ addr: '192.168.1.50', dhcp: false, prefix_len: 24 }], dns: ['8.8.8.8'], gateways: ['192.168.1.1'] },
  },
];

// Adapter state after WiFi connects and the OS assigns a new IP to wlan0
const wlan0AfterWifiAdapters = [
  {
    name: 'eth0',
    mac: 'aa:bb:cc:dd:ee:f0',
    online: true,
    ipv4: { addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }], dns: ['8.8.8.8'], gateways: ['192.168.1.1'] },
  },
  {
    name: 'wlan0',
    mac: 'aa:bb:cc:dd:ee:f1',
    online: true,
    ipv4: { addrs: [{ addr: '192.168.100.50', dhcp: false, prefix_len: 24 }], dns: ['10.0.0.1'], gateways: ['192.168.100.1'] },
  },
];

async function mockNetworkConfigSuccess(page: Page) {
  await page.route('**/network', async (route) => {
    if (route.request().method() === 'POST') {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ rollbackTimeoutSeconds: 90, uiPort: 5173, rollbackEnabled: false }),
      });
    } else {
      await route.continue();
    }
  });
}


test.describe('WiFi Management', () => {
  test('WiFi unavailable - no WiFi panel shown', async ({ page }) => {
    await setupMocks(page, false);
    await loginAndNavigateWith(page);

    await page.getByRole('tab', { name: /wlan0/i }).click();

    // WiFi panel should NOT be visible
    await expect(page.getByText('WiFi Connection')).not.toBeVisible();
  });

  test('WiFi panel visible on WiFi adapter tab only', async ({ page }) => {
    await setupMocks(page);
    await loginAndNavigateWith(page);

    // eth0 tab should not show WiFi panel
    await page.getByRole('tab', { name: /eth0/i }).click();
    await expect(page.getByText('WiFi Connection')).not.toBeVisible();

    // wlan0 tab should show WiFi panel
    await page.getByRole('tab', { name: /wlan0/i }).click();
    await expect(page.getByText('WiFi Connection')).toBeVisible({ timeout: 10000 });
    await expect(page.getByText('Available Networks')).toBeVisible();
    await expect(page.getByText('Saved Networks', { exact: true })).toBeVisible();
  });

  test('WiFi icon shown on WiFi adapter tab', async ({ page }) => {
    await setupMocks(page);
    await loginAndNavigateWith(page);

    // wlan0 tab should have WiFi icon
    const wlan0Tab = page.getByRole('tab', { name: /wlan0/i });
    await expect(wlan0Tab.locator('.mdi-wifi')).toBeVisible();

    // eth0 tab should NOT have WiFi icon
    const eth0Tab = page.getByRole('tab', { name: /eth0/i });
    await expect(eth0Tab.locator('.mdi-wifi')).not.toBeVisible();
  });

  test('WiFi panel not shown on wlan0 when service reports different interface', async ({ page }) => {
    await setupMocks(page);
    // Override: service reports wlan1 as the WiFi interface
    await page.unroute('**/wifi/available');
    await mockWifiAvailable(page, true, 'wlan1');

    const adaptersWithWlan1 = [
      ...defaultAdapters,
      {
        name: 'wlan1',
        mac: 'aa:bb:cc:dd:ee:f2',
        online: false,
        ipv4: { addrs: [], dns: [], gateways: [] },
      },
    ];
    await loginAndNavigateWith(page, adaptersWithWlan1);

    // wlan0 tab must NOT show WiFi panel (service reports wlan1)
    await page.getByRole('tab', { name: /wlan0/i }).click();
    await expect(page.getByText('WiFi Connection')).not.toBeVisible();

    // wlan1 tab must show WiFi panel
    await page.getByRole('tab', { name: /wlan1/i }).click();
    await expect(page.getByText('WiFi Connection')).toBeVisible({ timeout: 10000 });
  });

  test('scan flow - discovers networks', async ({ page }) => {
    await setupMocks(page);
    await mockWifiScanStart(page);
    await mockWifiScanResults(page, 'finished', scanNetworks);
    await loginAndNavigateWith(page);

    await page.getByRole('tab', { name: /wlan0/i }).click();
    await expect(page.getByText('WiFi Connection')).toBeVisible();

    // Click scan button
    await page.locator('[data-cy=wifi-scan-button]').click();

    // Networks should appear (after polling)
    await expect(page.locator('[data-cy="wifi-network-HomeNetwork"]')).toBeVisible({ timeout: 10000 });
    await expect(page.locator('[data-cy="wifi-network-OfficeWiFi"]')).toBeVisible();
    await expect(page.locator('[data-cy="wifi-network-GuestNet"]')).toBeVisible();
  });

  test('connect flow - password dialog and connection', async ({ page }) => {
    await setupMocks(page);
    await mockWifiScanStart(page);
    await mockWifiScanResults(page, 'finished', scanNetworks);
    await mockWifiConnect(page);
    await loginAndNavigateWith(page);

    await page.getByRole('tab', { name: /wlan0/i }).click();

    // Scan first
    await page.locator('[data-cy=wifi-scan-button]').click();
    await expect(page.locator('[data-cy="wifi-network-HomeNetwork"]')).toBeVisible({ timeout: 10000 });

    // Click on a network to connect
    await page.locator('[data-cy="wifi-network-HomeNetwork"]').click();

    // Password dialog should open
    await expect(page.getByText('Connect to HomeNetwork')).toBeVisible();
    await page.locator('[data-cy=wifi-password-input] input').fill('mypassword');

    // Mock status to return connected after connect
    await page.unroute('**/wifi/status');
    await mockWifiStatus(page, 'connected', 'HomeNetwork', '192.168.1.100');

    await page.locator('[data-cy=wifi-connect-button]').click();

    // Should show SSID and disconnect button
    await expect(page.locator('[data-cy=wifi-disconnect-button]')).toBeVisible({ timeout: 15000 });
  });

  test('disconnect flow', async ({ page }) => {
    // Start with connected status
    await setupMocks(page);
    await page.unroute('**/wifi/status');
    await mockWifiStatus(page, 'connected', 'HomeNetwork', '192.168.1.100');
    await mockWifiDisconnect(page);
    await loginAndNavigateWith(page);

    await page.getByRole('tab', { name: /wlan0/i }).click();

    // Should show SSID and disconnect button
    await expect(page.getByText('HomeNetwork')).toBeVisible({ timeout: 10000 });
    await expect(page.locator('[data-cy=wifi-disconnect-button]')).toBeVisible();

    // Mock status to return idle after disconnect
    await page.unroute('**/wifi/status');
    await mockWifiStatus(page, 'idle');

    await page.locator('[data-cy=wifi-disconnect-button]').click();

    // Should show not connected
    await expect(page.getByText('Not connected')).toBeVisible({ timeout: 10000 });
  });

  test('forget dialog shows warning when forgetting the current network', async ({ page }) => {
    await setupMocks(page);
    await page.unroute('**/wifi/networks');
    await mockWifiSavedNetworks(page, savedNetworks); // HomeNetwork is [CURRENT]
    await loginAndNavigateWith(page);

    await page.getByRole('tab', { name: /wlan0/i }).click();
    await expect(page.locator('[data-cy="wifi-saved-HomeNetwork"]')).toBeVisible({ timeout: 10000 });

    await page.locator('[data-cy="wifi-forget-HomeNetwork"]').click();

    await expect(page.locator('[data-cy="wifi-forget-current-warning"]')).toBeVisible();
  });

  test('forget dialog shows no warning for a non-current saved network', async ({ page }) => {
    await setupMocks(page);
    await page.unroute('**/wifi/networks');
    await mockWifiSavedNetworks(page, savedNetworks); // OldNetwork has no flags
    await loginAndNavigateWith(page);

    await page.getByRole('tab', { name: /wlan0/i }).click();
    await expect(page.locator('[data-cy="wifi-saved-OldNetwork"]')).toBeVisible({ timeout: 10000 });

    await page.locator('[data-cy="wifi-forget-OldNetwork"]').click();

    await expect(page.getByText('Forget Network')).toBeVisible();
    await expect(page.locator('[data-cy="wifi-forget-current-warning"]')).not.toBeVisible();
  });

  test('saved networks and forget', async ({ page }) => {
    await setupMocks(page);
    await page.unroute('**/wifi/networks');
    await mockWifiSavedNetworks(page, savedNetworks);
    await mockWifiForget(page);
    await loginAndNavigateWith(page);

    await page.getByRole('tab', { name: /wlan0/i }).click();

    // Saved networks should be visible
    await expect(page.locator('[data-cy="wifi-saved-HomeNetwork"]')).toBeVisible({ timeout: 10000 });
    await expect(page.locator('[data-cy="wifi-saved-OldNetwork"]')).toBeVisible();

    // Click forget on OldNetwork
    await page.locator('[data-cy="wifi-forget-OldNetwork"]').click();

    // Confirmation dialog
    await expect(page.getByText('Forget Network')).toBeVisible();
    await expect(page.getByRole('strong')).toHaveText('OldNetwork');

    // After confirming, mock returns only HomeNetwork
    await page.unroute('**/wifi/networks');
    await mockWifiSavedNetworks(page, [{ ssid: 'HomeNetwork', flags: '[CURRENT]' }]);

    await page.locator('[data-cy=wifi-forget-confirm-button]').click();

    // OldNetwork should disappear
    await expect(page.locator('[data-cy="wifi-saved-OldNetwork"]')).not.toBeVisible({ timeout: 10000 });
    await expect(page.locator('[data-cy="wifi-saved-HomeNetwork"]')).toBeVisible();
  });
});

test.describe('WiFi and Network Config Interaction', () => {
  // Both v-window-items are rendered in the DOM at all times. All helpers below scope
  // their selectors to the currently active tab panel to avoid strict-mode violations.
  const activePanel = (page: Page) => page.locator('.v-window-item--active');
  const activeIpField = (page: Page) =>
    activePanel(page).getByRole('textbox', { name: /IP Address/i });
  const activeApplyButton = (page: Page) =>
    activePanel(page).locator('[data-cy=network-apply-button]');
  const activeDiscardButton = (page: Page) =>
    activePanel(page).locator('[data-cy=network-discard-button]');

  test('network config form syncs with new IP after WiFi assigns one (clean form)', async ({ page }) => {
    // This test exposes a bug: after a NetworkStatusV1 update (simulating WiFi assigning a new
    // IP to wlan0), the network config form should show the new IP, not the stale one.
    await setupMocks(page);
    await loginAndNavigateWith(page, staticAdapters);

    await page.getByRole('tab', { name: /wlan0/i }).click();
    await expect(page.getByText('WiFi Connection')).toBeVisible({ timeout: 10000 });

    // Verify initial IP in form
    await expect(activeIpField(page)).toHaveValue('192.168.1.50', { timeout: 5000 });
    await expect(activeApplyButton(page)).toBeDisabled();

    // Simulate WiFi connecting and the OS assigning a new IP to wlan0
    await publishToWebsocket('NetworkStatusV1', { network_status: wlan0AfterWifiAdapters });

    // Form must sync to the WiFi-assigned IP and remain clean
    await expect(activeIpField(page)).toHaveValue('192.168.100.50', { timeout: 5000 });
    await expect(activeApplyButton(page)).toBeDisabled();
  });

  test('network config form preserves user edits when WiFi assigns a new IP (dirty form)', async ({ page }) => {
    await setupMocks(page);
    await loginAndNavigateWith(page, staticAdapters);

    await page.getByRole('tab', { name: /wlan0/i }).click();
    await expect(page.getByText('WiFi Connection')).toBeVisible({ timeout: 10000 });

    // User edits the IP → form becomes dirty
    await activeIpField(page).fill('192.168.1.99');
    await expect(activeApplyButton(page)).toBeEnabled({ timeout: 5000 });

    // Simulate WiFi assigning a different IP to wlan0
    await publishToWebsocket('NetworkStatusV1', { network_status: wlan0AfterWifiAdapters });

    // Dirty flag must protect the user's edit — form must NOT be overwritten
    await expect(activeIpField(page)).toHaveValue('192.168.1.99');
    await expect(activeApplyButton(page)).toBeEnabled();
  });

  test('WiFi scan does not reset or modify network config form state', async ({ page }) => {
    await setupMocks(page);
    await mockWifiScanStart(page);
    await mockWifiScanResults(page, 'finished', scanNetworks);
    await loginAndNavigateWith(page, staticAdapters);

    await page.getByRole('tab', { name: /wlan0/i }).click();
    await expect(page.getByText('WiFi Connection')).toBeVisible({ timeout: 10000 });

    // User edits the IP → form becomes dirty
    await activeIpField(page).fill('192.168.1.99');
    await expect(activeApplyButton(page)).toBeEnabled({ timeout: 5000 });

    // Trigger WiFi scan
    await page.locator('[data-cy=wifi-scan-button]').click();
    await expect(page.locator('[data-cy="wifi-network-HomeNetwork"]')).toBeVisible({ timeout: 10000 });

    // Network config form must be unchanged
    await expect(activeIpField(page)).toHaveValue('192.168.1.99');
    await expect(activeApplyButton(page)).toBeEnabled();
  });

  test('discard after WiFi assigns new IP resets form to WiFi-assigned IP', async ({ page }) => {
    await setupMocks(page);
    await loginAndNavigateWith(page, staticAdapters);

    await page.getByRole('tab', { name: /wlan0/i }).click();
    await expect(page.getByText('WiFi Connection')).toBeVisible({ timeout: 10000 });

    // User edits the IP → form becomes dirty
    await activeIpField(page).fill('192.168.1.99');
    await expect(activeApplyButton(page)).toBeEnabled({ timeout: 5000 });

    // Simulate WiFi assigning a new IP to wlan0 while user has unsaved edits
    await publishToWebsocket('NetworkStatusV1', { network_status: wlan0AfterWifiAdapters });

    // Discard changes — must reload from the current network_status, not the original
    await activeDiscardButton(page).click();

    // Form must show the WiFi-assigned IP (192.168.100.50), not the user edit or the original
    await expect(activeIpField(page)).toHaveValue('192.168.100.50', { timeout: 5000 });
    await expect(activeApplyButton(page)).toBeDisabled();
  });

  test('network config apply succeeds while WiFi scan is in progress', async ({ page }) => {
    await setupMocks(page);
    await mockWifiScanStart(page);
    // Scan stays in scanning state indefinitely
    await page.route('**/wifi/scan/results', async (route) => {
      if (route.request().method() === 'GET') {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({ status: 'ok', state: 'scanning', networks: [] }),
        });
      } else {
        await route.continue();
      }
    });
    await mockNetworkConfigSuccess(page);
    await loginAndNavigateWith(page, staticAdapters);

    // Start WiFi scan on wlan0 tab
    await page.getByRole('tab', { name: /wlan0/i }).click();
    await expect(page.getByText('WiFi Connection')).toBeVisible({ timeout: 10000 });
    await page.locator('[data-cy=wifi-scan-button]').click();
    // Scan spinner should be visible (scan in progress)
    await expect(page.locator('[data-cy=wifi-scan-button]')).toBeVisible();

    // Switch to eth0 tab (no network form unsaved changes) and edit config
    await page.getByRole('tab', { name: /eth0/i }).click();
    // Wait for networkFormStartEdit('eth0') to be processed before editing
    await expect(activeApplyButton(page)).toBeDisabled({ timeout: 5000 });
    await activeIpField(page).fill('192.168.1.200');
    await expect(activeApplyButton(page)).toBeEnabled({ timeout: 5000 });

    // Apply eth0 config while WiFi scan is still in progress
    await activeApplyButton(page).click();

    // Config should apply independently of WiFi scan state
    await expect(activeApplyButton(page)).toBeDisabled({ timeout: 10000 });
  });

  test('network config apply succeeds while WiFi is connecting', async ({ page }) => {
    await setupMocks(page);
    await mockWifiScanStart(page);
    await mockWifiScanResults(page, 'finished', scanNetworks);
    await mockWifiConnect(page);
    // Status stays at 'connecting' indefinitely so WiFi never completes
    await mockWifiStatus(page, 'connecting', 'HomeNetwork', null);
    await mockNetworkConfigSuccess(page);
    await loginAndNavigateWith(page, staticAdapters);

    // Start connecting to a WiFi AP on wlan0 tab
    await page.getByRole('tab', { name: /wlan0/i }).click();
    await expect(page.getByText('WiFi Connection')).toBeVisible({ timeout: 10000 });
    await page.locator('[data-cy=wifi-scan-button]').click();
    await expect(page.locator('[data-cy="wifi-network-HomeNetwork"]')).toBeVisible({ timeout: 10000 });
    await page.locator('[data-cy="wifi-network-HomeNetwork"]').click();
    await page.locator('[data-cy=wifi-password-input] input').fill('mypassword');
    await page.locator('[data-cy=wifi-connect-button]').click();
    // WiFi is now in Connecting state
    await expect(page.getByText('Connecting to HomeNetwork...')).toBeVisible({ timeout: 5000 });

    // Switch to eth0 tab and edit config
    await page.getByRole('tab', { name: /eth0/i }).click();
    // Wait for networkFormStartEdit('eth0') to be processed before editing
    await expect(activeApplyButton(page)).toBeDisabled({ timeout: 5000 });
    await activeIpField(page).fill('192.168.1.200');
    await expect(activeApplyButton(page)).toBeEnabled({ timeout: 5000 });

    // Apply eth0 config while WiFi connection is still in progress
    await activeApplyButton(page).click();

    // Config should apply independently of WiFi connecting state
    await expect(activeApplyButton(page)).toBeDisabled({ timeout: 10000 });
  });
});
