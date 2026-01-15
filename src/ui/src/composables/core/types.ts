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

// Import types and variant classes for conversions
import {
	DeviceOperationState,
	DeviceOperationStateVariantidle,
	DeviceOperationStateVariantrebooting,
	DeviceOperationStateVariantfactory_resetting,
	DeviceOperationStateVariantupdating,
	DeviceOperationStateVariantwaiting_reconnection,
	DeviceOperationStateVariantreconnection_failed,
	DeviceOperationStateVariantreconnection_successful,
	NetworkChangeState,
	NetworkChangeStateVariantidle,
	NetworkChangeStateVariantapplying_config,
	NetworkChangeStateVariantwaiting_for_new_ip,
	NetworkChangeStateVariantnew_ip_reachable,
	NetworkChangeStateVariantnew_ip_timeout,
	NetworkChangeStateVariantwaiting_for_old_ip,
	NetworkFormState,
	NetworkFormStateVariantidle,
	NetworkFormStateVariantediting,
	NetworkFormStateVariantsubmitting,
	FactoryResetStatus,
	FactoryResetStatusVariantunknown,
	FactoryResetStatusVariantmode_supported,
	FactoryResetStatusVariantmode_unsupported,
	FactoryResetStatusVariantbackup_restore_error,
	FactoryResetStatusVariantconfiguration_error,
	UploadState,
	UploadStateVariantIdle,
	UploadStateVariantUploading,
	UploadStateVariantCompleted,
	UploadStateVariantFailed,
} from '../../../../shared_types/generated/typescript/types/shared_types'

// Re-export variant classes for external use
export {
	DeviceOperationState,
	NetworkChangeState,
	NetworkFormState,
	FactoryResetStatus,
	UploadState,
}

// ============================================================================
// TypeScript Discriminated Union Types
// ============================================================================

export type DeviceOperationStateType =
	| { type: 'idle' }
	| { type: 'rebooting' }
	| { type: 'factory_resetting' }
	| { type: 'updating' }
	| { type: 'waiting_reconnection'; operation: string; attempt: number }
	| { type: 'reconnection_failed'; operation: string; reason: string }
	| { type: 'reconnection_successful'; operation: string }

export type NetworkChangeStateType =
	| { type: 'idle' }
	| { type: 'applying_config'; is_server_addr: boolean; ip_changed: boolean; new_ip: string; old_ip: string; switching_to_dhcp: boolean }
	| { type: 'waiting_for_new_ip'; new_ip: string; old_ip: string; attempt: number; ui_port: number; rollback_timeout_seconds: number; switching_to_dhcp: boolean }
	| { type: 'new_ip_reachable'; new_ip: string; ui_port: number }
	| { type: 'new_ip_timeout'; new_ip: string; old_ip: string; ui_port: number; switching_to_dhcp: boolean }
	| { type: 'waiting_for_old_ip'; old_ip: string; ui_port: number; attempt: number }

export type NetworkFormStateType =
	| { type: 'idle' }
	| { type: 'editing'; adapter_name: string; form_data: NetworkFormDataType }
	| { type: 'submitting'; adapter_name: string; form_data: NetworkFormDataType }

export type UploadStateType =
	| { type: 'idle' }
	| { type: 'uploading' }
	| { type: 'completed' }
	| { type: 'failed'; content: string }

export interface NetworkFormDataType {
	name: string
	ip_address: string
	dhcp: boolean
	prefix_len: number
	dns: string[]
	gateways: string[]
}

export interface OverlaySpinnerStateType {
	overlay: boolean
	title: string
	text: string | null
	timed_out: boolean
	progress: number | null
	countdown_seconds: number | null
}

export type FactoryResetStatusString = 'unknown' | 'mode_supported' | 'mode_unsupported' | 'backup_restore_error' | 'configuration_error'

// ============================================================================
// ViewModel Interface
// ============================================================================

export interface UpdateManifest {
	update_id: { provider: string; name: string; version: string }
	compatibility: Array<{ device_manufacturer: string; device_model: string }>
}

export interface ViewModel {
	system_info: {
		os: { name: string; version: string }
		azure_sdk_version: string
		omnect_device_service_version: string
		boot_time: string | null
	} | null
	network_status: {
		network_status: Array<{
			ipv4: {
				addrs: Array<{ addr: string; dhcp: boolean; prefix_len: number }>
				dns: string[]
				gateways: string[]
			}
			mac: string
			name: string
			online: boolean
		}>
	} | null
	online_status: { iothub: boolean } | null
	factory_reset: {
		keys: string[]
		result: {
			status: FactoryResetStatusString
			context: string | null
			error: string
			paths: string[]
		} | null
	} | null
	update_validation_status: { status: string } | null
	update_manifest: UpdateManifest | null
	timeouts: { wait_online_timeout: { nanos: number; secs: bigint } } | null
	healthcheck: {
		version_info: { version: string; git_sha: string }
		update_validation_status: { status: string }
		network_rollback_occurred: boolean
	} | null
	is_authenticated: boolean
	requires_password_set: boolean
	is_loading: boolean
	error_message: string | null
	success_message: string | null
	is_connected: boolean
	auth_token: string | null

