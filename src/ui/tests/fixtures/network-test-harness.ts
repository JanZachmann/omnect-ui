import { Page, expect } from '@playwright/test';
import { publishToWebsocket } from './websocket';
import { mockConfig, mockLoginSuccess, mockRequireSetPassword } from './mock-api';

/**
 * Polling interval used by the Core for new-IP healthcheck requests.
 * This matches NEW_IP_POLL_INTERVAL_MS in src/app/src/update/device/network/verification.rs.
 */
export const HEALTHCHECK_POLL_INTERVAL_MS = 5000;

/**
 * Default rollback timeout in seconds if not specified
 */
export const DEFAULT_ROLLBACK_TIMEOUT_SECONDS = 90;

/**
 * Default UI port for the preview server
 */
export const DEFAULT_UI_PORT = 5173;

/**
 * Network adapter configuration as received from the device
 */
export interface DeviceNetwork {
  name: string;
  mac: string;
  online: boolean;
  ipv4?: {
    addrs: Array<{ addr: string; dhcp: boolean; prefix_len: number }>;
    dns: string[];
    gateways: string[];
  };
}

/**
 * Configuration options for the network test harness
 */
export interface NetworkTestHarnessConfig {
  /** Rollback timeout in seconds (default: DEFAULT_ROLLBACK_TIMEOUT_SECONDS) */
  rollbackTimeoutSeconds?: number;
  /** Whether to enable healthcheck polling (default: true) */
  enableHealthcheckPolling?: boolean;
  /**
   * Time in milliseconds after which healthcheck should succeed.
   * Uses time-based calculation for accuracy regardless of poll interval.
   */
  healthcheckSuccessAfter?: number;
  /** If true, healthcheck always fails (default: false) */
  healthcheckAlwaysFails?: boolean;
}

/**
 * Response from the /network POST endpoint
 */
export interface SetNetworkConfigResponse {
  rollbackTimeoutSeconds: number;
  uiPort: number;
  rollbackEnabled: boolean;
}

/**
 * Test harness for network configuration E2E tests.
 *
 * Provides utilities for:
 * - Mocking /network and /healthcheck endpoints
 * - Publishing network status via WebSocket
 * - Simulating rollback scenarios
 * - Creating test adapter configurations
 *
 * @example
 * ```typescript
 * const harness = new NetworkTestHarness();
 * await harness.mockNetworkConfig(page, { rollbackTimeoutSeconds: 30 });
 * await harness.mockHealthcheck(page, { healthcheckSuccessAfter: 6000 });
 * await harness.publishNetworkStatus([harness.createAdapter('eth0')]);
 * ```
 */
export class NetworkTestHarness {
  private rollbackEnabled: boolean = false;
  private rollbackDeadline: number | null = null;
  private currentIp: string = '192.168.1.100';
  private newIp: string | null = null;
  private healthcheckCallCount: number = 0;
  private healthcheckStartTime: number | null = null;
  private healthcheckConfig: NetworkTestHarnessConfig = {};
  private networkRollbackOccurred: boolean = false;
  private lastNetworkConfig: DeviceNetwork[] = [];

