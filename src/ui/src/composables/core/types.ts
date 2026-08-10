/**
 * Type definitions and conversions for Crux Core integration
 *
 * This module provides:
 * - Re-exports of generated types from shared_types
 * - Type conversion helpers for DeviceOperationState, NetworkChangeState, etc.
 * - ODS JSON to Crux type mappings
 */

// Re-export generated types from shared_types for use in Vue components
export type {
	SystemInfo,
	NetworkStatus,
	OnlineStatus,
	FactoryReset,
	UpdateValidationStatus,
	Timeouts,
	HealthcheckInfo,
	Event,
	Effect,
	Model as CoreViewModel,
	UpdateManifest,
	NetworkFormData,
} from '../../../../shared_types/generated/typescript/types/shared_types'

export { NetworkConfigRequest } from '../../../../shared_types/generated/typescript/types/shared_types'

// Import types and variant classes for conversions
import {
	WifiState,
	WifiStateVariantunknown,
	WifiStateVariantunavailable,
	WifiStateVariantready,
	WifiConnectionState,
	WifiConnectionStateVariantidle,
	WifiConnectionStateVariantconnecting,
	WifiConnectionStateVariantconnected,
	WifiConnectionStateVariantfailed,
	WifiScanState,
	WifiScanStateVariantidle,
	WifiScanStateVariantscanning,
	WifiScanStateVariantfinished,
	WifiScanStateVarianterror,
	DeviceOperationState,
	DeviceOperationStateVariantidle,
	DeviceOperationStateVariantrebooting,
	DeviceOperationStateVariantfactoryResetting,
	DeviceOperationStateVariantupdating,
	DeviceOperationStateVariantwaitingReconnection,
	DeviceOperationStateVariantreconnectionFailed,
	DeviceOperationStateVariantreconnectionSuccessful,
	UpdateManifest,
	NetworkChangeState,
	NetworkChangeStateVariantidle,
	NetworkChangeStateVariantapplyingConfig,
	NetworkChangeStateVariantwaitingForNewIp,
	NetworkChangeStateVariantnewIpReachable,
	NetworkChangeStateVariantnewIpTimeout,
	NetworkChangeStateVariantwaitingForOldIp,
	NetworkFormState,
	NetworkFormStateVariantidle,
	NetworkFormStateVariantediting,
	NetworkFormStateVariantsubmitting,
	FactoryResetStatus,
	FactoryResetStatusVariantunknown,
	FactoryResetStatusVariantmodeSupported,
	FactoryResetStatusVariantmodeUnsupported,
	FactoryResetStatusVariantbackupRestoreError,
	FactoryResetStatusVariantconfigurationError,
	UploadState,
	UploadStateVariantidle,
	UploadStateVariantuploading,
	UploadStateVariantcompleted,
	UploadStateVariantfailed,
	DeviceNetwork,
	WebSocketChannel,
	WebSocketChannelVariantOnlineStatusV1,
	WebSocketChannelVariantSystemInfoV1,
	WebSocketChannelVariantTimeoutsV1,
	WebSocketChannelVariantNetworkStatusV1,
	WebSocketChannelVariantFactoryResetV1,
	WebSocketChannelVariantUpdateValidationStatusV1,
} from '../../../../shared_types/generated/typescript/types/shared_types'

// Re-export variant classes for external use
export {
	DeviceOperationState,
	NetworkChangeState,
	NetworkFormState,
	FactoryResetStatus,
	UploadState,
	DeviceNetwork,
	WifiState,
	WebSocketChannel,
	WebSocketChannelVariantOnlineStatusV1,
	WebSocketChannelVariantSystemInfoV1,
	WebSocketChannelVariantTimeoutsV1,
	WebSocketChannelVariantNetworkStatusV1,
	WebSocketChannelVariantFactoryResetV1,
	WebSocketChannelVariantUpdateValidationStatusV1,
}

// ============================================================================
// TypeScript Discriminated Union Types
// ============================================================================

/**
 * Convert WebSocketChannel variant to string
 */
export function webSocketChannelToString(channel: WebSocketChannel): string {
	if (channel instanceof WebSocketChannelVariantOnlineStatusV1) return 'OnlineStatusV1'
	if (channel instanceof WebSocketChannelVariantSystemInfoV1) return 'SystemInfoV1'
	if (channel instanceof WebSocketChannelVariantTimeoutsV1) return 'TimeoutsV1'
	if (channel instanceof WebSocketChannelVariantNetworkStatusV1) return 'NetworkStatusV1'
	if (channel instanceof WebSocketChannelVariantFactoryResetV1) return 'FactoryResetV1'
	if (channel instanceof WebSocketChannelVariantUpdateValidationStatusV1) return 'UpdateValidationStatusV1'
	return 'unknown'
}

