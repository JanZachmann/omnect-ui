import { Page } from '@playwright/test';
import { publishToCentrifugo } from './centrifugo';

/**
 * Polling interval used by the application for healthcheck requests.
 * This must match NEW_IP_POLL_INTERVAL_MS in src/ui/src/composables/core/timers.ts
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
 * - Publishing network status via Centrifugo
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
            version_info: {
              required: '>=0.39.0',
              current: '0.40.0',
              mismatch: false,
            },
            update_validation_status: {
              status: 'valid',
            },
            network_rollback_occurred: this.networkRollbackOccurred,
          }),
        });
      } else {
        await route.fulfill({
          status: 503,
          contentType: 'application/json',
          body: JSON.stringify({ error: 'Device unreachable - connection timed out' }),
        });
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

  /**
   * Publish network status via Centrifugo WebSocket.
   * Updates are received by the UI and trigger network adapter list refresh.
   *
   * @param adapters - Array of network adapter configurations to publish
   */
  async publishNetworkStatus(adapters: DeviceNetwork[]): Promise<void> {
    this.lastNetworkConfig = adapters;
    await publishToCentrifugo('NetworkStatusV1', {
      network_status: adapters,
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
   * - Publishes updated network status via Centrifugo
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
    return {
      name,
      mac: config.mac || '00:11:22:33:44:55',
      online: config.online !== undefined ? config.online : true,
      ipv4: config.ipv4 || {
        addrs: [{ addr: '192.168.1.100', dhcp: false, prefix_len: 24 }],
        dns: ['8.8.8.8'],
        gateways: ['192.168.1.1'],
      },
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