  /**
   * Mock the /network endpoint with configurable response.
   *
   * @param page - Playwright page instance
   * @param config - Configuration options for the mock
   */
  async mockNetworkConfig(page: Page, config: NetworkTestHarnessConfig = {}): Promise<void> {
    await page.unroute('**/network');
    await page.route('**/network', async (route) => {
      if (route.request().method() === 'POST') {
        const requestBody = route.request().postDataJSON();

        // Track rollback state
        this.rollbackEnabled = requestBody.enableRollback === true;

        // If rollback enabled, set deadline
        if (this.rollbackEnabled) {
          const timeoutSeconds = config.rollbackTimeoutSeconds ?? DEFAULT_ROLLBACK_TIMEOUT_SECONDS;
          this.rollbackDeadline = Date.now() + (timeoutSeconds * 1000);
        }

        // Track IP change
        if (requestBody.ip && requestBody.ip !== this.currentIp) {
          this.newIp = requestBody.ip;
        }

        // Reset healthcheck state
        this.healthcheckCallCount = 0;
        this.healthcheckStartTime = null;
        this.healthcheckConfig = config;

        // Send success response
        const response: SetNetworkConfigResponse = {
          rollbackTimeoutSeconds: config.rollbackTimeoutSeconds ?? DEFAULT_ROLLBACK_TIMEOUT_SECONDS,
          uiPort: DEFAULT_UI_PORT,
          rollbackEnabled: this.rollbackEnabled,
        };

        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify(response),
        });
      } else {
        await route.continue();
      }
    });
  }

  /**
   * Mock the /network endpoint to return an error.
   *
   * @param page - Playwright page instance
   * @param statusCode - HTTP status code to return (default: 500)
   * @param errorMessage - Error message to include in response (default: 'Failed to apply network configuration')
   */
  async mockNetworkConfigError(
    page: Page,
    statusCode: number = 500,
    errorMessage: string = 'Failed to apply network configuration. Please check your settings and try again.'
  ): Promise<void> {
    await page.route('**/network', async (route) => {
      if (route.request().method() === 'POST') {
        await route.fulfill({
          status: statusCode,
          contentType: 'application/json',
          body: JSON.stringify({ error: errorMessage }),
        });
      } else {
        await route.continue();
      }
    });
  }

  /**
   * Mock the /healthcheck endpoint with configurable responses.
   *
   * The healthcheckSuccessAfter option uses real time measurement rather than
   * counting poll requests, making tests more reliable regardless of polling interval.
   *
   * @param page - Playwright page instance
   * @param config - Configuration options for the mock
   *
   * @example
   * ```typescript
   * // Healthcheck succeeds immediately (default)
   * await harness.mockHealthcheck(page);
   *
   * // Healthcheck succeeds after 6 seconds
   * await harness.mockHealthcheck(page, { healthcheckSuccessAfter: 6000 });
   *
   * // Healthcheck always fails (for rollback testing)
   * await harness.mockHealthcheck(page, { healthcheckAlwaysFails: true });
   * ```
   */
  async mockHealthcheck(page: Page, config: NetworkTestHarnessConfig = {}): Promise<void> {
    this.healthcheckConfig = config;
    await page.unroute('**/healthcheck');

    await page.route('**/healthcheck', async (route) => {
      this.healthcheckCallCount++;

      // Track start time on first healthcheck request
      if (this.healthcheckStartTime === null) {
        this.healthcheckStartTime = Date.now();
      }

      // Determine if healthcheck should succeed
      let healthcheckSucceeds = false;

      if (config.healthcheckAlwaysFails) {
        healthcheckSucceeds = false;
      } else if (config.healthcheckSuccessAfter !== undefined) {
        // Use actual elapsed time for more accurate test timing
        const elapsedMs = Date.now() - this.healthcheckStartTime;
        healthcheckSucceeds = elapsedMs >= config.healthcheckSuccessAfter;
      } else {
        // Default: succeed immediately
        healthcheckSucceeds = true;
      }

      if (healthcheckSucceeds) {
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
            networkRollbackOccurred: this.networkRollbackOccurred,
          }),
        });
      } else {
        await route.abort();
      }
    });
  }

  /**
   * Mock the /ack-rollback endpoint.
   * When called, clears the networkRollbackOccurred flag.
   *
   * @param page - Playwright page instance
   */
  async mockAckRollback(page: Page): Promise<void> {
    await page.route('**/ack-rollback', async (route) => {
      if (route.request().method() === 'POST') {
        this.networkRollbackOccurred = false;
        await route.fulfill({
          status: 200,
        });
      }
    });
  }

  /** Mock a POST-only endpoint to return 200 with no body. */
  private async mockAckEndpoint(page: Page, path: string): Promise<void> {
    await page.route(`**/${path}`, async (route) => {
      if (route.request().method() === 'POST') {
        await route.fulfill({ status: 200 });
      }
    });
  }

  /**
   * Mock the /ack-factory-reset-result endpoint.
   *
   * @param page - Playwright page instance
   */
  async mockAckFactoryResetResult(page: Page): Promise<void> {
    await this.mockAckEndpoint(page, 'ack-factory-reset-result');
  }

  /**
   * Mock the /ack-update-validation endpoint.
   *
   * @param page - Playwright page instance
   */
  async mockAckUpdateValidation(page: Page): Promise<void> {
    await this.mockAckEndpoint(page, 'ack-update-validation');
  }

  /**
   * Publish network status via WebSocket.
   * Updates are received by the UI and trigger network adapter list refresh.
   *
   * @param adapters - Array of network adapter configurations to publish
   */
  async publishNetworkStatus(adapters: Array<Partial<DeviceNetwork> & { name: string }>): Promise<void> {
    const fullAdapters = adapters.map(adapter => this.createAdapter(adapter.name, adapter));
    this.lastNetworkConfig = fullAdapters;
    await publishToWebsocket('NetworkStatusV1', {
      network_status: fullAdapters,
    });
  }

  /**
   * Simulate automatic rollback after timeout.
   *
   * This simulates the device-side rollback that occurs when the user doesn't
   * confirm the new network configuration within the rollback timeout period.
   *
   * Effects:
   * - Sets the networkRollbackOccurred flag (reported in healthcheck response)
   * - Reverts IP addresses to original values
   * - Publishes updated network status via WebSocket
   *
   * @throws Error if rollback was not enabled when network config was applied
   */
  async simulateRollbackTimeout(): Promise<void> {
    if (!this.rollbackEnabled) {
      throw new Error('Cannot simulate rollback timeout: rollback was not enabled when the network configuration was applied');
    }

    this.networkRollbackOccurred = true;
    this.rollbackEnabled = false;
    this.rollbackDeadline = null;

    // Revert IP to original
    if (this.newIp) {
      this.newIp = null;
    }

    // Publish reverted network status
    const revertedAdapters = this.lastNetworkConfig.map(adapter => ({
      ...adapter,
      ipv4: adapter.ipv4 ? {
        ...adapter.ipv4,
        addrs: adapter.ipv4.addrs.map(addr => ({
          ...addr,
          addr: this.currentIp,
        })),
      } : undefined,
    }));

    await this.publishNetworkStatus(revertedAdapters);
  }

  /**
   * Simulate successful connection to the new IP address.
   *
   * This simulates the scenario where the user successfully connects to the
   * device at its new IP address, canceling the automatic rollback.
   *
   * Effects:
   * - Cancels the rollback timer
   * - Updates currentIp to the new value
   *
   * @throws Error if rollback was not enabled when network config was applied
   */
  async simulateNewIpReachable(): Promise<void> {
    if (!this.rollbackEnabled) {
      throw new Error('Cannot simulate new IP reachable: rollback was not enabled when the network configuration was applied');
    }

    // Cancel rollback
    this.rollbackEnabled = false;
    this.rollbackDeadline = null;

    // Update current IP to new IP
    if (this.newIp) {
      this.currentIp = this.newIp;
      this.newIp = null;
    }
  }

  /**
   * Create a standard network adapter with customizable configuration.
   *
   * @param name - Adapter name (e.g., 'eth0', 'wlan0')
   * @param config - Optional configuration overrides
   * @returns A DeviceNetwork object ready for publishing
   *
   * @example
   * ```typescript
   * // Create adapter with defaults
   * const eth0 = harness.createAdapter('eth0');
   *
   * // Create adapter with custom IP
   * const eth1 = harness.createAdapter('eth1', {
   *   ipv4: {
   *     addrs: [{ addr: '10.0.0.1', dhcp: false, prefix_len: 24 }],
   *     dns: ['8.8.8.8'],
   *     gateways: ['10.0.0.254'],
   *   },
   * });
   * ```
   */
  createAdapter(name: string, config: Partial<DeviceNetwork> = {}): DeviceNetwork {
    const defaultIpv4 = {
      addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }],
      dns: ['8.8.8.8'],
      gateways: ['192.168.1.1'],
    };

    return {
      name,
      mac: config.mac || '00:11:22:33:44:55',
      online: config.online !== undefined ? config.online : true,
      ipv4: config.ipv4 ? { ...defaultIpv4, ...config.ipv4 } : defaultIpv4,
    };
  }

  /**
   * Create multiple network adapters for testing multi-adapter scenarios.
   *
   * @param count - Number of adapters to create (max: 5)
   * @returns Array of DeviceNetwork objects with unique names, MACs, and IPs
   */
  createMultipleAdapters(count: number): DeviceNetwork[] {
    const names = ['eth0', 'eth1', 'wlan0', 'eth2', 'wlan1'];
    const macs = [
      '00:11:22:33:44:55',
      '00:11:22:33:44:56',
      '00:11:22:33:44:57',
      '00:11:22:33:44:58',
      '00:11:22:33:44:59',
    ];

    return Array.from({ length: Math.min(count, 5) }, (_, i) => ({
      name: names[i],
      mac: macs[i],
      online: true,
      ipv4: {
        addrs: [{ addr: `192.168.1.${100 + i}`, dhcp: false, prefix_len: 24 }],
        dns: ['8.8.8.8'],
        gateways: ['192.168.1.1'],
      },
    }));
  }

  /**
   * Set the rollback occurred flag for testing rollback notification scenarios.
   *
   * @param occurred - Whether a rollback has occurred
   */
  setRollbackOccurred(occurred: boolean): void {
    this.networkRollbackOccurred = occurred;
  }

  /**
   * Get current rollback state for test assertions.
   *
   * @returns Object containing rollback state information
   */
  getRollbackState(): { enabled: boolean; deadline: number | null; occurred: boolean } {
    return {
      enabled: this.rollbackEnabled,
      deadline: this.rollbackDeadline,
      occurred: this.networkRollbackOccurred,
    };
  }

  /**
   * Get the number of healthcheck requests received.
   * Useful for verifying polling behavior.
   *
   * @returns Number of healthcheck requests
   */
  getHealthcheckCallCount(): number {
    return this.healthcheckCallCount;
  }

  /**
   * Navigate to the Network section in the UI.
   *
   * @param page - Playwright page instance
   */
  async navigateToNetwork(page: Page): Promise<void> {
    // Ensure no blocking overlays are visible
    const overlay = page.locator('.v-overlay--active');
    if (await overlay.count() > 0) {
        await expect(overlay).not.toBeVisible({ timeout: 10000 });
    }

    const networkLink = page.getByText('Network', { exact: true });
    await expect(networkLink).toBeVisible({ timeout: 10000 });
    
    // Use dispatchEvent to bypass potential occlusion issues
    await networkLink.dispatchEvent('click');
    
    // Wait for the Network header to be visible to confirm navigation
    await expect(page.locator('.text-h4', { hasText: 'Network' })).toBeVisible({ timeout: 10000 });
  }

  /**
   * Navigate to a specific network adapter tab.
   *
   * @param page - Playwright page instance
   * @param adapterName - Name of the adapter to select
   */
  async navigateToAdapter(page: Page, adapterName: string): Promise<void> {
    const tab = page.getByRole('tab', { name: adapterName }).or(page.locator('.v-tab').filter({ hasText: new RegExp(`^${adapterName}$`) }));
    await expect(tab).toBeVisible({ timeout: 10000 });
    await tab.click();
    // Wait for form to load (IP input visible)
    await expect(page.getByRole('textbox', { name: /IP Address/i }).first()).toBeVisible({ timeout: 10000 });
  }

  /**
   * Combined helper to setup one or more adapters and navigate to one.
   *
   * @param page - Playwright page instance
   * @param configs - Single adapter config or array of configs
   * @param targetAdapter - Which adapter to navigate to (defaults to the first one)
   * @param options - Additional options (e.g., skipNavigation)
   */
  async setup(
    page: Page,
    configs: (Partial<DeviceNetwork> & { name?: string }) | Array<Partial<DeviceNetwork> & { name?: string }>,
    targetAdapter?: string,
    options: { skipNavigation?: boolean } = {}
  ): Promise<void> {
    const configArray = Array.isArray(configs) ? configs : [configs];
    const adapters = configArray.map((c, i) =>
      this.createAdapter(c.name || (i === 0 ? 'eth0' : `eth${i}`), c)
    );

    // Give UI a moment to be ready for WebSocket updates
    await page.waitForTimeout(100);
    await this.publishNetworkStatus(adapters);

    if (options.skipNavigation) {
      return;
    }

    await this.navigateToNetwork(page);
    await this.navigateToAdapter(page, targetAdapter || adapters[0].name);
  }

  /**
   * Helper to click Apply Changes and verify the operation completes successfully.
   *
   * @param page - Playwright page instance
   * @param timeout - Timeout for the operation to complete (default: 10000)
   */
  async saveAndVerify(page: Page, timeout: number = 10000): Promise<void> {
    const saveButton = page.locator('.v-window-item--active [data-cy=network-apply-button]');
    await saveButton.click();
    await expect(saveButton).toBeDisabled({ timeout });
    await expect(page.getByText('Network configuration updated')).toBeVisible();
  }

  /**
   * Standard beforeEach setup for network tests.
   *
   * Mocks auth and network API endpoints, suppresses unrelated modals via
   * sessionStorage, navigates to `/`, and performs login.
   *
   * @param page - Playwright page instance
   */
  async setupWithLogin(page: Page): Promise<void> {
    await mockConfig(page);
    await mockLoginSuccess(page);
    await mockRequireSetPassword(page);
    await this.mockNetworkConfig(page);
    await this.mockHealthcheck(page);
    await this.mockAckRollback(page);
    await page.route('**/version', route => route.fulfill({ status: 200, body: '0.0.0-test' }));

    await page.addInitScript(() => {
      sessionStorage.setItem('factoryResetResultAcked', 'true');
      sessionStorage.setItem('updateValidationAcked', 'true');
    });

    await page.goto('/');
    await page.getByPlaceholder(/enter your password/i).fill('password');
    await page.getByRole('button', { name: /log in/i }).click();
    await expect(page.getByText('Common Info')).toBeVisible({ timeout: 10000 });
  }

  /**
   * Intercept all traffic to a new IP address and proxy it to localhost.
   *
   * - Shims `WebSocket` via `addInitScript` so WebSocket connects to localhost.
   * - Routes all HTTP requests for `newIp` to the equivalent localhost path,
   *   with healthcheck responses reflecting current harness rollback state.
   *
   * Call this before any navigation to the new IP.
   *
   * @param page   - Playwright page instance
   * @param newIp  - IP address to intercept (e.g. '192.168.1.150')
   */
  async mockNewIpProxy(page: Page, newIp: string): Promise<void> {
    await page.addInitScript((ip: string) => {
      const OriginalWebSocket = window.WebSocket;
      // @ts-ignore
      window.WebSocket = function(url: string, protocols: any) {
        if (typeof url === 'string' && url.includes(ip)) {
          url = url.replace(ip, 'localhost');
        }
        return new OriginalWebSocket(url, protocols);
      };
      window.WebSocket.prototype = OriginalWebSocket.prototype;
      window.WebSocket.CONNECTING = OriginalWebSocket.CONNECTING;
      window.WebSocket.OPEN = OriginalWebSocket.OPEN;
      window.WebSocket.CLOSING = OriginalWebSocket.CLOSING;
      window.WebSocket.CLOSED = OriginalWebSocket.CLOSED;
    }, newIp);

    const escaped = newIp.replace(/\./g, '\\.');
    await page.route(new RegExp(`.*${escaped}.*`), async (route) => {
      const url = new URL(route.request().url());

      if (url.pathname.includes('/healthcheck')) {
        await route.fulfill({
          status: 200,
          contentType: 'application/json',
          headers: {
            'Access-Control-Allow-Origin': 'https://localhost:5173',
            'Access-Control-Allow-Credentials': 'true',
          },
          body: JSON.stringify({
            versionInfo: { required: '>=0.46.0', current: '0.46.0', mismatch: false },
            updateValidationStatus: { status: 'valid' },
            networkRollbackOccurred: this.networkRollbackOccurred,
          }),
        });
        return;
      }

      // Proxy all other requests (page, assets, API) to localhost
      const originalUrl = page.url();
      const originalPort = new URL(originalUrl).port || '5173';
      const originalProtocol = new URL(originalUrl).protocol;
      const proxiedUrl = `${originalProtocol}//localhost:${originalPort}${url.pathname}${url.search}`;

      try {
        const response = await page.request.fetch(proxiedUrl, {
          method: route.request().method(),
          headers: route.request().headers(),
          data: route.request().postDataBuffer(),
          ignoreHTTPSErrors: true,
        });
        await route.fulfill({ response });
      } catch (e) {
        console.error('Failed to proxy request:', e);
        await route.abort();
      }
    });
  }

  /**
   * Reset all harness state for test cleanup.
   * Call this in afterEach() to ensure test isolation.
   */
  reset(): void {
    this.rollbackEnabled = false;
    this.rollbackDeadline = null;
    this.currentIp = '192.168.1.100';
    this.newIp = null;
    this.healthcheckCallCount = 0;
    this.healthcheckStartTime = null;
    this.healthcheckConfig = {};
    this.networkRollbackOccurred = false;
    this.lastNetworkConfig = [];
  }
}