export type DeviceOperationStateType =
	| { type: 'idle' }
	| { type: 'rebooting' }
	| { type: 'factoryResetting' }
	| { type: 'updating' }
	| { type: 'waitingReconnection'; operation: string; attempt: number }
	| { type: 'reconnectionFailed'; operation: string; reason: string }
	| { type: 'reconnectionSuccessful'; operation: string }

export type NetworkChangeStateType =
	| { type: 'idle' }
	| { type: 'applyingConfig'; isServerAddr: boolean; ipChanged: boolean; newIp: string; oldIp: string; switchingToDhcp: boolean }
	| { type: 'waitingForNewIp'; newIp: string; oldIp: string; attempt: number; uiPort: number; rollbackTimeoutSeconds: number; switchingToDhcp: boolean }
	| { type: 'newIpReachable'; newIp: string; uiPort: number }
	| { type: 'newIpTimeout'; newIp: string; oldIp: string; uiPort: number; switchingToDhcp: boolean }
	| { type: 'waitingForOldIp'; oldIp: string; uiPort: number; attempt: number }

export type NetworkFormStateType =
	| { type: 'idle' }
	| { type: 'editing'; adapterName: string; formData: NetworkFormDataType; errors: Record<string, string> }
	| { type: 'submitting'; adapterName: string; formData: NetworkFormDataType; errors: Record<string, string> }

export type UploadStateType =
	| { type: 'idle' }
	| { type: 'uploading' }
	| { type: 'completed' }
	| { type: 'failed'; content: string }

export interface NetworkFormDataType {
	name: string
	ipAddress: string
	dhcp: boolean
	subnetMask: string
	dns: string[]
	gateways: string[]
}

export interface OverlaySpinnerStateType {
	overlay: boolean
	title: string
	text: string | null
	timedOut: boolean
	progress: number | null
	countdownSeconds: number | null
}

export interface TimeoutSettingsType {
	rebootTimeoutSecs: number
	factoryResetTimeoutSecs: number
	firmwareUpdateTimeoutSecs: number
	networkRollbackTimeoutSecs: number
}

export type WifiScanStateType =
	| { type: 'idle' }
	| { type: 'scanning' }
	| { type: 'finished' }
	| { type: 'error'; message: string }

export type WifiConnectionStateType =
	| { type: 'idle' }
	| { type: 'connecting' }
	| { type: 'connected' }
	| { type: 'failed'; message: string }

export interface WifiConnectionStatusType {
	state: WifiConnectionStateType
	ssid: string | null
	ipAddress: string | null
}

export interface WifiNetworkType {
	ssid: string
	mac: string
	channel: number
	rssi: number
}

export interface WifiSavedNetworkType {
	ssid: string
	flags: string
}

/** Shared by the omnect-device-service healthcheck and the WiFi service probe. */
export interface VersionInfoType {
	required: string
	current: string
	mismatch: boolean
}

export type WifiStateType =
	| { type: 'unknown' }
	| { type: 'unavailable', socketPresent: boolean, versionInfo: VersionInfoType | null }
	| {
		type: 'ready'
		interfaceName: string
		version: string | null
		status: WifiConnectionStatusType
		scanState: WifiScanStateType
		scanResults: WifiNetworkType[]
		savedNetworks: WifiSavedNetworkType[]
		scanPollAttempt: number
		connectPollAttempt: number
	}

export type FactoryResetStatusString = 'unknown' | 'modeSupported' | 'modeUnsupported' | 'backupRestoreError' | 'configurationError'

// ============================================================================
// ViewModel Interface
// ============================================================================

export interface ViewModel {
	systemInfo: {
		os: { name: string; version: string }
		azureSdkVersion: string
		omnectDeviceServiceVersion: string
		bootTime: string | null
		hostname: string
	} | null
	networkStatus: {
		networkStatus: DeviceNetwork[]
	} | null
	onlineStatus: { iothub: boolean } | null
	factoryReset: {
		keys: string[]
		result: {
			status: FactoryResetStatusString
			context: string | null
			error: string
			paths: string[]
		} | null
	} | null
	updateValidationStatus: { status: string } | null
	updateManifest: UpdateManifest | null
	timeouts: { waitOnlineTimeout: { nanos: number; secs: bigint } } | null
	healthcheck: {
		versionInfo: VersionInfoType
		updateValidationStatus: { status: string }
		networkRollbackOccurred: boolean
		updateValidationAcked: boolean
	} | null
	isAuthenticated: boolean
	requiresPasswordSet: boolean
	isLoading: boolean
	errorMessage: string | null
	successMessage: string | null
	isConnected: boolean
	authToken: string | null

