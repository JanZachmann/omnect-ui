# omnect UI

Product page: <www.omnect.io>

This module implements a web frontend and backend to provide omnect specific features in a local environment, where the device might not be connected to the azure cloud. In that case the device cannot be remotely controlled by [omnect-portal](https://cp.omnect.conplement.cloud/) and omnect UI might be the alternative.

## Features

omnect UI provides the following main features:

- **Device Status Monitoring**: Real-time visualization of device health, system information, and connectivity status.
- **Network Management**: Configuration of network adapters with support for DHCP and Static IP.
- **Automatic Network Rollback**: Safety mechanism that reverts failed network configurations to prevent device isolation.
- **Firmware Updates**: Local trigger and monitoring of atomic A/B partition updates.
- **Device Control**: Administrative actions like rebooting or factory resetting the device.
- **Secure Access**: Integrated user authentication to protect device settings.

## Architecture

omnect UI follows a full-stack Single Page Application (SPA) architecture:

- **Backend**: Rust-based web service (Actix-web) providing API endpoints and WebSocket support
- **Crux Core**: Platform-agnostic business logic compiled to WebAssembly
- **Frontend**: Vue 3 TypeScript SPA serving as the shell for the Crux Core
- **Shared Types**: TypeScript bindings auto-generated from Rust types

## Security

### Connections

All external communication uses encrypted transports. Internal service communication uses Unix domain sockets (not network-exposed).

| Connection | Direction | Protocol | Port | Security | Purpose |
|---|---|---|---|---|---|
| Browser ↔ Backend | bidirectional | HTTPS / WSS | 1977 (configurable) | TLS via rustls, self-signed certificate | UI serving and real-time WebSocket updates |
| Backend → omnect-device-service | outbound | Unix domain socket | — | OS filesystem permissions | Device control: reboot, factory reset, network config, firmware update |
| Backend → WiFi commissioning service | outbound | Unix domain socket | — | OS filesystem permissions | WiFi scan, connect, disconnect, forget |
| Backend → Keycloak (optional) | outbound | HTTPS | configured via `KEYCLOAK_URL` | TLS + RS256 JWT signature verification | Portal SSO token validation |
| Internal publish endpoint | localhost only | HTTP | 8000 (configurable) | `X-API-Key` header (UUID, generated per instance) | Receives event publications from omnect-device-service |

### Transport Security

- The backend serves HTTPS using **rustls** (memory-safe TLS library). TLS 1.2+ is enforced; no client certificate is required.
- The WebSocket endpoint (`/ws`) runs over the same TLS connection as the UI (WSS).
- The TLS certificate is issued by `aziot-edged` and written to `/cert/cert.pem` / `/cert/key.pem`. The certificate CN is set to the device's first detected IPv4 address and is regenerated when the address changes. See [HTTPS & Certificate Validation](#https--certificate-validation) for browser trust requirements.
- The internal publish endpoint (port 8000) is **plain HTTP** and must not be exposed outside the device network namespace. It is protected by a per-instance UUID API key passed as `X-API-Key`.

### Authentication

#### Local Password Mode (default)

1. On first access the UI shows a **Set Password** page — no prior credential exists.
2. The user sets a password (no username). The password is hashed with **Argon2id** and stored atomically at `/data/config/password`.
3. On subsequent logins the password is validated against the stored hash and a session cookie is issued.
4. API clients may alternatively pass `Authorization: Bearer <token>` or HTTP Basic Auth (`:<password>`).

#### Portal SSO Mode (optional)

When configured, Keycloak-based OIDC authentication gates the initial **Set Password** step:

1. The frontend initiates an OIDC flow and obtains a token from Keycloak (RS256-signed JWT).
2. The token is submitted to `POST /token/validate`. The backend fetches Keycloak's realm public key and verifies the RS256 signature.
3. If valid, the backend records a `portal_validated` flag in the session, allowing the user to call `POST /set-password`.
4. From that point on the local password flow applies.

### Session & Token Properties

**Session cookie:**

| Attribute | Value | Purpose |
|---|---|---|
| Name | `omnect-ui-session` | |
| `HttpOnly` | true | Prevents JavaScript access (XSS mitigation) |
| `Secure` | true | Transmitted over HTTPS only |
| `SameSite` | Strict | CSRF mitigation |
| Encryption | AES-GCM (private cookie) | Cookie value is authenticated and encrypted |
| Lifetime | Browser session | Destroyed when the browser is closed |

**Session JWT (stored inside the cookie):**

| Property | Value |
|---|---|
| Algorithm | HS256 |
| Expiry | 2 hours |
| Clock-skew leeway | 15 minutes |
| Signing key | 64-byte key from `OsRng`, stored at `/data/session.key` |

### Authorization

Authorization applies in Portal SSO mode when validating Keycloak tokens:

| Role | Access |
|---|---|
| `FleetAdministrator` | Full access — no fleet restriction |
| `FleetOperator` | Access only if the device's `fleet_id` is in the user's `fleet_list` claim |
| Other / missing | Access denied |

Additionally, the user's `tenant_list` claim must contain the configured tenant (default: `cp`).

### Credential and Secret Storage

| Secret | Path | Protection |
|---|---|---|
| Password hash | `/data/config/password` | Argon2id; written via atomic rename + post-write verification |
| Session signing key | `/data/session.key` | 64 bytes from `OsRng`; generated once and persisted for restart continuity |

Both paths are under `/data`, which is the persistent, writable partition on omnect Secure OS.

### Public and Protected API Endpoints

**No authentication required:**

- `GET /` — serves the SPA
- `GET /config.js` — Keycloak/SSO configuration for the frontend
- `GET /version` — backend version string
- `GET /healthcheck` — health probe (CORS enabled for external monitors)
- `GET /require-set-password` — indicates whether a password has been set
- `POST /set-password` — set initial password (requires portal session if SSO is configured)
- `POST /token/validate` — validate a Keycloak OIDC token
- `GET /api/settings` — read timeout settings

**Authentication required (session cookie, Bearer token, or Basic Auth):**

- `GET /ws` — WebSocket
- `POST /token/login`, `GET /token/refresh` — issue / refresh session token
- `POST /update/file`, `/update/load`, `/update/run` — firmware update
- `POST /factory-reset`, `POST /reboot` — device control
- `POST /network` — network configuration
- `POST /ack-rollback`, `/ack-factory-reset-result`, `/ack-update-validation` — acknowledge operations
- `POST /republish` — republish device state
- `POST /api/settings` — write timeout settings
- `POST /update-password` — change the current password
- `/wifi/*` — all WiFi management endpoints

### Security Considerations

- **Self-signed certificate**: The device certificate is not issued by a public CA. Browsers will show a security warning unless the device root certificate is imported into the browser's trust store. See [HTTPS & Certificate Validation](#https--certificate-validation).
- **Internal HTTP endpoint**: Port 8000 is plain HTTP and relies solely on the API key for protection. It must not be reachable from outside the device's loopback or container network.
- **Session key persistence**: `/data/session.key` is intentionally kept across restarts so that existing session cookies remain valid after a service restart. Delete this file to invalidate all active sessions (the service will generate a new key on the next start).
- **Password recovery**: There is no password-reset mechanism without authentication. If the password is lost, a factory reset is required.

---

## Install omnect UI

Since omnect secure OS is designed as generic OS, all specific or optional applications must be provided as docker images via azure iotedge deployment:

- deployment of omnect UI docker image via omnect-portal to a device in field
- device must be online (at least once) in order to receive the deployment and to set initial password
- after a factory reset omnect UI must be deployed again what requires a connection to azure cloud

## Access omnect UI

omnect UI can be reached at <https://DeviceIp:1977>

### HTTPS & Certificate Validation

omnect-ui uses HTTPS to secure communication. However, due to technical constraints, full certificate validation is only possible for the **first IPv4 address of the first online network interface** detected by systemd-networkd.

To access the UI without security warnings, you must **import the device's root certificate into your browser's trust store**.

**Why can't all IPs and hostnames be automatically secured?**

1.  **Certificate Generation Limitations**: The certificates are generated by the Azure IoT Edge security daemon (`aziot-edged`). This service only supports defining a Common Name (CN) but **does not support Subject Alternative Names (SANs)**. This means a single certificate cannot be valid for multiple IP addresses or hostnames simultaneously.

2.  **SNI (Server Name Indication) Limitations**: While SNI allows a server to present different certificates based on the hostname requested by the client, it relies on the client sending the hostname during the TLS handshake.
    *   **Direct IP Access**: Browsers do not send SNI when accessing a site via IP address (e.g., `https://192.168.1.100`).
    *   **Docker Bridge Mode**: In bridge mode, the container binds to `0.0.0.0` and cannot easily distinguish which host interface the traffic arrived on to serve a specific certificate.

3.  **Network Mode Constraints**:
    *   **Host Network Mode**: While using `network_mode: host` would allow binding to specific interfaces, it exposes all container ports to the host network interface directly. This would require opening the device firewall to allow traffic, which is security restrictive.
    *   **Bridge Mode (Required)**: We use Docker bridge mode to maintain isolation and control over exposed ports, but this necessitates the certificate limitations described above.

## Feature Details

### Device Status Monitoring

The device overview page displays a real-time snapshot of the device's health and identity. All data is pushed via WebSocket (healthcheck channel) so the UI updates automatically without polling.

**Information displayed:**

- **Cloud Connectivity**: Whether the device is connected to the omnect Azure IoT Hub
- **Hostname**: Device hostname
- **OS Variant & Version**: omnect Secure OS build name and version string
- **Boot Time**: Timestamp of the last system boot
- **Wait-Online Timeout**: Configured network wait timeout (seconds)
- **Service Versions**: omnect-device-service, Azure SDK, and WiFi commissioning service versions (when the installed WiFi service is too old, the entry shows the required version next to it)

---

### Firmware Updates

omnect UI supports uploading and applying firmware update packages directly from the browser.

#### Upload & Inspect

1. Navigate to the **Update** section
2. Drag-and-drop or click to select a `.tar` update archive
3. Upload progress is shown as a percentage
4. Once uploaded, the manifest is parsed and displayed in three columns:
   - **Version Info**: Current device version, update version, OS variant
   - **Provider Info**: Update provider name, creation date and time
   - **Compatibility**: Manufacturer, model, compatibility ID

#### Applying an Update

1. Optionally enable **"Enforce cloud connection"** — requires the device to successfully reconnect to Azure IoT Hub after the update before declaring success
2. Click **Install Update**
3. The UI polls for update completion (default timeout: 900 s, configurable in Settings)
4. On success the device reports the new version; on failure it reports a rollback

---

### Device Control

The device overview page provides two administrative operations, each guarded by a confirmation dialog.

#### Reboot

1. Click **Restart** and confirm the dialog
2. The reboot command is sent to omnect-device-service
3. An overlay with a countdown timer appears while the UI polls for the device to come back online (5 s interval, default timeout: 300 s)
4. Once the device responds the overlay clears and the page resumes normally

#### Factory Reset

1. Click **Factory Reset** and confirm the dialog
2. Optionally select items to **preserve** (e.g., network configuration, certificates) — available options are reported by the device
3. The reset command is sent; an overlay with a countdown timer appears
4. The UI polls for reconnection (5 s interval, default timeout: 600 s)
5. Once the device responds the overlay clears

---

### Secure Access

omnect UI protects device settings with authentication.

**Password login (default):**

- Enter the password on the login page — no username is required
- On first use, a guided **Set Password** page is shown before the login form is accessible
- Passwords are hashed and stored on the device; the session is managed via JWT

**Portal SSO (optional):**

- If the device is configured for OIDC-based portal authentication, the UI detects this automatically and redirects to the identity provider
- When the portal session expires the UI clears the stale token and re-initiates the login flow

---

### Settings

The **Settings** page exposes configurable timeout durations (in seconds) for long-running operations. All values are persisted on the device and applied on the next operation.

| Setting | Default | Description |
|---|---|---|
| Network rollback | 90 s | How long to wait at the new address before reverting a failed network change |
| Reboot reconnect | 300 s | Maximum time to wait for the device to come back online after a reboot |
| Factory reset reconnect | 600 s | Maximum time to wait for the device to come back online after a factory reset |
| Firmware update | 900 s | Maximum time to wait for an update to complete |

All timeouts accept values between 30 s and 3600 s. A **Reset to Defaults** button restores all four values at once.

---

### WiFi Management

omnect UI can discover and manage WiFi networks when the WiFi commissioning service is installed on the device and meets the minimum required version.

#### Scanning & Connecting

1. Navigate to the **Network** section and open the **WiFi** panel
2. Click **Scan** to discover nearby networks; each result shows the SSID and signal strength (1–4 bars based on RSSI)
3. Click a network to open the **Connect** dialog, enter the password, and press **Connect**
4. Connection status (Idle / Connecting / Connected / Failed) is shown in real time

#### Managing Saved Networks

- Previously connected networks appear in the **Saved Networks** list with a **CURRENT** badge on the active connection
- Click the delete icon next to a saved network to forget its credentials
- Use the **Disconnect** button to drop the current WiFi connection without forgetting it

---

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
./scripts/build-and-deploy-image.sh

# Build for different architecture
./scripts/build-and-deploy-image.sh --arch amd64

# Build with custom tag
./scripts/build-and-deploy-image.sh --tag v1.2.0

# Build and push to registry
./scripts/build-and-deploy-image.sh --push

# Build and deploy to device
./scripts/build-and-deploy-image.sh --deploy

# Full example with all options
./scripts/build-and-deploy-image.sh --arch arm64 --tag my-feature --push --deploy --host 192.168.1.100 --port 1977
```

Run `./scripts/build-and-deploy-image.sh --help` for all available options.

### Testing

```bash
# Run all unit tests (backend + core)
cargo test --features mock

# Run E2E tests (automated setup - starts Mock WebSocket server + frontend dev server)
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

- `check_ods` task: Verifies omnect-device-service is running

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
