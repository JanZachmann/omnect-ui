# Project Context

This repository implements a web frontend and backend to provide omnect specific features in a local environment, where the device might not be connected to the azure cloud.

## Rules

- Before committing any changes that affect the project structure, the "Project Structure" section in this file must be updated.

## 1. Architecture & Tech Stack

The project uses a Cargo workspace structure and implements the Crux framework's Core/Shell architecture for the frontend.

### Crux Architecture

The application follows the Crux pattern where:

- **Core** (`src/app/`): Contains all business logic, state management, and type definitions. Compiled to WASM for the web shell.
- **Shell** (`src/ui/`): Vue 3 UI that renders the Core's view model and processes effects (HTTP, WebSocket).
- **Shared Types** (`src/shared_types/`): Generates TypeScript bindings from Rust types using TypeGen.

#### Core/Shell Communication

1. Shell sends Events to Core (serialized via bincode)
2. Core processes events, updates Model, returns Commands
3. Shell processes Commands (render UI, make HTTP requests, manage WebSocket)
4. Shell reads ViewModel from Core for rendering

### Backend (`src/backend/`)
- **Role:** Web Service / API providing access to omnect device features
- **Frameworks:** Rust, Actix-web
- Contains frontend as static ressource

### Frontend Core (`src/app/`)
- **Role:** Business Logic, State Management (Platform-agnostic)
- **Frameworks:** Rust, Crux (compiled to WASM)

### Frontend Shell (`src/ui/`)
- **Role:** User Interface (Single Page Application)
- **Frameworks:** Vue 3, TypeScript, Vite

### Shared Types (`src/shared_types/`)
- **Role:** Type definitions shared between Backend/Core and Frontend
- **Frameworks:** TypeGen (generates TypeScript bindings)

## Build Commands

### Build Frontend (Crux Core WASM, Frontend (Vue 3), TypeScript Types)

```bash
# build all frontend artefacts
# This script performs:
# 1. Builds WASM module with wasm-pack
# 2. Generates TypeScript types from Rust (cargo build -p shared_types)
# 3. Removes .js files to force Vite to use .ts sources
# 4. Installs dependencies and builds UI with bun
scripts/build-frontend.sh
```

### Build and Deploy omnect-ui Docker Image

```bash
# Build and Deploy ARM64 image
./scripts/build-and-deploy-image.sh --arch arm64 --deploy --host <device-ip> --password <ssh-pwd>
```

### Build and run Image on Host

```bash
# Build and Deploy ARM64 image
./scripts/build-and-run-image.sh
```

## Test Commands

### Unit Tests (Rust)

```bash
# Run tests
cargo test --features mock
```

### Formatting (Rust)

```bash
# Requires nightly toolchain (rustfmt.toml uses unstable options)
cargo +nightly fmt
```

### End-to-End (e2e tests)

```bash
# Run all e2e tests
./scripts/run-e2e-tests.sh

# Run a single e2e test
 ./scripts/run-e2e-tests.sh -g 'my-test'

# Run all e2e tests located in a file
 ./scripts/run-e2e-tests.sh my-tests.spec.ts
```

## UX Guidelines

### Page Layout

- Every page wraps content in `<v-sheet :border="true" rounded class="ma-4">`.
- No page-level padding beyond `ma-4` on the sheet; inner spacing is handled per component.

### Typography Hierarchy

| Level | Class | Usage |
| ----- | ----- | ----- |
| Page section header | `text-h4 text-secondary border-b` | Top-level section titles ("Common Info", "Network") |
| Sub-section header | `text-h5 text-secondary font-weight-bold border-b pb-2` | Secondary sections ("Commands", "Update Details") |
| Category label | `text-subtitle-2 text-medium-emphasis mb-1` | Column/group labels ("Connectivity", "Version", "Provider") |

### Data Display

- Read-only data uses plain `<dt>`/`<dd>` pairs or the `KeyValuePair` component — no form fields, no icons.
- Form fields that become read-only (e.g., DHCP-managed network fields) switch to `variant="plain"` to visually distinguish them from editable fields (`variant="outlined"`).

