# Development & Deployment Guide

This guide provides instructions for setting up the DeezChatz API development environment from scratch, understanding the project structure, and deploying it to production.

---

## 🛠 From Zero to Running (Local Setup)

Follow these steps to get the API running locally, even if you're starting with a fresh machine.

### 1. Prerequisites

1. **Rust**: Install the Rust toolchain (version 1.75+).
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
2. **Docker & Docker Compose**: Required for running Redis and RMQTT locally.
   - [Install Docker Desktop](https://www.docker.com/products/docker-desktop/) (Mac/Windows) or Docker Engine (Linux).
3. **AWS Credentials**: The API requires access to DynamoDB. You can use an AWS account or DynamoDB Local. If using AWS, configure your credentials:
   ```bash
   aws configure
   ```

### 2. Clone & Configure

```bash
git clone https://github.com/deez-in/deezchatz-api.git
cd deezchatz-api
cp .env.example .env
```

Edit `.env` to configure your settings:
- `AWS_REGION`: e.g., `ap-south-1`
- `REDIS_URL`: `redis://localhost:6379`
- `PRIMARY_TABLE`: e.g., `deezchatz-identity`
- `GOOGLE_CLIENT_ID`: Your Google OAuth Web Client ID

### 3. Start Supporting Services

Start Redis and the RMQTT broker in the background:

```bash
docker-compose up -d
```

### 4. Run the API

```bash
cargo run
```

You should see output indicating both the Public API (port 3000) and Private API (port 3001) are listening.

---

## 📁 Project Structure

Understanding the `src/` directory layout is key to navigating the codebase:

```text
src/
├── main.rs                 # Entrypoint. Initializes tracing, state, and spawns the two Axum routers (ports 3000 & 3001).
├── state.rs                # Defines AppState (DynamoDB client, Redis pool, reqwest client, push provider).
├── error.rs                # AppError enum mapping domain errors to HTTP status codes.
├── crypto.rs               # Wrappers around libsignal-dezire for VXEdDSA verification.
│
├── auth/
│   ├── google_oauth.rs     # Google OAuth 2.0 PKCE exchange and ID token verification.
│   └── signature.rs        # Stateless signature auth middleware (Axum FromRequestParts extractor).
│
├── handlers/
│   ├── public/             # Handlers exposed on port 3000 (Client-facing)
│   │   ├── bundle.rs       # POST /bundle/{id}, GET /bundle/sync/{id}
│   │   ├── device.rs       # POST /register/device/fcm
│   │   ├── register.rs     # POST /register/device, POST /register/google/id_token, POST /register/google/pkce
│   │   └── user.rs         # DELETE /users/me, DELETE /users/me/oauth, POST /users/report
│   │
│   └── private/            # Handlers exposed on port 3001 (Internal only)
│       └── offline_message.rs # POST /offline_message (from RMQTT)
│
├── db/                     # Modular DynamoDB and Redis operations
│   ├── keys.rs             # Helper functions for generating partition/sort keys.
│   ├── lib.rs              # Shared DynamoDB conversion helpers (parse_item, etc.).
│   ├── registration.rs     # Pending registration caching and session orchestration.
│   ├── user.rs             # User profile queries, deletion, reports.
│   ├── device.rs           # Device registration and FCM token updates.
│   ├── message.rs          # Offline message persistence.
│   └── temp.rs             # Redis operations (set_temp_json_nx, get_temp_json).
│
├── models/                 # Data transfer and persistence models
│   ├── api/                # Public HTTP request/response payloads (auth, bundle, device, user, webhook)
│   └── db/                 # DynamoDB items and Redis cached entities (profile, device, temp_registration)
│
└── push/                   # Push notification abstractions
    ├── mod.rs              # PushProvider trait.
    ├── fcm.rs              # Firebase Cloud Messaging implementation.
    └── apns.rs             # (Future) Apple Push Notification service.
```

---

## 🧪 Testing

The project uses Rust's built-in testing framework.

### Running Tests

```bash
# Run all tests
cargo test

# Run tests with console output (helpful for debugging)
cargo test -- --nocapture
```

### What gets tested?

- **Unit tests**: Located in the same file as the code (e.g., `src/crypto.rs`). These test isolated logic like key decoding and verification functions.
- **Integration tests**: Located in the `tests/` directory or within handlers. These often require a running Redis instance and valid AWS credentials.

> **Note on Crypto Tests**: The `src/crypto.rs` module contains critical VXEdDSA verification logic (`verify_signed_signature` and `verify_signature`). Ensure any changes to these functions are accompanied by rigorous unit tests, specifically testing failure modes (VRF mismatch, invalid signature, truncated keys).

---

## 🚀 Deployment

The DeezChatz API is packaged as an OCI-compliant container image, distributed via the GitHub Container Registry. Images are built for both `linux/amd64` and `linux/arm64`.

### 1. Pulling the Image

```bash
docker pull ghcr.io/deez-in/deezchatz-api:latest
```

### 2. Running the Container

Ensure your `.env` file is configured with the necessary production credentials.

```bash
docker run -d \
  --name deezchatz-api \
  -p 3000:3000 \
  -p 3001:3001 \
  --env-file .env \
  ghcr.io/deez-in/deezchatz-api:latest
```

### Production Considerations (CRITICAL)

- **Network Isolation**: Port 3001 (Private API) **MUST NOT** be exposed to the internet. It has no authentication. It should only be reachable by the RMQTT broker within your VPC or docker network. Use firewall rules or AWS Security Groups to restrict access.
- **AWS IAM Permissions**: The production task execution role requires the following DynamoDB permissions on the primary table:
  - `dynamodb:PutItem`
  - `dynamodb:GetItem`
  - `dynamodb:UpdateItem`
  - `dynamodb:DeleteItem`
  - `dynamodb:TransactWriteItems`
- **Secrets Management**: Do not bake `.env` files into production images. Use a secret manager (like AWS Secrets Manager or ECS environment variable injection) for `GOOGLE_CLIENT_SECRET` and `REDIS_URL`.
- **RMQTT Configuration**: The API relies on RMQTT webhooks to trigger offline push notifications. Ensure the RMQTT production configuration mirrors `devenv/rmqtt/plugins/rmqtt-web-hook.toml` and points to the API's port 3001.