	// Device operation state (reboot/factory reset reconnection)
	device_operation_state: DeviceOperationStateType
	reconnection_attempt: number
	reconnection_timeout_seconds: number

	// Network change state (IP change detection and polling)
	network_change_state: NetworkChangeStateType

	// Network form state (editing without WebSocket interference)
	network_form_state: NetworkFormStateType

	// Network form dirty flag (tracks unsaved changes)
	network_form_dirty: boolean

	// Firmware upload state
	firmware_upload_state: UploadStateType

	// Overlay spinner state
	overlay_spinner: OverlaySpinnerStateType
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
	if (status instanceof FactoryResetStatusVariantmode_supported) return 'mode_supported'
	if (status instanceof FactoryResetStatusVariantmode_unsupported) return 'mode_unsupported'
	if (status instanceof FactoryResetStatusVariantbackup_restore_error) return 'backup_restore_error'
	if (status instanceof FactoryResetStatusVariantconfiguration_error) return 'configuration_error'
	return 'unknown'
}

/**
 * Convert string to FactoryResetStatus variant class
 */
export function stringToFactoryResetStatus(status: string): FactoryResetStatus {
	switch (status) {
		case 'mode_supported':
			return new FactoryResetStatusVariantmode_supported()
		case 'mode_unsupported':
			return new FactoryResetStatusVariantmode_unsupported()
		case 'backup_restore_error':
			return new FactoryResetStatusVariantbackup_restore_error()
		case 'configuration_error':
			return new FactoryResetStatusVariantconfiguration_error()
		default:
			return new FactoryResetStatusVariantunknown()
	}
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
	if (state instanceof DeviceOperationStateVariantfactory_resetting) {
		return { type: 'factory_resetting' }
	}
	if (state instanceof DeviceOperationStateVariantupdating) {
		return { type: 'updating' }
	}
	if (state instanceof DeviceOperationStateVariantwaiting_reconnection) {
		return { type: 'waiting_reconnection', operation: state.operation, attempt: state.attempt }
	}
	if (state instanceof DeviceOperationStateVariantreconnection_failed) {
		return { type: 'reconnection_failed', operation: state.operation, reason: state.reason }
	}
	if (state instanceof DeviceOperationStateVariantreconnection_successful) {
		return { type: 'reconnection_successful', operation: state.operation }
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
	if (state instanceof NetworkChangeStateVariantapplying_config) {
		return {
			type: 'applying_config',
			is_server_addr: state.is_server_addr,
			ip_changed: state.ip_changed,
			new_ip: state.new_ip,
			old_ip: state.old_ip,
			switching_to_dhcp: state.switching_to_dhcp,
		}
	}
	if (state instanceof NetworkChangeStateVariantwaiting_for_new_ip) {
		return {
			type: 'waiting_for_new_ip',
			new_ip: state.new_ip,
			old_ip: state.old_ip,
			attempt: state.attempt,
			ui_port: state.ui_port,
			rollback_timeout_seconds: Number(state.rollback_timeout_seconds),
			switching_to_dhcp: state.switching_to_dhcp,
		}
	}
	if (state instanceof NetworkChangeStateVariantnew_ip_reachable) {
		return { type: 'new_ip_reachable', new_ip: state.new_ip, ui_port: state.ui_port }
	}
	if (state instanceof NetworkChangeStateVariantnew_ip_timeout) {
		return {
			type: 'new_ip_timeout',
			new_ip: state.new_ip,
			old_ip: state.old_ip,
			ui_port: state.ui_port,
			switching_to_dhcp: state.switching_to_dhcp,
		}
	}
	if (state instanceof NetworkChangeStateVariantwaiting_for_old_ip) {
		return {
			type: 'waiting_for_old_ip',
			old_ip: state.old_ip,
			ui_port: state.ui_port,
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
			adapter_name: state.adapter_name,
			form_data: {
				name: state.form_data.name,
				ip_address: state.form_data.ip_address,
				dhcp: state.form_data.dhcp,
				prefix_len: state.form_data.prefix_len,
				dns: [...state.form_data.dns],
				gateways: [...state.form_data.gateways],
			},
		}
	}
	if (state instanceof NetworkFormStateVariantsubmitting) {
		return {
			type: 'submitting',
			adapter_name: state.adapter_name,
			form_data: {
				name: state.form_data.name,
				ip_address: state.form_data.ip_address,
				dhcp: state.form_data.dhcp,
				prefix_len: state.form_data.prefix_len,
				dns: [...state.form_data.dns],
				gateways: [...state.form_data.gateways],
			},
		}
	}
	return { type: 'idle' }
}

/**
 * Convert UploadState variant to typed object
 */
export function convertUploadState(state: UploadState): UploadStateType {
	if (state instanceof UploadStateVariantIdle) {
		return { type: 'idle' }
	}
	if (state instanceof UploadStateVariantUploading) {
		return { type: 'uploading' }
	}
	if (state instanceof UploadStateVariantCompleted) {
		return { type: 'completed' }
	}
	if (state instanceof UploadStateVariantFailed) {
		return { type: 'failed', content: state.value }
	}
	return { type: 'idle' }
}
