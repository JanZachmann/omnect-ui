<script setup lang="ts">
/**
 * DeviceInfoCore - Device information component using Crux Core state management
 *
 * This component uses the Crux architecture where:
 * - All state lives in the Core
 * - Shell handles only effects (HTTP, WebSocket)
 * - Components read from the reactive viewModel
 * - No local refs for data - all computed from Core state
 */
import { computed } from 'vue'
import { useCore } from '../../composables/useCore'
import { useCoreInitialization } from '../../composables/useCoreInitialization'
import KeyValuePair from '../ui-components/KeyValuePair.vue'

const { viewModel } = useCore()

useCoreInitialization()

// All device info computed from the Core's viewModel
const deviceInfo = computed(() => {
  const infoMap = new Map([
    ['omnect Cloud Connection', viewModel.onlineStatus?.iothub ? 'connected' : 'disconnected'],
    ['Hostname', viewModel.systemInfo?.hostname ?? 'n/a'],
    ['omnect Secure OS variant', viewModel.systemInfo?.os.name ?? 'n/a'],
    [
      'Boot time',
      viewModel.systemInfo?.bootTime
        ? new Date(viewModel.systemInfo.bootTime).toLocaleString()
        : 'n/a',
    ],
    ['omnect Secure OS version', String(viewModel.systemInfo?.os.version) ?? 'n/a'],
    ['Wait online timeout (in seconds)', viewModel.timeouts?.waitOnlineTimeout.secs ?? 'n/a'],
    [
      'omnect device service version',
      viewModel.systemInfo?.omnectDeviceServiceVersion ?? 'n/a',
    ],
    ['Azure SDK version', viewModel.systemInfo?.azureSdkVersion ?? 'n/a'],
  ])

  // Conditionally add WiFi commissioning service version
  if (viewModel.wifiState) {
    if (viewModel.wifiState.type === 'ready') {
      infoMap.set('WiFi commissioning service version', viewModel.wifiState.version || 'unknown')
    } else if (viewModel.wifiState.type === 'unavailable' && viewModel.wifiState.socketPresent) {
      // No versionInfo means the service did not answer the probe at all.
      const info = viewModel.wifiState.versionInfo
      infoMap.set(
        'WiFi commissioning service version',
        info ? `${info.current} (required: ${info.required})` : 'unknown'
      )
    }
  }

  return infoMap
})

const displayItems = computed(() =>
  Array.from(deviceInfo.value, ([title, value]) => ({ title, value }))
)
</script>

<template>
  <div class="flex flex-col gap-y-4 w-full">
    <div class="text-h4 text-secondary border-b pb-2 mb-4">Common Info</div>
    <dl class="grid grid-cols-1 md:grid-cols-2 gap-x-12 gap-y-4 w-full">
      <div v-for="item of displayItems" :key="item.title" class="grid grid-cols-[max-content_1fr] items-baseline border-b pb-1 gap-x-4">
        <dt class="text-subtitle-2 text-medium-emphasis">{{ item.title }}</dt>
        <dd class="text-body-1 font-weight-medium text-gray-900 text-right">{{ item.value }}</dd>
      </div>
    </dl>
  </div>
</template>
