import { test, expect } from '@playwright/test';
import { mockConfig, mockLoginSuccess, mockRequireSetPassword, mockNetworkConfig } from './fixtures/mock-api';
import { publishToCentrifugo } from './fixtures/centrifugo';

test.describe('Network Settings', () => {
  test.beforeEach(async ({ page }) => {
    // Listen for console logs
    page.on('console', msg => console.log(`BROWSER LOG: ${msg.text()}`));
    page.on('pageerror', err => console.log(`BROWSER ERROR: ${err}`));

    await mockConfig(page);
    await mockLoginSuccess(page);
    await mockRequireSetPassword(page);
    await mockNetworkConfig(page);
    
    await page.goto('/');
    
    // Login
    await page.getByPlaceholder(/enter your password/i).fill('password');
    await page.getByRole('button', { name: /log in/i }).click();
    await expect(page.getByText('Common Info')).toBeVisible();
    
    // Publish initial network status
    await publishToCentrifugo('NetworkStatusV1', {
      network_status: [
        {
          name: 'eth0',
          mac: '00:11:22:33:44:55',
          online: true,
          ipv4: {
            addrs: [{ addr: 'localhost', dhcp: true, prefix_len: 24 }],
            dns: ['8.8.8.8'],
            gateways: ['192.168.1.1'],
          },
        },
      ],
    });
  });

  test('shows rollback timer on configuration change', async ({ page }) => {
    // Navigate to Network page
    await page.getByText('Network').click();
    
    // Wait for network list
    await expect(page.getByText('eth0')).toBeVisible();
    
    // Open the interface details
    await page.getByText('eth0').click();
    
    // Switch to Static IP
    // It's a radio group, so we need to click the "Static" option
    await page.getByLabel('Static').click({ force: true });
    
    // Wait for IP Address field to be enabled/visible
    // The name might be "IP Address IP Address" due to Vuetify structure, so use regex
    const ipInput = page.getByRole('textbox', { name: /IP Address/i }).first();
    await expect(ipInput).toBeVisible();
    await expect(ipInput).toBeEditable();

    // Fill in static IP details
    await ipInput.fill('192.168.1.101');
    // Netmask is a dropdown, default is usually fine (24). Skipping interaction for simplicity.
    // await page.getByRole('button', { name: /\/24/ }).click(); // Example if we needed to change it
    
    await page.getByRole('textbox', { name: /Gateway/i }).first().fill('192.168.1.1');
    
    // Click Save (not Apply)
    await page.getByRole('button', { name: /save/i }).click();
    
    // Confirm dialog (title: Confirm Network Configuration Change)
    // Button: Apply Changes
    await expect(page.getByText('Confirm Network Configuration Change')).toBeVisible();
    await page.getByRole('button', { name: /apply changes/i }).click();
    
    // Wait for modal to close
    await expect(page.getByText('Confirm Network Configuration Change')).not.toBeVisible();
    
    // Assert Rollback Overlay appears with countdown label
    await expect(page.locator('#overlay').getByText('Automatic rollback in:')).toBeVisible({ timeout: 10000 });
  });
});
