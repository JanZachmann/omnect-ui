/**
 * Timer management for device operations and network changes
 *
 * This module handles:
 * - Reconnection polling after reboot/factory reset
 * - New IP polling after network config changes
 * - Automatic timeout handling
 */

import { watch } from 'vue'
import { viewModel, isInitialized, wasmModule } from './state'
import type { Event } from '../../../../shared_types/generated/typescript/types/shared_types'
import {
	EventVariantDevice,
	DeviceEventVariantReconnectionCheckTick,
	DeviceEventVariantReconnectionTimeout,
	DeviceEventVariantNewIpCheckTick,
	DeviceEventVariantNewIpCheckTimeout,
} from '../../../../shared_types/generated/typescript/types/shared_types'

// Timer callback type - will be set by index.ts to avoid circular dependency
let sendEventCallback: ((event: Event) => Promise<void>) | null = null

/**
 * Set the event sender callback (called from index.ts after initialization)
 */
export function setEventSender(callback: (event: Event) => Promise<void>): void {
	sendEventCallback = callback
}

// ============================================================================
// Timer Constants
// ============================================================================

const RECONNECTION_POLL_INTERVAL_MS = 5000 // 5 seconds
const REBOOT_TIMEOUT_MS = 300000 // 5 minutes
const FACTORY_RESET_TIMEOUT_MS = 600000 // 10 minutes
const NEW_IP_POLL_INTERVAL_MS = 5000 // 5 seconds

// ============================================================================
// Timer IDs
// ============================================================================

let reconnectionIntervalId: ReturnType<typeof setInterval> | null = null
let reconnectionTimeoutId: ReturnType<typeof setTimeout> | null = null
let newIpIntervalId: ReturnType<typeof setInterval> | null = null
let newIpTimeoutId: ReturnType<typeof setTimeout> | null = null
let newIpCountdownIntervalId: ReturnType<typeof setInterval> | null = null

// Countdown deadline (Unix timestamp in milliseconds)
let countdownDeadline: number | null = null

// ============================================================================
// Reconnection Polling
// ============================================================================

/**
 * Start reconnection polling for reboot/factory reset
 * Sends ReconnectionCheckTick every 5 seconds and sets a timeout
 */
export function startReconnectionPolling(isFactoryReset: boolean): void {
	stopReconnectionPolling() // Clear any existing timers

	console.log(`[useCore] Starting reconnection polling (${isFactoryReset ? 'factory reset' : 'reboot'})`)

	// Start polling interval
	reconnectionIntervalId = setInterval(() => {
		if (isInitialized.value && wasmModule && sendEventCallback) {
			sendEventCallback(new EventVariantDevice(new DeviceEventVariantReconnectionCheckTick()))
		}
	}, RECONNECTION_POLL_INTERVAL_MS)

	// Set timeout (factory reset uses longer timeout)
	const timeoutMs = isFactoryReset ? FACTORY_RESET_TIMEOUT_MS : REBOOT_TIMEOUT_MS
	reconnectionTimeoutId = setTimeout(() => {
		console.log('[useCore] Reconnection timeout reached')
		if (isInitialized.value && wasmModule && sendEventCallback) {
			sendEventCallback(new EventVariantDevice(new DeviceEventVariantReconnectionTimeout()))
		}
		stopReconnectionPolling()
	}, timeoutMs)
}

/**
 * Stop reconnection polling
 */
export function stopReconnectionPolling(): void {
	if (reconnectionIntervalId !== null) {
		clearInterval(reconnectionIntervalId)
		reconnectionIntervalId = null
	}
	if (reconnectionTimeoutId !== null) {
		clearTimeout(reconnectionTimeoutId)
		reconnectionTimeoutId = null
	}
}

// ============================================================================
// New IP Polling - LocalStorage Persistence
// ============================================================================

const NETWORK_CHANGE_STORAGE_KEY = 'omnect-network-change-state'

interface StoredNetworkChangeState {
	targetIp: string
	deadline: number // Unix timestamp in milliseconds
	rollbackTimeoutSeconds: number
}

/**
 * Save network change state to localStorage
 */
