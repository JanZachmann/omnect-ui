<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
	overlay: boolean
	title: string
	text?: string
	timedOut: boolean
	progress?: number
	countdownSeconds?: number
	redirectUrl?: string
}>()

const refresh = () => {
	window.location.reload()
}

const navigateToRedirectUrl = () => {
	if (props.redirectUrl) {
		// Open in new tab - allows user to accept self-signed certificate
		// Once accepted, user can close the tab with cert warning and click again to navigate in current window
		const opened = window.open(props.redirectUrl, '_blank')
		if (!opened) {
			// Fallback if popup blocked
			window.location.href = props.redirectUrl
		}
	}
}

const formattedCountdown = computed(() => {
	if (props.countdownSeconds === undefined) return null
	const minutes = Math.floor(props.countdownSeconds / 60)
	const seconds = props.countdownSeconds % 60
	return `${minutes}:${seconds.toString().padStart(2, '0')}`
})
</script>

<template>
	<v-overlay :persistent="true" :model-value="props.overlay" :no-click-animation="true"
		class="align-center justify-center">
		<div id="overlay" class="flex flex-col items-center">
			<v-sheet class="flex flex-col gap-y-8 items-center p-8" :rounded="'lg'">
				<div class="text-h4 text-center">{{ props.title }}</div>
				<v-progress-circular color="secondary" :indeterminate="props.progress === undefined"
					:model-value="props.progress" size="100" width="5">
					<template v-slot:default>
						<span v-if="props.progress !== undefined" class="text-h6">{{ props.progress }}%</span>
					</template>
				</v-progress-circular>
				<p class="text-h6 m-t-4">{{ props.text }}</p>
				<div v-if="formattedCountdown" class="flex flex-col items-center gap-2">
					<div class="text-subtitle-1 text-medium-emphasis">Automatic rollback in:</div>
					<div class="text-h5 text-primary font-mono">
						{{ formattedCountdown }}
					</div>
				</div>
				<v-btn v-if="props.redirectUrl" color="primary" @click="navigateToRedirectUrl">
					Open new address in new tab
				</v-btn>
				<v-btn v-if="props.timedOut" text="Refresh" @click="refresh" />
			</v-sheet>
		</div>
	</v-overlay>
</template>