<script setup lang="ts">
import axios from "axios"
import { onMounted, type Ref, ref, computed, watch } from "vue"
import { useRoute, useRouter } from "vue-router"
import { useDisplay } from "vuetify"
import BaseSideBar from "./components/BaseSideBar.vue"
import DialogContent from "./components/DialogContent.vue"
import OmnectLogo from "./components/branding/OmnectLogo.vue"
import OverlaySpinner from "./components/feedback/OverlaySpinner.vue"
import UserMenu from "./components/UserMenu.vue"
import { useCore } from "./composables/useCore"
import { useSnackbar } from "./composables/useSnackbar"
import { useMessageWatchers } from "./composables/useMessageWatchers"
import type { HealthcheckResponse } from "./types"

axios.defaults.validateStatus = (_) => true

const { snackbarState } = useSnackbar()
const { viewModel, ackRollback, subscribeToChannels, unsubscribeFromChannels } = useCore()

// Enable automatic message watchers
useMessageWatchers()

const { lgAndUp } = useDisplay()
const router = useRouter()
const route = useRoute()
const showSideBar: Ref<boolean> = ref(lgAndUp.value)
const overlay: Ref<boolean> = ref(false)
const showRollbackNotification: Ref<boolean> = ref(false)
const errorTitle = ref("")
const errorMsg = ref("")

const overlaySpinnerState = computed(() => viewModel.overlay_spinner)

// Build redirect URL from network_change_state when waiting for new IP
const redirectUrl = computed(() => {
	const state = viewModel.network_change_state

	// Show button for both waiting and timeout states (but NOT for DHCP or rollback verification)
	if ((state.type === 'waiting_for_new_ip' || state.type === 'new_ip_timeout')
		&& 'new_ip' in state
		&& 'ui_port' in state
		&& !('switching_to_dhcp' in state && state.switching_to_dhcp)) {
		return `https://${state.new_ip}:${state.ui_port}`
	}

	// Don't show for waiting_for_old_ip - rollback in progress
	return undefined
})

const toggleSideBar = () => {
	showSideBar.value = !showSideBar.value
}

const updateSidebarVisibility = (visible: boolean) => {
	showSideBar.value = visible
}

const acknowledgeRollback = () => {
	ackRollback()
	showRollbackNotification.value = false
}

// Watch authentication state to redirect to login if session is lost
// This handles the case where the backend restarts (reboot/factory reset) and the session becomes invalid
watch(
	() => viewModel.is_authenticated,
	async (isAuthenticated) => {
		if (isAuthenticated) {
			subscribeToChannels()
		} else {
			unsubscribeFromChannels()
			if (route.meta.requiresAuth) {
				await router.push("/login")
			}
		}
	},
    { immediate: true }
)

// Watch for network rollback status from healthcheck updates (e.g. after automatic rollback)
watch(
	() => viewModel.healthcheck?.network_rollback_occurred,
	(occurred) => {
		if (occurred) {
			showRollbackNotification.value = true
		}
	}
)

onMounted(async () => {
	const res = await fetch("healthcheck", {
		headers: {
			"Cache-Control": "no-cache, no-store, must-revalidate",
			Pragma: "no-cache",
			Expires: "0"
		}
	})
	const data = (await res.json()) as HealthcheckResponse
	if (data.network_rollback_occurred) {
		showRollbackNotification.value = true
	}

	if (!res.ok) {
		overlay.value = true
		errorTitle.value = "omnect-device-service version mismatch"
		errorMsg.value = `Current version: ${data.version_info.current}. Required version ${data.version_info.required}. Please consider to update omnect Secure OS.`
	}
})
</script>