function saveNetworkChangeState(targetIp: string, rollbackTimeoutSeconds: number): void {
	const deadline = Date.now() + rollbackTimeoutSeconds * 1000
	const state: StoredNetworkChangeState = {
		targetIp,
		deadline,
		rollbackTimeoutSeconds,
	}
	try {
		localStorage.setItem(NETWORK_CHANGE_STORAGE_KEY, JSON.stringify(state))
		console.log('[useCore] Saved network change state to localStorage:', state)
	} catch (e) {
		console.error('[useCore] Failed to save network change state to localStorage:', e)
	}
}

/**
 * Load network change state from localStorage
 */
function loadNetworkChangeState(): StoredNetworkChangeState | null {
	try {
		const stored = localStorage.getItem(NETWORK_CHANGE_STORAGE_KEY)
		if (!stored) return null

		const state: StoredNetworkChangeState = JSON.parse(stored)
		console.log('[useCore] Loaded network change state from localStorage:', state)
		return state
	} catch (e) {
		console.error('[useCore] Failed to load network change state from localStorage:', e)
		return null
	}
}

/**
 * Clear network change state from localStorage
 */
function clearNetworkChangeState(): void {
	try {
		localStorage.removeItem(NETWORK_CHANGE_STORAGE_KEY)
		console.log('[useCore] Cleared network change state from localStorage')
	} catch (e) {
		console.error('[useCore] Failed to clear network change state from localStorage:', e)
	}
}

/**
 * Check for pending network change on app initialization
 * Resumes polling if deadline hasn't passed, otherwise checks rollback status
 */
export function checkPendingNetworkChange(): void {
	const stored = loadNetworkChangeState()
	if (!stored) return

	const now = Date.now()
	const timeRemaining = stored.deadline - now

	if (timeRemaining > 0) {
		// Deadline hasn't passed yet - resume polling
		console.log(`[useCore] Resuming network change polling (${Math.round(timeRemaining / 1000)}s remaining)`)
		// Note: The actual resumption will happen via the watcher when Core state is restored
	} else {
		// Deadline has passed - check rollback status and clean up
		console.log('[useCore] Network change deadline has passed, checking rollback status')
		clearNetworkChangeState()
	}
}

// ============================================================================
// New IP Polling
// ============================================================================

/**
 * Start new IP polling after network config change
 * Sends NewIpCheckTick every 5 seconds and sets a timeout based on the backend's rollback timeout
 */
export function startNewIpPolling(): void {
	stopNewIpPolling() // Clear any existing timers

	console.log('[useCore] Starting new IP polling')

	// Get timeout from viewModel (provided by backend)
	const state = viewModel.network_change_state
	if (!state || (state.type !== 'waiting_for_new_ip' && state.type !== 'waiting_for_old_ip')) {
		console.warn('[useCore] startNewIpPolling called but state is not waiting_for_new_ip or waiting_for_old_ip:', state)
		return
	}

	let rollbackTimeout = 0
	let targetIp = ''
	let switchingToDhcp = false

	if (state.type === 'waiting_for_new_ip') {
		// Type casting for properties that exist on specific variants
		const s = state as any
		rollbackTimeout = s.rollback_timeout_seconds
		targetIp = s.new_ip
		switchingToDhcp = s.switching_to_dhcp
	} else {
		// waiting_for_old_ip
		const s = state as any
		// No rollback timeout in this state (we are already rolled back)
		rollbackTimeout = 0
		targetIp = s.old_ip
		// We are polling the old IP, so we know it
		switchingToDhcp = false
	}

	const timeoutMs = rollbackTimeout * 1000 // Convert seconds to milliseconds

	// Save to localStorage for page refresh resilience
	// For waiting_for_old_ip, we might not need to save timeout, or save 0
	saveNetworkChangeState(targetIp, rollbackTimeout)

	// Set countdown deadline
	countdownDeadline = Date.now() + timeoutMs

	// Start polling interval (every 5 seconds) ONLY if we are not switching to DHCP
	// If switching to DHCP, we don't know the IP so polling is useless
	if (!switchingToDhcp) {
		newIpIntervalId = setInterval(() => {
			if (isInitialized.value && wasmModule && sendEventCallback) {
				sendEventCallback(new EventVariantDevice(new DeviceEventVariantNewIpCheckTick()))
			}
		}, NEW_IP_POLL_INTERVAL_MS)
	} else {
		console.log('[useCore] Skipping polling because switching to DHCP (IP unknown)')
	}

	// Only start countdown and timeout if rollback is enabled (timeout > 0)
	if (rollbackTimeout > 0) {
		// Start countdown interval (every 1 second for UI countdown)
		// Calculate remaining seconds from deadline instead of decrementing
		newIpCountdownIntervalId = setInterval(() => {
			if (countdownDeadline !== null) {
				const remainingMs = Math.max(0, countdownDeadline - Date.now())
				const remainingSeconds = Math.ceil(remainingMs / 1000)
				viewModel.overlay_spinner.countdown_seconds = remainingSeconds
			}
		}, 1000)

		// Set timeout
		newIpTimeoutId = setTimeout(() => {
			console.log('[useCore] New IP polling timeout reached')
			if (isInitialized.value && wasmModule && sendEventCallback) {
				sendEventCallback(new EventVariantDevice(new DeviceEventVariantNewIpCheckTimeout()))
			}
			stopNewIpPolling()
		}, timeoutMs)
	}
}

