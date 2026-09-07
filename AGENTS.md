# AGENTS.md — DeezChatz API

This document provides instructions and context for AI coding agents working in this repository.

## Ecosystem Context

> **This is the DeezChatz backend.** It manages identities, serves key bundles, and handles offline message notifications. It operates on a Zero-Trust model — it never sees plaintext messages and never generates cryptographic keys.

```
deezchatz-mobile  →  REST (port 3000)  →  ⭐ DeezChatz API (this server)
RMQTT Broker      →  Webhook (port 3001) →  ⭐ DeezChatz API
```

| Relationship | Details |
|-------------|---------|
| **Depends on** | `libsignal-dezire` via cargo git dependency |
| **Used by** | `deezchatz-mobile` (via REST API) |
| **Integrates with** | Google OAuth (for JWT verification) and Firebase Cloud Messaging (for push notifications) |

### Cross-Repo Impact Rules

- **If you change `crypto.rs` verification logic**: Ensure it stays in sync with how `libsignal-dezire` generates VXEdDSA signatures and VRFs.
- **If you change the JSON payload schemas (`src/models/payload.rs`)**: You MUST notify that `deezchatz-mobile`'s API client needs to be updated.
- **If you change the MQTT topic schema**: You MUST update `deezchatz-mobile`'s subscription logic and the RMQTT webhook configuration.

---

## Commands

```bash
# Fast check
cargo check

# Build & Run (Starts on ports 3000 and 3001)
cargo run

# Testing
cargo test

# Code quality
cargo clippy -- -D warnings
cargo fmt

# Start supporting services (Redis, RMQTT)
docker-compose up -d
```

---

## Project Structure

Map of the `src/` directory:

- `main.rs`: Entrypoint. Initializes tracing, state, and spawns the two Axum routers.
- `state.rs`: Defines `AppState` (DynamoDB client, Redis connection manager, reqwest client, push provider). Passed to handlers via Axum's `State` extractor.
- `error.rs`: Defines `AppError`, mapping domain errors to Axum HTTP status codes.
- `crypto.rs`: Wrappers around `libsignal-dezire` for VXEdDSA verification.
- `auth/signature.rs`: Stateless signature auth middleware (implements Axum's `FromRequestParts`).
- `handlers/public/`: Handlers for port 3000 (registration, key bundles, FCM updates).
- `handlers/private/`: Handlers for port 3001 (offline message webhooks from RMQTT).
- `db/`: DynamoDB and Redis operations (primary CRUD, temporary state).
- `models/`: Structs representing DynamoDB items (`Profile`, `Device`) and JSON payloads.
- `push/`: Push notification traits and implementations (`FCM`).

---

## Architecture Rules

1. **Dual-Port Design**:
   - Port `3000` (Public API): Exposed to the internet. Uses signature auth.
   - Port `3001` (Private API): **Internal network only**. Has NO client auth. Used for webhooks from the MQTT broker. Never expose this port.
2. **Error Handling**: All handler errors must return `AppError` from `error.rs`. Use the `?` operator to propagate errors naturally. Do not use `unwrap()` in handler logic.
3. **State Sharing**: Share dependencies (DB clients, HTTP clients) via `State<AppState>`. Do not instantiate new clients per request.

---

## Cryptography Rules

1. **Verification Only**: The server ONLY verifies signatures; it never generates keys. Do not add key generation logic to the server.
2. **VXEdDSA wrappers**: `crypto.rs` has two functions:
   - `verify_signed_signature`: Used for registration (dual-key verification).
   - `verify_signature`: Used for per-request auth (verifies signature and returns VRF).
3. **No Plaintext**: The server must never log or inspect the `payload` field from the MQTT webhook (it is opaque ciphertext).

---

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `AWS_REGION` | DynamoDB region |
| `REDIS_URL` | Redis connection string |
| `PRIMARY_TABLE` | DynamoDB table name |
| `GOOGLE_CLIENT_ID_ANDROID` | OAuth Android Client ID (verifies `aud` claim in JWTs) |
| `GOOGLE_CLIENT_ID_WEB` | OAuth Web Client ID (verifies `aud` claim in JWTs and used for PKCE code exchange) |
| `GOOGLE_CLIENT_SECRET` | OAuth client secret |
| `GOOGLE_REDIRECT_URI` | OAuth redirect URI |
| `PUBLIC_API_PORT` | Port for client traffic (default: `3000`) |
| `PRIVATE_API_PORT` | Port for internal webhooks (default: `3001`) |

---

## Code Style

- Follow standard Rust conventions.
- Use `tracing` for logging (`info!`, `error!`, `debug!`), not `println!`.
- Keep handlers thin: extract DB logic to `src/db/` and crypto logic to `src/crypto.rs`.

---

## Testing

- Run `cargo test`.
- Note: Integration tests interacting with DynamoDB or Redis may require `docker-compose up -d` to be running and valid AWS credentials to be configured in your environment.