	// Device operation state (reboot/factory reset reconnection)
	deviceOperationState: DeviceOperationStateType
	reconnectionAttempt: number

	// Network change state (IP change detection and polling)
	networkChangeState: NetworkChangeStateType

	// Network form state
	networkFormState: NetworkFormStateType

	// Network form dirty flag (tracks unsaved changes)
	networkFormDirty: boolean

	// Browser hostname
	browserHostname: string | null

	// Current connection adapter name
	currentConnectionAdapter: string | null

	// Device offline tracking
	deviceWentOffline: boolean

	// Network rollback modal state
	shouldShowRollbackModal: boolean
	defaultRollbackEnabled: boolean

	// Version mismatch state (derived from healthcheck)
	versionMismatch: boolean
	versionMismatchMessage: string | null

	// Firmware upload state
	firmwareUploadState: UploadStateType

	// Overlay spinner state
	overlaySpinner: OverlaySpinnerStateType

	// WiFi state
	wifiState: WifiStateType

	// User-configurable timeout settings
	timeoutSettings: TimeoutSettingsType
}

// ============================================================================
// Type Conversion Functions
// ============================================================================

/**
 * Convert FactoryResetStatus class variant to string literal
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function factoryResetStatusToString(status: any): FactoryResetStatusString {
	if (status instanceof FactoryResetStatusVariantunknown) return 'unknown'
	if (status instanceof FactoryResetStatusVariantmodeSupported) return 'modeSupported'
	if (status instanceof FactoryResetStatusVariantmodeUnsupported) return 'modeUnsupported'
	if (status instanceof FactoryResetStatusVariantbackupRestoreError) return 'backupRestoreError'
	if (status instanceof FactoryResetStatusVariantconfigurationError) return 'configurationError'
	return 'unknown'
}

/**
 * Convert DeviceOperationState variant to typed object
 */
export function convertDeviceOperationState(state: DeviceOperationState): DeviceOperationStateType {
	if (state instanceof DeviceOperationStateVariantidle) {
		return { type: 'idle' }
	}
	if (state instanceof DeviceOperationStateVariantrebooting) {
		return { type: 'rebooting' }
	}
	if (state instanceof DeviceOperationStateVariantfactoryResetting) {
		return { type: 'factoryResetting' }
	}
	if (state instanceof DeviceOperationStateVariantupdating) {
		return { type: 'updating' }
	}
	if (state instanceof DeviceOperationStateVariantwaitingReconnection) {
		return { type: 'waitingReconnection', operation: state.operation, attempt: state.attempt }
	}
	if (state instanceof DeviceOperationStateVariantreconnectionFailed) {
		return { type: 'reconnectionFailed', operation: state.operation, reason: state.reason }
	}
	if (state instanceof DeviceOperationStateVariantreconnectionSuccessful) {
		return { type: 'reconnectionSuccessful', operation: state.operation }
	}
	return { type: 'idle' }
}

/**
 * Convert NetworkChangeState variant to typed object
 */
export function convertNetworkChangeState(state: NetworkChangeState): NetworkChangeStateType {
	if (state instanceof NetworkChangeStateVariantidle) {
		return { type: 'idle' }
	}
	if (state instanceof NetworkChangeStateVariantapplyingConfig) {
		return {
			type: 'applyingConfig',
			isServerAddr: state.is_server_addr,
			ipChanged: state.ip_changed,
			newIp: state.new_ip,
			oldIp: state.old_ip,
			switchingToDhcp: state.switching_to_dhcp,
		}
	}
	if (state instanceof NetworkChangeStateVariantwaitingForNewIp) {
		return {
			type: 'waitingForNewIp',
			newIp: state.new_ip,
			oldIp: state.old_ip,
			attempt: state.attempt,
			uiPort: state.ui_port,
			rollbackTimeoutSeconds: Number(state.rollback_timeout_seconds),
			switchingToDhcp: state.switching_to_dhcp,
		}
	}
	if (state instanceof NetworkChangeStateVariantnewIpReachable) {
		return { type: 'newIpReachable', newIp: state.new_ip, uiPort: state.ui_port }
	}
	if (state instanceof NetworkChangeStateVariantnewIpTimeout) {
		return {
			type: 'newIpTimeout',
			newIp: state.new_ip,
			oldIp: state.old_ip,
			uiPort: state.ui_port,
			switchingToDhcp: state.switching_to_dhcp,
		}
	}
	if (state instanceof NetworkChangeStateVariantwaitingForOldIp) {
		return {
			type: 'waitingForOldIp',
			oldIp: state.old_ip,
			uiPort: state.ui_port,
			attempt: state.attempt,
		}
	}
	return { type: 'idle' }
}