/**
 * Stop new IP polling
 */
export function stopNewIpPolling(): void {
	if (newIpIntervalId !== null) {
		clearInterval(newIpIntervalId)
		newIpIntervalId = null
	}
	if (newIpCountdownIntervalId !== null) {
		clearInterval(newIpCountdownIntervalId)
		newIpCountdownIntervalId = null
	}
	if (newIpTimeoutId !== null) {
		clearTimeout(newIpTimeoutId)
		newIpTimeoutId = null
	}
	// Clear countdown seconds in viewModel
	viewModel.overlay_spinner.countdown_seconds = undefined
	// Clear countdown deadline
	countdownDeadline = null
}

// ============================================================================
// State Watchers
// ============================================================================

/**
 * Initialize watchers for automatic timer management
 * Call this once during module initialization
 */
export function initializeTimerWatchers(): void {
	// Watch device_operation_state for reconnection polling
	watch(
		() => viewModel.device_operation_state,
		(newState, oldState) => {
			const newType = newState?.type
			const oldType = oldState?.type

			// Start polling when entering rebooting, factory_resetting, or updating state
			if (newType === 'rebooting' || newType === 'factory_resetting' || newType === 'updating') {
				startReconnectionPolling(newType === 'factory_resetting')
			}
			// Stop polling when leaving these states or entering terminal states
			else if (
				(oldType === 'rebooting' || oldType === 'factory_resetting' || oldType === 'updating' || oldType === 'waiting_reconnection') &&
				(newType === 'idle' || newType === 'reconnection_successful' || newType === 'reconnection_failed')
			) {
				stopReconnectionPolling()
			}
		},
		{ deep: true }
	)

	// Watch network_change_state for new IP polling and redirect
	watch(
		() => viewModel.network_change_state,
		(newState, oldState) => {
			const newType = newState?.type
			const oldType = oldState?.type

			// Start polling when entering polling states
			const isPollingState = (type: string | undefined) =>
				type === 'waiting_for_new_ip' || type === 'waiting_for_old_ip'

			if (isPollingState(newType) && !isPollingState(oldType)) {
				startNewIpPolling()
			}
			// Stop polling when leaving polling states
			else if (isPollingState(oldType) && !isPollingState(newType)) {
				stopNewIpPolling()
			}
			// If switching between polling states (e.g. new_ip -> old_ip), restart to update config/target
			else if (isPollingState(newType) && isPollingState(oldType) && newType !== oldType) {
				startNewIpPolling()
			}

			// Clear localStorage when entering terminal states (success, timeout, or idle)
			if (newType === 'new_ip_reachable' || newType === 'new_ip_timeout' || newType === 'waiting_for_old_ip' || newType === 'idle') {
				clearNetworkChangeState()
			}

			// Navigate to new IP when it's reachable
			if (newState?.type === 'new_ip_reachable') {
				console.log(`[useCore] Redirecting to new IP: ${newState.new_ip}:${newState.ui_port}`)
				// Use HTTPS (server only listens on HTTPS)
				window.location.href = `https://${newState.new_ip}:${newState.ui_port}`
			}
		},
		{ deep: true }
	)
}