<template>
  <v-app>
    <v-dialog v-model="overlay" max-width="50vw" :no-click-animation="true" persistent fullscreen>
      <DialogContent :title="errorTitle" dialog-type="Error" :show-close="false">
        <div class="flex flex-col gap-2 mb-8">
          {{ errorMsg }}
        </div>
      </DialogContent>
    </v-dialog>
    <v-dialog v-model="showRollbackNotification" max-width="500" persistent>
      <DialogContent title="Network Settings Rolled Back" dialog-type="Warning" :show-close="false">
        <div class="flex flex-col gap-4 mb-4">
          <p>
            The network settings were rolled back to the previous configuration because the new settings could not be confirmed.
          </p>
          <div class="flex justify-end">
            <v-btn color="primary" @click="acknowledgeRollback">OK</v-btn>
          </div>
        </div>
      </DialogContent>
    </v-dialog>
    <v-app-bar flat :style="{ borderBottomWidth: '1px', borderColor: '#677680' }">
      <template #prepend>
        <v-icon class="hidden-lg-and-up mr-4 cursor-pointer text-primary" @click.stop="toggleSideBar">mdi-menu</v-icon>
        <OmnectLogo class="h-12"></OmnectLogo>
      </template>
      <template v-if="route.meta.showMenu" #append>
        <div class="flex gap-x-4 mr-4 items-center">
          <UserMenu />
        </div>
      </template>
    </v-app-bar>
    <BaseSideBar v-if="route.meta.showMenu" :showSideBar="showSideBar"
      @drawerVisibiltyChanged="updateSidebarVisibility">
    </BaseSideBar>
    <v-main>
      <RouterView></RouterView>
      <v-snackbar v-model="snackbarState.snackbar" :color="snackbarState.color" :timeout="snackbarState.timeout">
        {{ snackbarState.msg }}
        <template #actions>
          <v-btn icon=" mdi-close" @click="snackbarState.snackbar = false"></v-btn>
        </template>
      </v-snackbar>
      <OverlaySpinner :overlay="overlaySpinnerState.overlay" :title="overlaySpinnerState.title"
        :text="overlaySpinnerState.text || undefined" :timed-out="overlaySpinnerState.timed_out"
        :progress="overlaySpinnerState.progress || undefined"
        :countdown-seconds="overlaySpinnerState.countdown_seconds || undefined"
        :redirect-url="redirectUrl" />
    </v-main>
  </v-app>
</template>

<style>
:root {
  --color-primary: #677680;
  --color-background: #f4f5f7;
  --color-header: #dee2e6;
  --color-primary-rgb: 103, 118, 128;
  --color-secondary: #0094b1;
  --color-grey-30: #afb1b3;
  --color-grey-10: #e1e4e6;
  --color-grey-5: #f2f2f2;
  --color-white-dimmed: #d9d9d9;
  --color-notification-success-fill: #6ca425;
  --color-notification-setup-fill: #de5c14;
  --color-notification-setup-text: #f2d3c2;
  --color-notification-update-fill: #008a96;
  --color-notification-update-text: #c2eef2;
  --color-notification-general-fill: #388bc7;
  --color-notification-common-fill: #005d86;
  --color-notification-general-text: #c2e4f2;
  --color-notification-cancelled-fill: #ffb100;
  --color-notification-pending-fill: #bd5ec0;
  --color-text-primary: #292f33;
  --color-fail: #b3101d;
  font-size: 16px;
}

body {
  color: var(--color-text-primary);
  border-color: var(--color-grey-30);
}

p,
ul,
ol,
pre {
  margin: '1em 0';
  line-height: 1.75;
}

blockquote {
  margin: '1em 0';
  padding-left: '1em';
  font-style: 'italic';
  border-left: '.25em solid var(--un-prose-borders)';
}

img,
video {
  max-width: 100%;
}

figure,
picture {
  margin: 1em 0;
}

figcaption {
  color: var(--un-prose-captions);
  font-size: .875em;
}

table {
  margin: 1em 0;
  border-collapse: collapse;
  overflow-x: auto;
}

td,
th {
  padding: .625em 1em;
}

th {
  font-weight: 600;
}

abbr {
  cursor: help;
}

kbd {
  color: var(--un-prose-code);
  border: 1px solid;
  padding: .25rem .5rem;
  font-size: .875em;
  border-radius: .25rem;
}

details {
  margin: 1em 0;
  padding: 1.25rem 1.5rem;
  background: var(--un-prose-bg-soft);
}

summary {
  cursor: pointer;
  font-weight: 600;
}

.v-data-table-header__content {
  font-weight: 600;
}

.v-table__wrapper {
  overflow: visible !important;
}

.white_30 {
  background-color: rgb(255 255 255 / 0.3)
}
</style>
