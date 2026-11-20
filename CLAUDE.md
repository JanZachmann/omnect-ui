# omnect-ui

Local web UI for omnect devices. It gives access to device features
(status, reboot, factory reset, firmware update, network and Wi-Fi
settings), also when the device has no cloud connection. The backend talks
to omnect-device-service and wifi-commissioning-service on the device.

## Structure

- `src/app/`: Crux Core (crate `omnect-ui-core`). All business logic and
  state: `lib.rs` (`App`, `Effect`), `model.rs`, `events.rs`, handlers in
  `update/`, domain types in `types/`. Compiled to WASM for the UI.
- `src/ui/`: Vue 3 + Vuetify shell (Vite, bun). The WASM bridge and effect
  handlers (HTTP, WebSocket, time) are in `src/composables/core/`; pages in
  `src/pages/`; Playwright tests in `tests/`.
- `src/backend/`: Actix web service (crate `omnect-ui`). Route table in
  `main.rs`, handlers in `api.rs`, logic in `services/`, clients for
  omnect-device-service, wifi-commissioning-service and Keycloak.
- `src/shared_types/`: TypeGen. Its `build.rs` generates the TypeScript
  types for the Core types into `generated/` (git-ignored).

## Constraints

- Business logic goes into the Core, not into Vue. The shell sends an
  `Event` to the WASM function `process_event`, runs the effects that come
  back and returns each result with `handle_response`. It reads the view
  model with `view()`. All data on this bridge is bincode.
- `Model` is also the `ViewModel`, so the shell sees all of its fields.
- `src/backend/src/build.rs` embeds `src/ui/dist` into the binary (served at
  `/static`) and fails if it is missing. Run `scripts/build-frontend.sh`
  before any cargo build, test or clippy of the backend, and before
  `bun run tsc` and the E2E tests; they need the generated types and WASM.
- `shared_types` is not in the default workspace members, so a plain
  `cargo build` does not update the TypeScript types. After a change to a
  type that crosses the bridge, run `scripts/build-frontend.sh`. A new event
  or state enum must be registered in `src/shared_types/build.rs`, or
  TypeGen misses some of its variants.
- The `mock` feature of the backend crate generates `mockall` mocks of the
  service clients for unit tests and lets the config fall back to local
  defaults instead of failing on missing environment variables. It also
  skips the module certificate and uses a temp data directory, so it is
  for tests only.

## Commands

- Frontend: `scripts/build-frontend.sh` builds the WASM module
  (wasm-pack), generates the TypeScript types and builds the UI with bun.
- Test: `cargo test --features mock` (workspace root; the feature applies
  to the backend crate).
- Lint: `cargo clippy --all-targets --features mock`; in `src/ui`:
  `bun run lint` (Biome) and `bun run tsc`.
- Format: `cargo +nightly fmt` (`rustfmt.toml` uses an unstable option).
- E2E: `scripts/run-e2e-tests.sh` starts a mock WebSocket server and a Vite
  preview server and runs Playwright. Arguments go to `playwright test`,
  e.g. `-g '<test name>'` or `<file>.spec.ts`.
  `scripts/test-e2e-in-container.sh` runs the same in a Docker container.
- Image: `scripts/build-and-deploy-image.sh` builds the Docker image
  (`--arch`, default arm64). `--deploy --host <ip> [--password <pwd>]`
  copies it to a device over SSH and restarts iotedge. The script header
  says what the device deployment needs; see also `--help`.
- Local container: `scripts/build-and-run-image.sh` builds for the host
  architecture and runs the image. It expects `/tmp/api.sock` from a local
  omnect-device-service and `temp/cert.pem`, `temp/key.pem`.

## UX guidelines

- Main pages wrap their content in
  `<v-sheet :border="true" rounded class="ma-4">`. Login and password pages
  use a centered sheet with `max-width="448"`.
- Headings:
  - section: `text-headline-large text-secondary border-b pb-2 mb-4`
  - sub-section:
    `text-headline-small text-secondary font-weight-bold border-b pb-2 mb-4`
  - group label: `text-title-small text-medium-emphasis mb-1`
- Read-only data uses `<dt>`/`<dd>` pairs or the `KeyValuePair` component,
  not form fields. A form field that becomes read-only (e.g. DHCP-managed
  network fields) switches from `variant="outlined"` to `variant="plain"`.
- Icons only for actions and status (copy, show password, navigation,
  buttons), not as decoration on labels or data.
- Buttons:
  - primary action: `variant="flat" color="primary"`
  - confirm an action that cannot be undone (reset, forget):
    `variant="flat" color="error"`
  - discard unsaved changes: `variant="text" color="error"`
  - cancel: `variant="text" color="primary"`
- Confirmation dialogs: `v-card` with
  `v-card-title class="text-headline-small"`, `v-card-text` and
  `v-card-actions` with `<v-spacer>` before the buttons (cancel first).
  Notification dialogs use the `DialogContent` component.
- Show each piece of information only once. Keep the most informative
  view and remove the other.
