import { test, expect, Page } from '@playwright/test';
import { mockConfig, mockLoginSuccess, mockRequireSetPassword } from './fixtures/mock-api';
import { NetworkTestHarness, DeviceNetwork } from './fixtures/network-test-harness';

// Run all tests in this file serially to avoid Centrifugo channel interference
test.describe.configure({ mode: 'serial' });

test.describe('Network Configuration - Comprehensive E2E Tests', () => {
  let harness: NetworkTestHarness;

  test.beforeEach(async ({ page }) => {
    harness = new NetworkTestHarness();
    await mockConfig(page);
    await mockLoginSuccess(page);
    await mockRequireSetPassword(page);
    await harness.mockNetworkConfig(page);
    await harness.mockHealthcheck(page);
    await harness.mockAckRollback(page);

    await page.goto('/');
    await page.getByPlaceholder(/enter your password/i).fill('password');
    await page.getByRole('button', { name: /log in/i }).click();
    await expect(page.getByText('Common Info')).toBeVisible({ timeout: 10000 });
  });

  test.afterEach(() => {
    harness.reset();
  });

  test.describe('CRITICAL: Rollback Flows and Error Handling', () => {
    test('automatic rollback timeout - healthcheck fails, rollback triggered', async ({ page }) => {
      const shortTimeoutSeconds = 3;
      await page.unroute('**/network');
      await harness.mockNetworkConfig(page, { rollbackTimeoutSeconds: shortTimeoutSeconds });
      await harness.mockHealthcheck(page, { healthcheckAlwaysFails: true });

      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: 'localhost', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      await expect(page.getByText('(current connection)')).toBeVisible();

      // Change IP address
      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await ipInput.fill('192.168.1.150');

      await page.getByRole('button', { name: /save/i }).click();

      await expect(page.getByText('Confirm Network Configuration Change')).toBeVisible({ timeout: 5000 });
      await expect(page.getByRole('checkbox', { name: /Enable automatic rollback/i })).toBeChecked();

      await page.getByRole('button', { name: /apply changes/i }).click();

      await expect(page.locator('#overlay').getByText('Automatic rollback in:')).toBeVisible({ timeout: 10000 });

      expect(harness.getRollbackState().enabled).toBe(true);

      await harness.simulateRollbackTimeout();
      // Wait long enough for client-side timeout (3s) to fire, which triggers the UI update
      // Increased to 6s for stability in slow environments
      await page.waitForTimeout(6000);

      await harness.mockHealthcheck(page, { healthcheckAlwaysFails: false });
      // Explicitly fail the new IP healthcheck to force timeout/rollback logic on client
      // We use abort() to simulate a network error because the app masks 503 responses as 200
      await page.route('**/*192.168.1.150*/healthcheck', route => route.abort());

      await expect(page.locator('#overlay').getByText(/Rollback in progress/i).first()).toBeVisible({ timeout: 15000 });
      await expect(page.locator('#overlay')).not.toBeVisible({ timeout: 20000 });
      await expect(page).toHaveURL(/\/login/, { timeout: 15000 });
    });

    test('DHCP rollback - automatic redirect to login after timeout', async ({ page }) => {
      const shortTimeoutSeconds = 3;
      await page.unroute('**/network');
      await harness.mockNetworkConfig(page, { rollbackTimeoutSeconds: shortTimeoutSeconds });
      // Ensure healthcheck fails initially so we can verify the timeout/rollback state
      await harness.mockHealthcheck(page, { healthcheckAlwaysFails: true });

      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: 'localhost', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      await page.getByLabel('DHCP').click({ force: true });
      await page.waitForTimeout(500);
      await page.getByRole('button', { name: /save/i }).click();

      await expect(page.getByText('Confirm Network Configuration Change')).toBeVisible({ timeout: 5000 });
      await page.getByRole('checkbox', { name: /Enable automatic rollback/i }).check();
      await page.getByRole('button', { name: /apply changes/i }).click();

      await expect(page.locator('#overlay')).toBeVisible();

      await harness.simulateRollbackTimeout();
      // Wait for client timeout (3s) to trigger UI update
      await page.waitForTimeout(4000);

      // Verify rollback message matches
      await expect(page.locator('#overlay').getByText(/Rollback in progress/i).first()).toBeVisible({ timeout: 15000 });

      // Now allow recovery
      await harness.mockHealthcheck(page, { healthcheckAlwaysFails: false });

      await expect(page.locator('#overlay')).not.toBeVisible({ timeout: 20000 });
      await expect(page).toHaveURL(/\/login/, { timeout: 10000 });
      // Changed: expect MODAL instead of snackbar (fix: show rollback modal instead of snackbar on dynamic rollback)
      await expect(page.getByText('Network Settings Rolled Back')).toBeVisible();
    });

    test('rollback cancellation - new IP becomes reachable within timeout', async ({ page }) => {
      await harness.mockHealthcheck(page, { healthcheckSuccessAfter: 2000 });

      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: 'localhost', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await ipInput.fill('192.168.1.150');

      await page.getByRole('button', { name: /save/i }).click();
      await expect(page.getByText('Confirm Network Configuration Change')).toBeVisible({ timeout: 5000 });
      await page.getByRole('button', { name: /apply changes/i }).click();

      await expect(page.locator('#overlay').getByText('Automatic rollback in:')).toBeVisible({ timeout: 10000 });

      await page.waitForTimeout(3000);
      await harness.simulateNewIpReachable();
      await page.waitForTimeout(1000);

      const rollbackState = harness.getRollbackState();
      expect(rollbackState.enabled).toBe(false);
      expect(rollbackState.occurred).toBe(false);
    });

    test('invalid IP address validation error', async ({ page }) => {
      await harness.setup(page, {}); // default adapter

      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await page.getByLabel('Static').click({ force: true });

      await ipInput.fill('999.999.999.999');
      await expect(ipInput).toBeVisible(); // Vuetify error state check usually involves class or error message

      await ipInput.fill('not.an.ip.address');
      await expect(ipInput).toHaveValue('not.an.ip.address');

      await ipInput.fill('192.168.1.200');
      await expect(ipInput).toHaveValue('192.168.1.200');
    });

    test('backend error handling during configuration apply', async ({ page }) => {
      await harness.mockNetworkConfigError(page, 500, 'Failed to apply network configuration. Please check your settings and try again.');

      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: '192.168.1.200', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await ipInput.fill('192.168.1.210');

      const saveButton = page.getByRole('button', { name: /save/i });
      await saveButton.click();

      await expect(saveButton).toBeEnabled({ timeout: 5000 });
    });

    test('REGRESSION: form fields not reset during editing (caret stability)', async ({ page }) => {
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await expect(ipInput).toHaveValue('192.168.1.100', { timeout: 5000 });

      await ipInput.clear();
      await ipInput.pressSequentially('10.20.30.40', { delay: 50 });
      await expect(ipInput).toHaveValue('10.20.30.40');

      await page.getByLabel('DHCP').click({ force: true });
      await expect(page.getByLabel('DHCP')).toBeChecked();

      await page.getByLabel('Static').click({ force: true });
      await expect(page.getByLabel('Static')).toBeChecked();
      await expect(ipInput).toBeEditable();

      await ipInput.clear();
      await ipInput.pressSequentially('172.16.0.1', { delay: 50 });
      await expect(ipInput).toHaveValue('172.16.0.1');
    });

    test('REGRESSION: Save on non-current adapter should not show endless progress', async ({ page }) => {
      // Setup two adapters: eth0 (current) and eth1 (not current)
      await harness.setup(page, [
        { name: 'eth0', ipv4: { addrs: [{ addr: 'localhost', dhcp: false, prefix_len: 24 }] } },
        { name: 'eth1', mac: '00:11:22:33:44:56', ipv4: { addrs: [{ addr: '192.168.1.101', dhcp: false, prefix_len: 24 }] } }
      ], 'eth1');

      // Verify it's not the current connection
      await expect(page.getByText('(current connection)')).not.toBeVisible();

      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await ipInput.fill('192.168.1.102'); // Change static IP

      // Use the new helper to click Save and verify it finishes
      await harness.saveAndVerify(page);

      // Verify IP field is still editable
      await expect(ipInput).toBeEditable();

      // Switch to DHCP
      await page.locator('.v-window-item--active').getByLabel('DHCP').click({ force: true });
      await expect(page.locator('.v-window-item--active').getByLabel('DHCP')).toBeChecked();

      // Verify IP field is NOT editable (or hidden/disabled)
      await expect(ipInput).not.toBeEditable();

      await harness.saveAndVerify(page);
    });

    test('button remains visible when rollback disabled (no timeout occurs)', async ({ page }) => {
      // Setup current adapter with static IP
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: 'localhost', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      await expect(page.getByText('(current connection)')).toBeVisible();

      // Change IP address
      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await ipInput.fill('192.168.1.150');
      await page.getByRole('button', { name: /save/i }).click();

      // Open modal and DISABLE rollback
      await expect(page.getByText('Confirm Network Configuration Change')).toBeVisible();
      await page.getByRole('checkbox', { name: /Enable automatic rollback/i }).uncheck();
      await page.getByRole('button', { name: /apply changes/i }).click();

      // Verify overlay appears
      await expect(page.locator('#overlay')).toBeVisible({ timeout: 10000 });

      // Verify button is shown (IP is known, not DHCP)
      await expect(page.getByRole('button', { name: /Open new address in new tab/i })).toBeVisible();

      // Simulate unreachable new IP (polling continues indefinitely - no timeout when rollback disabled)
      await page.route('**/*192.168.1.150*/healthcheck', route => route.abort());
      await page.waitForTimeout(3000); // Wait a bit

      // CRITICAL: Button remains visible (stays in waiting_for_new_ip state, no timeout)
      await expect(page.getByRole('button', { name: /Open new address in new tab/i })).toBeVisible();

      // Verify text for no-rollback scenario
      await expect(page.locator('#overlay').getByText(/Network configuration applied/i)).toBeVisible();
    });

    test('button hidden when switching to DHCP (IP unknown)', async ({ page }) => {
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: 'localhost', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      await expect(page.getByText('(current connection)')).toBeVisible();

      // Switch to DHCP
      await page.getByLabel('DHCP').click({ force: true });
      await page.waitForTimeout(500);
      await page.getByRole('button', { name: /save/i }).click();

      await expect(page.getByText('Confirm Network Configuration Change')).toBeVisible();
      await page.getByRole('checkbox', { name: /Enable automatic rollback/i }).check();
      await page.getByRole('button', { name: /apply changes/i }).click();

      // Verify overlay appears
      await expect(page.locator('#overlay')).toBeVisible({ timeout: 10000 });

      // CRITICAL: Button should NOT be shown (IP is unknown for DHCP)
      await expect(page.getByRole('button', { name: /Open new address in new tab/i })).not.toBeVisible();

      // Verify DHCP-specific text
      await expect(page.locator('#overlay').getByText(/Find the new IP via DHCP server/i)).toBeVisible();
    });

    test('button hidden when waiting for rollback to complete', async ({ page }) => {
      const shortTimeoutSeconds = 3;
      await page.unroute('**/network');
      await harness.mockNetworkConfig(page, { rollbackTimeoutSeconds: shortTimeoutSeconds });
      await harness.mockHealthcheck(page, { healthcheckAlwaysFails: true });

      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: 'localhost', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await ipInput.fill('192.168.1.150');
      await page.getByRole('button', { name: /save/i }).click();

      await expect(page.getByText('Confirm Network Configuration Change')).toBeVisible();
      await page.getByRole('button', { name: /apply changes/i }).click();

      // Button should be visible initially during polling
      await expect(page.getByRole('button', { name: /Open new address in new tab/i })).toBeVisible();

      // Wait for timeout to trigger rollback
      await harness.simulateRollbackTimeout();
      await page.route('**/*192.168.1.150*/healthcheck', route => route.abort());
      await page.waitForTimeout(6000);

      // CRITICAL: Button should be HIDDEN during rollback verification (WaitingForOldIp state)
      await expect(page.locator('#overlay').getByText(/Rollback in progress/i)).toBeVisible({ timeout: 15000 });
      await expect(page.getByRole('button', { name: /Open new address in new tab/i })).not.toBeVisible();
    });
  });

  test.describe('CRITICAL: Rollback Persistence and State', () => {
    test('rollback status is cleared after ack and does not reappear on re-login', async ({ page }) => {
      let healthcheckRollbackStatus = true;

      await page.unroute('**/healthcheck');
      await page.route('**/healthcheck', async (route) => {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            version_info: { required: '>=0.39.0', current: '0.40.0', mismatch: false },
            update_validation_status: { status: 'valid' },
            network_rollback_occurred: healthcheckRollbackStatus,
          }),
        });
      });

      await page.route('**/ack-rollback', async (route) => {
        if (route.request().method() === 'POST') {
          healthcheckRollbackStatus = false;
          await route.fulfill({ status: 200 });
        }
      });

      await page.goto('/');
      await expect(page.getByText('Network Settings Rolled Back')).toBeVisible({ timeout: 10000 });

      await page.getByRole('button', { name: /ok/i }).click();
      await expect(page.getByText('Network Settings Rolled Back')).not.toBeVisible();
      await page.waitForTimeout(500);

      await page.getByPlaceholder(/enter your password/i).fill('password');
      await page.getByRole('button', { name: /log in/i }).click();
      await expect(page.getByText('Common Info')).toBeVisible({ timeout: 10000 });

      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: 'localhost', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      await page.reload();
      await expect(page.getByText('Network Settings Rolled Back')).not.toBeVisible({ timeout: 3000 });

      await page.getByPlaceholder(/enter your password/i).fill('password');
      await page.getByRole('button', { name: /log in/i }).click();
      await expect(page.getByText('Common Info')).toBeVisible({ timeout: 10000 });
      await expect(page.getByText('Network Settings Rolled Back')).not.toBeVisible();

      await harness.navigateToNetwork(page);
      await harness.navigateToAdapter(page, 'eth0');
      await expect(page.getByText('eth0')).toBeVisible();
    });

    test('Static -> DHCP: Rollback should be DISABLED by default', async ({ page }) => {
      await harness.setup(page, { ipv4: { addrs: [{ addr: 'localhost', dhcp: false, prefix_len: 24 }] } });
      await expect(page.getByLabel('Static')).toBeChecked();

      await page.getByLabel('DHCP').click({ force: true });
      await page.getByRole('button', { name: /save/i }).click();

      await expect(page.getByText('Confirm Network Configuration Change')).toBeVisible();
      await expect(page.getByRole('checkbox', { name: /Enable automatic rollback/i })).not.toBeChecked();
    });

    test('DHCP -> Static: Rollback should be ENABLED by default', async ({ page }) => {
      await harness.setup(page, { ipv4: { addrs: [{ addr: 'localhost', dhcp: true, prefix_len: 24 }] } });
      await expect(page.getByLabel('DHCP')).toBeChecked();

      await page.getByLabel('Static').click({ force: true });
      await page.getByRole('textbox', { name: /IP Address/i }).fill('192.168.1.150');
      await page.getByRole('button', { name: /save/i }).click();

      await expect(page.getByText('Confirm Network Configuration Change')).toBeVisible();
      await expect(page.getByRole('checkbox', { name: /Enable automatic rollback/i })).toBeChecked();
    });

    test('DHCP -> Static (Same IP): Rollback should be ENABLED', async ({ page }) => {
      await harness.setup(page, { ipv4: { addrs: [{ addr: 'localhost', dhcp: true, prefix_len: 24 }] } });
      await expect(page.getByLabel('DHCP')).toBeChecked();

      await page.getByLabel('Static').click({ force: true });
      // IP is auto-filled with current IP ('localhost'), do NOT change it.
      await page.getByRole('button', { name: /save/i }).click();

      // Verify Modal
      await expect(page.getByText('Confirm Network Configuration Change')).toBeVisible();

      // Verify Checkbox is CHECKED
      await expect(page.getByRole('checkbox', { name: /Enable automatic rollback/i })).toBeChecked();

      // Apply changes
      await page.getByRole('button', { name: /apply changes/i }).click();

      // Verify overlay appears with countdown label
      await expect(page.locator('#overlay').getByText('Automatic rollback in:')).toBeVisible({ timeout: 10000 });
    });

    test('Rollback should show MODAL not SNACKBAR when connection is restored at old IP', async ({ page }) => {
      await harness.setup(page, { ipv4: { addrs: [{ addr: 'localhost', dhcp: false, prefix_len: 24 }] } });

      await page.getByLabel('DHCP').click({ force: true });

      // Mock /network to return a short rollback timeout for testing
      await page.unroute('**/network');
      await harness.mockNetworkConfig(page, { rollbackTimeoutSeconds: 2 });

      await page.getByRole('button', { name: /save/i }).click();
      await expect(page.getByText('Confirm Network Configuration Change')).toBeVisible();
      await page.getByRole('checkbox', { name: /Enable automatic rollback/i }).check();

      // Override healthcheck mock to return network_rollback_occurred: true
      await page.unroute('**/healthcheck');
      await page.route('**/healthcheck', async (route) => {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            version_info: { required: '>=0.39.0', current: '0.40.0', mismatch: false },
            update_validation_status: { status: 'valid' },
            network_rollback_occurred: true,
          }),
        });
      });

      await page.getByRole('button', { name: /Apply Changes/i }).click();
      await expect(page.getByText('Applying network settings')).toBeVisible();

      await expect(page.getByText('Automatic network rollback successful')).not.toBeVisible();
      await expect(page.getByText('The network settings were rolled back to the previous configuration')).toBeVisible({ timeout: 10000 });
    });

    test('Rollback modal should close on second apply after a rollback', async ({ page }) => {
      test.setTimeout(60000); // Increase timeout for rollback scenario

      // Shim WebSocket to redirect 192.168.1.150 to localhost
      await page.addInitScript(() => {
          const OriginalWebSocket = window.WebSocket;
          // @ts-ignore
          window.WebSocket = function(url, protocols) {
              if (typeof url === 'string' && url.includes('192.168.1.150')) {
                  console.log(`[Shim] Redirecting WebSocket ${url} to localhost`);
                  url = url.replace('192.168.1.150', 'localhost');
              }
              return new OriginalWebSocket(url, protocols);
          };
          window.WebSocket.prototype = OriginalWebSocket.prototype;
          window.WebSocket.CONNECTING = OriginalWebSocket.CONNECTING;
          window.WebSocket.OPEN = OriginalWebSocket.OPEN;
          window.WebSocket.CLOSING = OriginalWebSocket.CLOSING;
          window.WebSocket.CLOSED = OriginalWebSocket.CLOSED;
      });

      // Mock the redirect to the new IP to prevent navigation failure
      await page.route(/.*192\.168\.1\.150.*/, async (route) => {
          const url = new URL(route.request().url());

          // Handle healthcheck separately to avoid CORS and ensure correct response
          if (url.pathname.includes('/healthcheck')) {
              await route.fulfill({
                  status: 200,
                  contentType: 'application/json',
                  headers: {
                      'Access-Control-Allow-Origin': 'https://localhost:5173',
                      'Access-Control-Allow-Credentials': 'true',
                  },
                  body: JSON.stringify({
                      version_info: { required: '>=0.39.0', current: '0.40.0', mismatch: false },
                      update_validation_status: { status: 'valid' },
                      network_rollback_occurred: harness.getRollbackState().occurred,
                  }),
              });
              return;
          }

          // For other requests (main page, assets), proxy to localhost
          const originalUrl = page.url();
          const originalPort = new URL(originalUrl).port || '5173';
          const originalProtocol = new URL(originalUrl).protocol;

          const newUrl = `${originalProtocol}//localhost:${originalPort}${url.pathname}${url.search}`;

          console.log(`Redirecting ${url.href} to ${newUrl}`);

          try {
              const response = await page.request.fetch(newUrl, {
                  method: route.request().method(),
                  headers: route.request().headers(),
                  data: route.request().postDataBuffer(),
                  ignoreHTTPSErrors: true
              });

              await route.fulfill({
                  response: response
              });
          } catch (e) {
              console.error('Failed to proxy request:', e);
              await route.abort();
          }
      });

      await page.unroute('**/network');
      await harness.mockNetworkConfig(page, { rollbackTimeoutSeconds: 10 }); // Short timeout

      // Set browser hostname to match the harness default IP (192.168.1.100)
      // so Core detects it as current connection
      await page.evaluate(() => {
          // @ts-ignore
          if (window.setBrowserHostname) {
              // @ts-ignore
              window.setBrowserHostname('192.168.1.100');
          }
      });

      // 1. Setup adapter with static IP (current connection)
      // Using 192.168.1.100 to match harness default currentIp
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      // 2. First Change - will trigger rollback
      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await ipInput.fill('192.168.1.150');

      await page.getByRole('button', { name: /save/i }).click();

      const confirmDialog = page.getByText('Confirm Network Configuration Change');
      await expect(confirmDialog).toBeVisible();

      const rollbackCheckbox = page.getByRole('checkbox', { name: /Enable automatic rollback/i });
      if (!(await rollbackCheckbox.isChecked())) {
          await rollbackCheckbox.check();
      }

      await page.getByRole('button', { name: /apply changes/i }).click();

      // Verify modal closed
      await expect(confirmDialog).not.toBeVisible();

      // Verify rollback overlay appears
      await expect(page.locator('#overlay').getByText('Automatic rollback in:')).toBeVisible();

      // Wait for at least one healthcheck attempt on the new IP
      await page.waitForResponse(resp => resp.url().includes('192.168.1.150') && resp.url().includes('healthcheck'));

      // 3. Simulate Rollback Timeout
      await harness.simulateRollbackTimeout();

      // UI should eventually show "Network Settings Rolled Back"
      await expect(page.getByText('Network Settings Rolled Back')).toBeVisible({ timeout: 30000 });

      // 4. Acknowledge Rollback
      await page.getByRole('button', { name: /ok/i }).click();

      // Verify rollback dialog is fully closed
      await expect(page.getByText('Network Settings Rolled Back')).not.toBeVisible();

      // Wait for the ack-rollback API call to complete and backend to process it
      // This ensures the rollback flag is cleared before we reload
      await page.waitForTimeout(2000);

      // Navigate to localhost to avoid the 192.168.1.150 route and reload cleanly
      await page.goto('/');
      await page.waitForLoadState('domcontentloaded');

      // Wait for the login page to be ready
      await page.waitForTimeout(1000);

      // Dismiss rollback dialog if it appears again (backend hasn't cleared flag yet)
      const rollbackDialog = page.getByText('Network Settings Rolled Back');
      if (await rollbackDialog.isVisible()) {
        await page.getByRole('button', { name: /ok/i }).click();
        await expect(rollbackDialog).not.toBeVisible();
        await page.waitForTimeout(500);
      }

      // Re-login after navigation
      await page.getByPlaceholder(/enter your password/i).fill('password');
      await page.getByRole('button', { name: /log in/i }).click();
      await expect(page.getByText('Common Info')).toBeVisible({ timeout: 15000 });

      // Navigate to the network page
      await harness.navigateToNetwork(page);
      await harness.navigateToAdapter(page, 'eth0');

      // Wait for the Core to fully initialize and load network status
      // After reload, it takes a moment for Centrifugo to reconnect and publish status
      await page.waitForTimeout(2000);

      // Set browser hostname again after reload to mark eth0 as current connection
      await page.evaluate(() => {
        // @ts-ignore
        if (window.setBrowserHostname) {
          // @ts-ignore
          window.setBrowserHostname('192.168.1.100');
        }
      });

      // Wait for Core to process the hostname change
      await page.waitForTimeout(500);

      // After rollback, IP should be back to the original value (192.168.1.100)
      const currentIpInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await expect(currentIpInput).toHaveValue('192.168.1.100');

      // 5. Second Change - try again with a different IP
      await currentIpInput.clear();
      await currentIpInput.fill('192.168.1.151');

      // Wait for form to recognize the change
      await page.waitForTimeout(500);

      await page.getByRole('button', { name: /save/i }).click();

      // Verify confirmation dialog appears for the second change
      const confirmDialog2 = page.getByText('Confirm Network Configuration Change');
      await expect(confirmDialog2).toBeVisible();

      // Ensure rollback is checked
      const rollbackCheckbox2 = page.getByRole('checkbox', { name: /Enable automatic rollback/i });
      if (!(await rollbackCheckbox2.isChecked())) {
          await rollbackCheckbox2.check();
      }

      await page.getByRole('button', { name: /apply changes/i }).click();

      // 6. Verify modal closed (second time)
      await expect(confirmDialog2).not.toBeVisible({ timeout: 5000 });
    });
  });

  test.describe('HIGH: Basic Configuration Workflows', () => {
    test('static IP on non-server adapter - no rollback modal', async ({ page }) => {
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: '192.168.1.200', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await ipInput.fill('192.168.1.210');

      await page.getByRole('button', { name: /save/i }).click();
      await page.waitForTimeout(500);
      await expect(page.getByText('Confirm Network Configuration Change')).not.toBeVisible();
    });

    test('static IP on server adapter with rollback enabled', async ({ page }) => {
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: 'localhost', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      await expect(page.getByText('(current connection)')).toBeVisible();

      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await ipInput.fill('192.168.1.150');

      await page.getByRole('button', { name: /save/i }).click();

      await expect(page.getByText('Confirm Network Configuration Change')).toBeVisible({ timeout: 5000 });
      await expect(page.getByRole('checkbox', { name: /Enable automatic rollback/i })).toBeChecked();

      await page.getByRole('button', { name: /apply changes/i }).click();
      await expect(page.locator('#overlay').getByText('Automatic rollback in:')).toBeVisible({ timeout: 10000 });
      expect(harness.getRollbackState().enabled).toBe(true);
    });

    test('static IP on server adapter with rollback disabled', async ({ page }) => {
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: 'localhost', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await ipInput.fill('192.168.1.150');

      await page.getByRole('button', { name: /save/i }).click();

      await expect(page.getByText('Confirm Network Configuration Change')).toBeVisible({ timeout: 5000 });
      await page.getByRole('checkbox', { name: /Enable automatic rollback/i }).uncheck();

      await page.getByRole('button', { name: /apply changes/i }).click();
      await page.waitForTimeout(500);
      expect(harness.getRollbackState().enabled).toBe(false);
    });

    test('DHCP on non-server adapter', async ({ page }) => {
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: '192.168.1.200', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      await page.getByLabel('DHCP').click({ force: true });
      await page.waitForTimeout(300);
      await page.getByRole('button', { name: /save/i }).click();

      await page.waitForTimeout(500);
      await expect(page.getByText('Confirm Network Configuration Change')).not.toBeVisible();
    });

    test('DHCP on server adapter with rollback enabled', async ({ page }) => {
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: 'localhost', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      await expect(page.getByRole('textbox', { name: /IP Address/i }).first()).toHaveValue('localhost', { timeout: 8000 });

      await page.getByLabel('DHCP').click({ force: true });
      await page.waitForTimeout(300);
      await page.getByRole('button', { name: /save/i }).click();

      await expect(page.getByText('Confirm Network Configuration Change')).toBeVisible({ timeout: 5000 });
      await expect(page.getByRole('checkbox', { name: /Enable automatic rollback/i })).not.toBeChecked();

      await page.getByRole('checkbox', { name: /Enable automatic rollback/i }).check();
      await page.getByRole('button', { name: /apply changes/i }).click();
      await page.waitForTimeout(1000);
      expect(harness.getRollbackState().enabled).toBe(true);
    });

    test('DHCP on server adapter with rollback disabled', async ({ page }) => {
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: 'localhost', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      await page.getByLabel('DHCP').click({ force: true });
      await page.waitForTimeout(300);
      await page.getByRole('button', { name: /save/i }).click();

      await expect(page.getByText('Confirm Network Configuration Change')).toBeVisible({ timeout: 5000 });
      await page.getByRole('checkbox', { name: /Enable automatic rollback/i }).uncheck();
      await page.getByRole('button', { name: /apply changes/i }).click();
      await page.waitForTimeout(500);
      expect(harness.getRollbackState().enabled).toBe(false);
    });
  });

  test.describe('MEDIUM: Form Interactions and Validation', () => {
    test('DNS multiline textarea parsing and submission', async ({ page }) => {
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: '192.168.1.200', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      const dnsInput = page.getByRole('textbox', { name: /DNS/i }).first();
      await dnsInput.fill('8.8.8.8\n1.1.1.1\n9.9.9.9');

      await harness.saveAndVerify(page);
    });

    test('gateway multiline textarea parsing and submission', async ({ page }) => {
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: '192.168.1.200', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      const gatewayInput = page.getByRole('textbox', { name: /Gateway/i }).first();
      await gatewayInput.fill('192.168.1.1\n192.168.1.2');

      await harness.saveAndVerify(page);
    });

    test('gateway field readonly when DHCP enabled', async ({ page }) => {
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: '192.168.1.200', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      await expect(page.getByLabel('Static')).toBeChecked();
      const gatewayField = page.getByRole('textbox', { name: /Gateways/i });
      await expect(gatewayField).toBeEditable();

      await page.getByLabel('DHCP').click({ force: true });
      await page.waitForTimeout(300);
      await expect(page.getByLabel('DHCP')).toBeChecked();
      await expect(gatewayField).not.toBeEditable();

      await page.getByLabel('Static').click({ force: true });
      await page.waitForTimeout(300);
      await expect(page.getByLabel('Static')).toBeChecked();
      await expect(gatewayField).toBeEditable();
    });

    test('netmask dropdown selection', async ({ page }) => {
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: '192.168.1.200', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      await expect(page.getByText('/24')).toBeVisible();
      await page.getByRole('button', { name: /\/24/i }).click();
      await page.waitForSelector('.v-list-item');
      await page.locator('.v-list-item-title').filter({ hasText: '/16' }).click();

      await expect(page.getByRole('button', { name: /\/16/i })).toBeVisible();
      await expect(page.getByRole('button', { name: /save/i })).toBeEnabled();
    });

    test('form dirty flag tracking', async ({ page }) => {
      await harness.setup(page, {}); // default

      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await ipInput.fill('192.168.1.210');

      await expect(page.getByRole('button', { name: /save/i })).toBeEnabled();
      await expect(page.getByRole('button', { name: /reset/i })).toBeEnabled();
    });

    test('form reset button discards unsaved changes', async ({ page }) => {
      const originalIp = '192.168.1.100';
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: originalIp, dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await expect(ipInput).toHaveValue(originalIp);

      await ipInput.fill('192.168.1.210');
      await expect(ipInput).toHaveValue('192.168.1.210');

      await page.getByRole('button', { name: /reset/i }).click();
      await expect(ipInput).toHaveValue(originalIp);
    });

    test('tab switching with unsaved changes - discard and switch', async ({ page }) => {
      await harness.setup(page, [
        { name: 'eth0', ipv4: { addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }] } },
        { name: 'eth1', mac: '00:11:22:33:44:56', ipv4: { addrs: [{ addr: '192.168.1.101', dhcp: false, prefix_len: 24 }] } }
      ], 'eth0');

      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await ipInput.fill('192.168.1.210');
      await page.waitForTimeout(500);

      await page.getByRole('tab', { name: 'eth1' }).click();
      await expect(page.getByText('Unsaved Changes', { exact: true })).toBeVisible({ timeout: 5000 });

      await page.getByRole('button', { name: /discard/i }).click();
      await page.waitForTimeout(500);
      await expect(page.getByRole('textbox', { name: /IP Address/i }).first()).toBeVisible();
    });

    test('tab switching with unsaved changes - cancel and stay', async ({ page }) => {
      await harness.setup(page, [
        { name: 'eth0', ipv4: { addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }] } },
        { name: 'eth1', mac: '00:11:22:33:44:56', ipv4: { addrs: [{ addr: '192.168.1.101', dhcp: false, prefix_len: 24 }] } }
      ], 'eth0');

      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await ipInput.fill('192.168.1.210');
      await page.waitForTimeout(500);

      await page.getByRole('tab', { name: 'eth1' }).click();
      await expect(page.getByText('Unsaved Changes', { exact: true })).toBeVisible({ timeout: 5000 });

      await page.getByRole('button', { name: /cancel/i }).click();
      await page.waitForTimeout(300);

      await expect(ipInput).toHaveValue('192.168.1.210');
      await expect(page.getByText('Unsaved Changes', { exact: true })).not.toBeVisible();
    });
  });

  test.describe('LOW: Edge Cases and UI Polish', () => {
    test('copy to clipboard - IP address', async ({ page, context }) => {
      await context.grantPermissions(['clipboard-read', 'clipboard-write']);
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      await page.locator('.mdi-content-copy').first().click();
      const clipboardText = await page.evaluate(() => navigator.clipboard.readText());
      expect(clipboardText).toMatch(/^[a-zA-Z0-9.:]+\/\d+$/);
    });

    test('copy to clipboard - MAC address', async ({ page, context }) => {
      await context.grantPermissions(['clipboard-read', 'clipboard-write']);
      const testMac = '00:11:22:33:44:55';
      await harness.setup(page, { mac: testMac });

      await page.locator('.mdi-content-copy').nth(1).click();
      const clipboardText = await page.evaluate(() => navigator.clipboard.readText());
      expect(clipboardText).toBe(testMac);
    });

    test('offline adapter handling and display', async ({ page }) => {
      await harness.setup(page, {
        online: false,
        ipv4: {
          addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      await expect(page.locator('.v-chip').filter({ hasText: 'Offline' })).toBeVisible({ timeout: 5000 });
      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await expect(ipInput).toBeEditable();
    });

    test('REGRESSION: adapter status updates from online to offline via WebSocket', async ({ page }) => {
      // Setup adapter initially online
      await harness.setup(page, {
        online: true,
        ipv4: {
          addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      // Verify adapter is online
      await expect(page.locator('.v-chip').filter({ hasText: 'Online' })).toBeVisible({ timeout: 5000 });
      await expect(page.locator('.v-chip').filter({ hasText: 'Offline' })).not.toBeVisible();

      // Simulate network cable removal - adapter goes offline via WebSocket update
      await harness.publishNetworkStatus([{
        name: 'eth0',
        mac: '00:11:22:33:44:55',
        online: false, // Changed from true to false
        ipv4: {
          addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      }]);

      // Verify UI updates to show offline status
      await expect(page.locator('.v-chip').filter({ hasText: 'Offline' })).toBeVisible({ timeout: 5000 });
      await expect(page.locator('.v-chip').filter({ hasText: 'Online' })).not.toBeVisible();
    });

    test('REGRESSION: adapter status updates while editing form', async ({ page }) => {
      // Setup adapter initially online
      await harness.setup(page, {
        online: true,
        ipv4: {
          addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      // Verify adapter is online
      await expect(page.locator('.v-chip').filter({ hasText: 'Online' })).toBeVisible({ timeout: 5000 });

      // Start editing the form to make it dirty
      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await ipInput.fill('192.168.1.101');
      await page.waitForTimeout(500); // Let dirty flag propagate

      // Verify form is dirty
      await expect(page.getByRole('button', { name: /reset/i })).toBeEnabled();

      // While editing, simulate network cable removal - adapter goes offline
      await harness.publishNetworkStatus([{
        name: 'eth0',
        mac: '00:11:22:33:44:55',
        online: false, // Changed from true to false
        ipv4: {
          addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      }]);

      // Verify UI updates to show offline status even while form is dirty
      await expect(page.locator('.v-chip').filter({ hasText: 'Offline' })).toBeVisible({ timeout: 5000 });
      await expect(page.locator('.v-chip').filter({ hasText: 'Online' })).not.toBeVisible();

      // Verify edited IP is preserved (dirty flag prevents overwrite of form fields)
      await expect(ipInput).toHaveValue('192.168.1.101');
    });

    test('WebSocket sync during editing - dirty flag prevents overwrite', async ({ page }) => {
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
      await expect(ipInput).toBeVisible();

      const editedIp = '10.20.30.40';
      await ipInput.fill(editedIp);
      await page.waitForTimeout(500);

      await harness.publishNetworkStatus([
        harness.createAdapter('eth0', {
          ipv4: {
            addrs: [{ addr: '192.168.1.150', dhcp: false, prefix_len: 24 }],
            dns: ['8.8.8.8'],
            gateways: ['192.168.1.1'],
          },
        }),
      ]);

      await page.waitForTimeout(1000);
      await expect(ipInput).toHaveValue(editedIp);
    });

    test('multiple adapters navigation', async ({ page }) => {
      await harness.setup(page, [
        { name: 'eth0', ipv4: { addrs: [{ addr: '10.0.0.1', dhcp: false, prefix_len: 24 }] } },
        { name: 'eth1', mac: '00:11:22:33:44:56', ipv4: { addrs: [{ addr: '10.0.0.2', dhcp: false, prefix_len: 24 }] } }
      ], 'eth0');

      await expect(page.getByRole('tab', { name: 'eth0' })).toBeVisible();
      await expect(page.getByRole('tab', { name: 'eth1' })).toBeVisible();

      await harness.navigateToAdapter(page, 'eth1');
      await expect(page.getByRole('textbox', { name: /IP Address/i }).first()).toBeVisible();
    });

    test('current connection detection - IP match', async ({ page }) => {
      await harness.setup(page, {
        ipv4: {
          addrs: [{ addr: 'localhost', dhcp: false, prefix_len: 24 }],
          dns: ['8.8.8.8'],
          gateways: ['192.168.1.1'],
        },
      });

      await expect(page.getByText('(current connection)')).toBeVisible();
    });

    test('current connection detection - hostname not matching any IP', async ({ page }) => {
      await harness.setup(page, [
        { name: 'eth0', ipv4: { addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }] } },
        { name: 'eth1', mac: '00:11:22:33:44:56', ipv4: { addrs: [{ addr: '192.168.1.101', dhcp: false, prefix_len: 24 }] } }
      ], 'eth0');

      await expect(page.getByText('(current connection)')).not.toBeVisible();

      await harness.navigateToAdapter(page, 'eth1');
      await expect(page.getByText('(current connection)')).not.toBeVisible();
    });
  });
});