/**
 * Convert NetworkFormState variant to typed object
 */
export function convertNetworkFormState(state: NetworkFormState): NetworkFormStateType {
	if (state instanceof NetworkFormStateVariantidle) {
		return { type: 'idle' }
	}
	if (state instanceof NetworkFormStateVariantediting) {
		return {
			type: 'editing',
			adapterName: state.adapter_name,
			formData: {
				name: state.form_data.name,
				ipAddress: state.form_data.ipAddress,
				dhcp: state.form_data.dhcp,
				subnetMask: state.form_data.subnetMask,
				dns: [...state.form_data.dns],
				gateways: [...state.form_data.gateways],
			},
			errors: state.errors instanceof Map ? Object.fromEntries(state.errors) : state.errors,
		}
	}
	if (state instanceof NetworkFormStateVariantsubmitting) {
		return {
			type: 'submitting',
			adapterName: state.adapter_name,
			formData: {
				name: state.form_data.name,
				ipAddress: state.form_data.ipAddress,
				dhcp: state.form_data.dhcp,
				subnetMask: state.form_data.subnetMask,
				dns: [...state.form_data.dns],
				gateways: [...state.form_data.gateways],
			},
			errors: state.errors instanceof Map ? Object.fromEntries(state.errors) : state.errors,
		}
	}
	return { type: 'idle' }
}

/**
 * Convert UploadState variant to typed object
 */
export function convertUploadState(state: UploadState): UploadStateType {
	if (state instanceof UploadStateVariantidle) {
		return { type: 'idle' }
	}
	if (state instanceof UploadStateVariantuploading) {
		return { type: 'uploading' }
	}
	if (state instanceof UploadStateVariantcompleted) {
		return { type: 'completed' }
	}
	if (state instanceof UploadStateVariantfailed) {
		return { type: 'failed', content: state.value }
	}
	return { type: 'idle' }
}

/**
 * Convert WifiScanState variant to typed object
 */
export function convertWifiScanState(state: WifiScanState): WifiScanStateType {
	if (state instanceof WifiScanStateVariantidle) return { type: 'idle' }
	if (state instanceof WifiScanStateVariantscanning) return { type: 'scanning' }
	if (state instanceof WifiScanStateVariantfinished) return { type: 'finished' }
	if (state instanceof WifiScanStateVarianterror) return { type: 'error', message: state.value }
	return { type: 'idle' }
}

/**
 * Convert WifiConnectionState variant to typed object
 */
export function convertWifiConnectionState(state: WifiConnectionState): WifiConnectionStateType {
	if (state instanceof WifiConnectionStateVariantidle) return { type: 'idle' }
	if (state instanceof WifiConnectionStateVariantconnecting) return { type: 'connecting' }
	if (state instanceof WifiConnectionStateVariantconnected) return { type: 'connected' }
	if (state instanceof WifiConnectionStateVariantfailed) return { type: 'failed', message: state.value }
	return { type: 'idle' }
}

/**
 * Convert WifiState variant to typed object
 */
export function convertWifiState(state: WifiState): WifiStateType {
	if (state instanceof WifiStateVariantunknown) {
		return { type: 'unknown' }
	}
	if (state instanceof WifiStateVariantunavailable) {
		const s = state as any
		return {
			type: 'unavailable',
			socketPresent: s.socket_present,
			versionInfo: s.version_info
				? {
						required: s.version_info.required,
						current: s.version_info.current,
						mismatch: s.version_info.mismatch,
					}
				: null,
		}
	}
	if (state instanceof WifiStateVariantready) {
		const s = state as any
		return {
			type: 'ready',
			interfaceName: s.interface_name,
			version: s.version || null,
			status: {
				state: convertWifiConnectionState(s.status.state),
				ssid: s.status.ssid || null,
				ipAddress: s.status.ipAddress || null,
			},
			scanState: convertWifiScanState(s.scan_state),
			scanResults: s.scan_results.map((n: any) => ({
				ssid: n.ssid,
				mac: n.mac,
				channel: n.ch,
				rssi: n.rssi,
			})),
			savedNetworks: s.saved_networks.map((n: any) => ({
				ssid: n.ssid,
				flags: n.flags,
			})),
			scanPollAttempt: s.scan_poll_attempt,
			connectPollAttempt: s.connect_poll_attempt,
		}
	}
	return { type: 'unknown' }
}