### Icons

- Do not add decorative icons to form field labels or data displays. Labels are self-explanatory.
- Icons are reserved for **functional actions** (copy-to-clipboard, status indicators, navigation).

### Buttons

| Role | Variant | Color |
| ---- | ------- | ----- |
| Primary action | `variant="flat"` | `color="primary"` |
| Destructive confirm | `variant="flat"` | `color="error"` |
| Secondary / Cancel | `variant="text"` | `color="primary"` |

### Dialogs

- Confirmation dialogs use `v-card` with `v-card-title` (`text-h5`), `v-card-text`, and `v-card-actions`.
- Actions are right-aligned (`<v-spacer>` before buttons).
- Cancel is always `variant="text"`, confirm uses `variant="flat"`.

### Redundancy

- Do not display the same information twice in different components. Choose the most informative presentation and remove the redundant one.

## Project Structure

```text
omnect-ui/
├── Cargo.toml                    # Workspace root
├── Dockerfile                    # Multi-stage Docker build
├── scripts/                      # Build and test scripts
│   ├── build-frontend.sh         # Build WASM + Types + UI
│   ├── build-and-deploy-image.sh # Docker build and deploy
│   ├── build-and-run-image.sh    # Local docker development
│   └── run-e2e-tests.sh          # Playwright test runner
├── src/
│   ├── app/                      # Crux Core (business logic)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs            # App struct, Effect enum, re-exports
│   │       ├── model.rs          # Model struct (application state)
│   │       ├── events.rs         # Event enum
│   │       ├── wasm.rs           # WASM FFI bindings
│   │       ├── macros.rs         # URL and log macros
│   │       ├── http_helpers.rs   # HTTP request utilities
│   │       ├── wifi_psk.rs       # WiFi PSK utilities
│   │       ├── commands/         # Custom side-effect commands
│   │       │   ├── mod.rs
│   │       │   └── websocket.rs      # WebSocket commands
│   │       ├── types/            # Domain types
│   │       │   ├── mod.rs
│   │       │   ├── auth.rs       # Authentication types
│   │       │   ├── common.rs     # Common shared types
│   │       │   ├── device.rs     # Device information types
│   │       │   ├── network.rs    # Network configuration types
│   │       │   ├── wifi.rs       # WiFi types
│   │       │   ├── ods.rs        # ODS-specific DTOs
│   │       │   ├── settings.rs   # Timeout settings types
│   │       │   ├── websocket.rs  # WebSocket channel enum
│   │       │   ├── factory_reset.rs
│   │       │   └── update.rs     # Update validation types
│   │       └── update/           # Domain-based event handlers
│   │           ├── mod.rs        # Main dispatcher
│   │           ├── auth.rs       # Auth event handlers
│   │           ├── ui.rs         # UI state handlers
│   │           ├── wifi.rs       # WiFi event handlers
│   │           ├── websocket.rs  # WebSocket state handlers
│   │           └── device/       # Device domain handlers
│   │               ├── mod.rs
│   │               ├── operations.rs # Reboot/Factory Reset logic
│   │               ├── reconnection.rs # Device reconnection polling
│   │               └── network/    # Network configuration logic
│   ├── backend/                  # Rust backend (Actix-web)
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── main.rs           # Application entry point
│   │   │   ├── lib.rs            # Library entry point
│   │   │   ├── api.rs            # API route handlers
│   │   │   ├── build.rs          # Static resource generation
│   │   │   ├── middleware.rs     # Auth middleware
│   │   │   ├── config.rs         # Configuration loading
│   │   │   ├── http_client.rs    # Internal HTTP client
│   │   │   ├── keycloak_client.rs
│   │   │   ├── omnect_device_service_client.rs
│   │   │   ├── wifi_commissioning_client.rs
│   │   │   ├── version.rs        # Service version requirement check
│   │   │   └── services/         # Business logic services
│   │   │       ├── mod.rs
│   │   │       ├── certificate.rs
│   │   │       ├── firmware.rs
│   │   │       ├── network.rs
│   │   │       ├── marker.rs
│   │   │       ├── settings.rs   # Timeout settings service
│   │   │       ├── websocket.rs  # WebSocket handlers
│   │   │       └── auth/         # Auth logic
│   │   │           ├── mod.rs
│   │   │           ├── authorization.rs # JWT/SSO validation
│   │   │           ├── password.rs      # Password hashing/storage
│   │   │           ├── session_key.rs   # Session key management
│   │   │           └── token.rs         # JWT generation
│   │   └── tests/                # Integration tests
│   ├── shared_types/             # TypeGen for TypeScript bindings
│   │   ├── Cargo.toml
│   │   ├── build.rs              # TypeGen build script
│   │   └── generated/            # Generated TypeScript types
│   └── ui/                       # Vue 3 Shell
│       ├── package.json
│       ├── playwright.config.ts  # E2E test configuration
│       ├── vite.config.ts        # Build configuration
│       ├── src/
│       │   ├── App.vue           # Root component
│       │   ├── main.ts           # UI entry point
│       │   ├── components/       # UI components
│       │   ├── composables/      # Logic & WASM bridge
│       │   │   ├── useCore.ts    # Main bridge + effect handlers
│   │       │   ├── useWebSocket.ts # Native WebSocket client
│   │       │   └── core/         # Modular Core integration
│   │       │       ├── index.ts  # Main entry point
│   │       │       ├── state.ts  # Singleton reactive state
│   │       │       ├── types.ts  # TypeScript type conversions
│   │       │       ├── effects.ts # Effect processing
│   │       │       ├── http.ts   # HTTP capability
│   │       │       ├── websocket.ts # WebSocket capability
│   │       │       ├── time.ts   # Time/timer capability (crux_time)
│   │       │       └── sync.ts   # ViewModel synchronization and navigation side-effects
│       │   ├── pages/            # Route components
│   │   │   ├── DeviceOverview.vue
│   │   │   ├── DeviceUpdate.vue
│   │   │   ├── Network.vue
│   │   │   ├── Settings.vue
│   │   │   ├── Login.vue
│   │   │   ├── SetPassword.vue
│   │   │   ├── UpdatePassword.vue
│   │   │   └── Callback.vue
│   │   ├── plugins/          # Router, Vuetify
│   │   └── types/            # UI-specific types
│   └── tests/                # Playwright E2E tests
│       ├── auth.spec.ts
│       ├── device.spec.ts
│       ├── error-handling.spec.ts
│       ├── factory-reset.spec.ts
│       ├── network-configuration.spec.ts
│       ├── network-multi-adapter.spec.ts
│       ├── reboot.spec.ts
│       ├── settings.spec.ts
│       ├── smoke.spec.ts
│       ├── update.spec.ts
│       ├── version-mismatch.spec.ts
│       ├── wifi.spec.ts
│       └── fixtures/
│           ├── mock-api.ts
│           ├── network-test-harness.ts
│           ├── test-setup.ts
│           └── websocket.ts      # WebSocket test mocks
└── project-context.md            # This file
```

### Key Files

**Frontend (Shell):**
- `src/ui/src/composables/useCore.ts` - Core WASM bridge + effect handlers
- `src/ui/src/composables/useWebSocket.ts` - WebSocket client integration
- `src/ui/src/pages/DeviceOverview.vue` - Main device dashboard page

**Core:**
- `src/app/src/lib.rs` - App struct, Effect enum, and re-exports
- `src/app/src/model.rs` - Model struct (application state)
- `src/app/src/events.rs` - Event enum definitions
- `src/app/src/types/` - Domain types organized by domain
- `src/app/src/update/` - Domain-based event handlers
- `src/app/src/commands/websocket.rs` - Custom WebSocket commands

**Backend:**
- `src/backend/src/main.rs` - Application entry point
- `src/backend/src/api.rs` - API route handlers
- `src/backend/src/services/websocket.rs` - Native WebSocket implementation

**Scripts:**
- `scripts/build-frontend.sh` - Build complete frontend (WASM + TypeScript types + UI)
- `scripts/build-and-deploy-image.sh` - Docker build and deploy script
