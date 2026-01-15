# omnect UI

Product page: <www.omnect.io>

This module implements a web frontend and backend to provide omnect specific features in a local environment, where the device might not be connected to the azure cloud. In that case the device cannot be remotely controlled by [omnect-portal](https://cp.omnect.conplement.cloud/) and omnect UI might be the alternative.

## Architecture

omnect UI follows a full-stack Single Page Application (SPA) architecture:

- **Backend**: Rust-based web service (Actix-web) providing API endpoints and WebSocket support via Centrifugo
- **Crux Core**: Platform-agnostic business logic compiled to WebAssembly
- **Frontend**: Vue 3 TypeScript SPA serving as the shell for the Crux Core
- **Shared Types**: TypeScript bindings auto-generated from Rust types

## Install omnect UI

Since omnect secure OS is designed as generic OS, all specific or optional applications must be provided as docker images via azure iotedge deployment:

- deployment of omnect UI docker image via omnect-portal to a device in field
- device must be online (at least once) in order to receive the deployment and to set initial password
- after a factory reset omnect UI must be deployed again what requires a connection to azure cloud

## Access omnect UI

omnect UI can be reached at <https://DeviceIp:1977>

Login with the configured password<br>
![login](docu/login.png)<br>
Watch device status<br>
![login](docu/main.png)<br>
Reset device and choose options to keep<br>
![factory-reset](docu/factory-reset.png)<br>
Update your device<br>
![update](docu/update.png)

## Features

### Network Configuration

omnect UI allows you to configure network settings for your device's network adapters. This feature is particularly useful when you need to change IP addresses or switch between DHCP and static IP configuration.

#### Configuring Network Adapters

1. Navigate to the Network section in the UI
2. Select the network adapter you want to configure
3. Choose between DHCP or Static IP assignment
4. For static IP, configure:
   - IP address and subnet mask
   - Gateway addresses
   - DNS servers
5. Click "Save" to apply the configuration

#### Automatic Rollback Protection

When changing network settings that affect your current connection, omnect UI provides an optional automatic rollback feature to prevent losing access to your device:

**When does this apply:**

The confirmation dialog appears when you:
- Change the static IP address of the adapter you're currently connected to, OR
- Switch from static IP to DHCP on the adapter you're currently connected to

**How it works:**

1. When you attempt a change that affects your connection, a confirmation dialog appears
2. You can choose to enable automatic rollback protection (enabled by default and recommended)
3. If enabled:
   - **For static IP changes**: You have 90 seconds to access the device at the new IP address. An overlay with a countdown timer will guide you to the new address. You must log in at the new IP address to confirm the change works.
   - **For DHCP changes**: You have 90 seconds to find and access the new DHCP-assigned IP (check your DHCP server or device console). The overlay will show a countdown.
   - If you don't access the new address and log in within 90 seconds, the device automatically restores the previous network configuration. The browser will attempt to reconnect to the original address.
4. If disabled:
   - Changes are applied immediately without automatic rollback protection
   - **For static IP changes**: An overlay appears with a button to navigate to the new IP address
   - **For DHCP changes**: An overlay informs you to use your DHCP server or console to find the new IP
   - You're responsible for manually accessing the new address
   - If the new configuration doesn't work, you may lose network access to the device

**Important notes:**

- Automatic rollback only applies when changing settings on the adapter you're currently connected through
- Changes to other network adapters don't trigger the rollback mechanism
- When switching to DHCP, the new IP address cannot be known in advance - you must check your DHCP server or device console
- The rollback feature requires physical or console access to recover if network access is lost and rollback fails

## Development

### Prerequisites

- Rust toolchain (1.91+)
- Bun for frontend development
- wasm-pack for WASM builds
- Docker with buildx support
- `toml` CLI tool (for version extraction)
- Running instance of [omnect-device-service](https://github.com/omnect/omnect-device-service)

### Project Structure

```
omnect-ui/
├── src/
│   ├── backend/          # Rust backend (Actix-web)
│   ├── app/              # Crux Core (business logic)
│   ├── shared_types/     # TypeGen for TypeScript bindings
│   └── ui/               # Vue 3 frontend
├── scripts/              # Build and development scripts
│   └── build-frontend.sh # Build WASM + TypeScript types + UI
├── tools/                # Development tools
│   ├── centrifugo        # WebSocket server binary (gitignored)
│   └── setup-centrifugo.sh  # Download script for Centrifugo
├── Dockerfile            # Multi-stage Docker build
└── build-and-deploy-image.sh  # Build and deployment script
```

### Building

#### Quick Start for Local Development

```bash
# Run development setup (builds frontend once)
./scripts/build-frontend.sh

# Run backend with mock features
cargo run --bin omnect-ui --features=mock

# Or use VSCode debugger (F5) - pre-launch task is configured
```

#### Manual Build Steps

```bash
# Build frontend (WASM + TypeScript types + UI)
./scripts/build-frontend.sh

# Build backend
cargo build -p omnect-ui --release
```

#### Frontend Development Server

For hot-reload during frontend development:

```bash
cd src/ui
bun run dev  # Starts Vite dev server with HMR
```

#### Docker Image Build

Use the `build-and-deploy-image.sh` script for building and optionally deploying a Docker image to device **(there must be an existing deployment and the restart policy of omnect-ui must be set to `never`)**.

```bash
# Build ARM64 image (default)
./build-and-deploy-image.sh

# Build for different architecture
./build-and-deploy-image.sh --arch amd64

# Build with custom tag
./build-and-deploy-image.sh --tag v1.2.0

# Build and push to registry
./build-and-deploy-image.sh --push

# Build and deploy to device
./build-and-deploy-image.sh --deploy

# Full example with all options
./build-and-deploy-image.sh --arch arm64 --tag my-feature --push --deploy --host 192.168.1.100 --port 1977
```

Run `./build-and-deploy-image.sh --help` for all available options.

### Testing

```bash
# Run all unit tests (backend + core)
cargo test --features mock

# Run E2E tests (automated setup - starts Centrifugo + frontend dev server)
./scripts/run-e2e-tests.sh

# Run E2E tests in Docker container (isolated environment)
./scripts/test-e2e-in-container.sh

# Lint all code
cargo clippy --all-targets --features mock
```

#### Troubleshooting E2E Tests

If you encounter permission errors when running E2E tests (typically after running Docker-based tests), clean up files created by root:

```bash
# Clean all E2E test artifacts with permission issues
sudo rm -rf temp/certs src/ui/dist src/ui/test-results src/ui/playwright-report
```

### VSCode Integration

The project includes VSCode launch configurations optimized for development:

#### Pre-Launch Task (runs before each debug session)

- `check_ods_and_centrifugo` task: Verifies omnect-device-service is running and kills existing Centrifugo processes

**Prerequisites before launching the debugger:**

- Ensure omnect-device-service is running (`/tmp/api.sock` must exist)
- Build frontend if you made changes: `./scripts/build-frontend.sh`

## License

Licensed under either of

- Apache License, Version 2.0, (./LICENSE-APACHE or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license (./LICENSE-MIT or <http://opensource.org/licenses/MIT>)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.

---

copyright (c) 2024 conplement AG<br>
Content published under the Apache License Version 2.0 or MIT license, are marked as such. They may be used in accordance with the stated license conditions.
