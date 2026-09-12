# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.8.0] - 2026-09-12

### Added
- **MQTT Client Authentication Webhook (`POST /verify-mqtt-client`)**: Implemented a private API endpoint for the RMQTT broker's HTTP auth plugin to authenticate connecting devices. Supports fixed-length (182 characters) password unpacking consisting of a VXEdDSA signature, VRF output, and a Unix epoch seconds timestamp. Validates signatures against the device's registered `signedPreKey` and enforces a $\pm$ 10s timestamp drift window.

### Changed
- **Removed Unused Offline Message Variables**: Removed redundant `senderDeviceId` references from `put_offline_message` and the offline message DynamoDB payload.

---

## [0.7.0] - 2026-09-07

### Added
- **Web OAuth PKCE Account Deletion (`DELETE /users/me/oauth`)**: Secure endpoint enabling account deletion directly from web interfaces via Google OAuth 2.0 PKCE code exchange without requiring mobile cryptographic keys or VXEdDSA signatures.
- **Multi-Client ID OAuth Configuration**: Added separate configuration support for Android (`GOOGLE_CLIENT_ID_ANDROID`) and Web (`GOOGLE_CLIENT_ID_WEB`) OAuth client IDs in `AppState` and JWT token verification.

### Changed
- **Modular OAuth Module**: Extracted and centralized Google OAuth verification, token exchange, and user resolution logic into `src/auth/oauth.rs`.
- **User Resolution Behavior**: `resolve_user_id` now strictly searches for existing user identities without auto-registering new profiles during web deletion workflows.

---

## [0.6.3] - 2026-09-03

### Fixed
- **DynamoDB Transaction Collision During Device Re-Registration**: Resolved `ValidationException: Transaction request cannot include multiple operations on one item` in `/register/device`. When an existing user re-registered, mismatched raw phone strings previously queued both a `Delete` and a `Put` for the exact same pointer item in a single `transact_write_items` call. Added a strict key inequality guard (`old_pk != new_pk`) so duplicate operations can never be sent in the same transaction.

### Changed
- **Simplified Phone Pointer Keys**: Removed custom truncation heuristics (`normalize_phone`) on the backend. `phone_lookup_pk` now stores and queries exact canonical phone keys (`PHONE#{phone}`), delegating international E.164 normalization cleanly to client applications.

---

## [0.5.5] - 2026-08-31

### Added
- **User Account Deletion (`DELETE /users/me`)**: Authenticated endpoint allowing users to permanently delete their account, cleaning up user profiles, registered devices, phone and email pointer lookups, and pending registration records.
- **User Abuse Reporting (`POST /users/report`)**: Authenticated endpoint to submit abuse/harassment reports with optional message transcripts for moderation, recorded in DynamoDB under `REPORTER#{reporterId}` and `REPORTED#{reportedId}` partitions.
- **State-Based Device Registration Flow**: Temporary registration sessions are now bound to cryptographic random state tokens stored in Redis (`reg:pending:{state_token}`) with 10-minute TTL. Device registration requires VXEdDSA signature and VRF proofs over the state token.

### Architecture & Refactoring
- **Modular Data Models & DB Layer**: Split database logic and models into dedicated domain modules (`src/db/user.rs`, `src/db/device.rs`, `src/db/message.rs`, `src/db/lib.rs`, `src/models/api/`, `src/models/db/`).
- **Separation of Concerns**: Cleanly separated public API transfer models from DynamoDB persistence entities.

### Fixed
- **IAM Roles Anywhere Compatibility**: Added `gcompat` to runtime container image dependencies to support glibc-linked credential helpers like `aws_signing_helper` on Alpine Linux.

### Documentation
- Updated `docs/api.md` with complete documentation for `/users/me` and `/users/report`.
- Updated `docs/development.md` and `docs/architecture.md` with current project layout and data models.

---

## [0.4.2] - 2026-08-28

### Performance & Container Optimization
- **Alpine Base Migration**: Migrated builder image to `rust:1.98.0-alpine3.24` and runtime image to `alpine:3.24.1`, significantly reducing container footprint.
- **Eliminated Layer Duplication**: Fixed file ownership assignment via `COPY --chown=rocky:rocky` instead of a separate `RUN chown` step.
- **Cargo Release Profile**: Added `[profile.release]` with `lto = true`, `strip = true`, `codegen-units = 1`, and `panic = "abort"` to reduce binary size and improve runtime efficiency.

---

## [0.4.1] - 2026-08-26

### Fixed
- Fixed a deserialization issue with offline message payloads.

---

## [0.4.0] - 2026-08-26

### Breaking Changes
- **Strict UUID v4 Format Validation**: The `X-User-Id` request header, user ID path parameters (`/bundle/{identifier}` when non-email/phone, `/bundle/sync/{userId}`), and device registration payloads now strictly require valid UUID v4 strings. Malformed or non-v4 UUIDs are immediately rejected with HTTP `400 Bad Request` or `401 Unauthorized`.
- **Sanitized 500 Error Responses**: Internal Redis and DynamoDB error details are no longer exposed in HTTP 500 error response JSON. Clients relying on specific internal error strings in response bodies will now receive generic error messages.

### Security
- Sanitized internal error responses from Redis and DynamoDB in `500 Internal Server Error` payloads to prevent infrastructure details and connection string leakage.
- Added strict UUID v4 input validation for `X-User-Id` request headers, bundle lookup parameters, and device registration payloads.
- Implemented dynamic Google JWKS caching that parses and honors HTTP `Cache-Control: max-age` response headers (clamped between 300s and 86400s) to support key rotation without cold-start penalties.

### Performance
- Added explicit request (10s) and connect (5s) timeouts to the shared outbound `reqwest::Client`.
- Implemented exponential back-off (50ms, 100ms, 200ms, 400ms) in the One-Time Prekey (OPK) conflict retry loop.
- Optimized `put_offline_message` by computing `SystemTime::now()` once for both millisecond timestamps and 30-day TTL seconds.
- Switched DynamoDB sort key functions (`profile_sk`, `lookup_sk`) to return `&'static str` to eliminate heap allocations per request.

### Quality & Reliability
- Replaced `lazy_static` macro with standard library `std::sync::LazyLock` for static topic regex compilation and removed `lazy_static` from dependencies.
- Eliminated `.unwrap()` calls on byte array conversions and option lookups across hot request paths.
- Replaced panicking `unimplemented!()` in `ApnsProvider` with graceful `Err(PushError::Internal)`.
- Added explicit logging and handling for FCM push provider `RateLimit` errors.
- Made unused OAuth credentials (`GOOGLE_CLIENT_SECRET`, `GOOGLE_REDIRECT_URI`) fall back to default values to avoid unnecessary startup panics.
- Added unit test suite covering Base64 key decoding, signature validation rejection, DB key normalization, and MQTT topic regex parsing.
- Pinned `redis` image in `docker-compose.yml` to `redis:7-alpine`.
- Pinned GitHub Actions workflow steps in `podman-push.yml` to full commit SHAs.

---

## [0.3.0] - 2026-08-25

### Breaking Changes
- **DynamoDB Zero-GSI Schema Migration**: Replaced Global Secondary Index lookups with direct primary key pointer items (`EMAIL#<email>` and `PHONE#<phone>`). Existing databases require pointer records to be populated for lookups to resolve.
- **Rebranding & MQTT Topic Namespace**: Rebranded the API and renamed message topics to `/deezchatz/{rec_id}/{rec_dev}/{sen_id}/{sen_dev}`. Clients publishing or subscribing to legacy `/khamosh/...` or `/nijhum/...` topics must update their topic format.

### Added
- Direct pointer-based lookup items (`EMAIL#<email>` and `PHONE#<phone>`) in DynamoDB to eliminate Global Secondary Indexes (GSIs).
- Transactional write support (`transact_write_items`) for atomic user registration and re-registration.

### Changed
- Project rebranded to **DeezChatz**.
- Consolidated device attributes directly into `Profile` records for single-device operations.
- Revamped architecture, cryptographic flow, and developer documentation (`AGENTS.md`).

---

## [0.2.0] - 2026-08-24

### Breaking Changes
- **Server-Generated Device Identifiers**: Client-supplied device IDs during initial registration are no longer accepted; `deviceId` is now assigned server-side as a UUID v4 and returned in the registration response.
- **CamelCase DTO Serialization**: Standardized payload fields to strict `camelCase` (e.g., `signedDeviceKey`, `signedPreKey`, `fcmToken`, `deviceId`).
- **Strict Timestamp Drift Window**: Reduced the allowed timestamp drift window for VXEdDSA signature authentication from 60 seconds to 10 seconds.

### Added
- User re-registration support preserving existing device IDs and creation metadata.
- Direct user lookup by email.
- Modular push notification provider architecture with FCM HTTP v1 support and APNs stub.
- DynamoDB offline message persistence with automatic 30-day TTL expiry.
- Bundle sync endpoint (`/bundle/sync/{userId}`) for lightweight profile and identity key lookups.
- Optional phone and profile picture fields in key bundle response payloads.
- Atomic Redis `SET NX` support for signature replay protection.

### Changed
- Migrated device ID generation from client to server during registration.
- Standardized API DTO naming and serialization conventions using `camelCase`.
- Tightened authentication timestamp drift threshold to 10 seconds.

---

## [0.1.0] - 2026-08-20

### Added
- Initial release of the zero-trust identity and key server.
- Stateless signature authentication using VXEdDSA and VRF verification via `libsignal-dezire`.
- Google OAuth ID token verification for user registration.
- X3DH Prekey Bundle serving (`/bundle/{identifier}`) and One-Time Prekey (OPK) popping.
- RMQTT broker webhook integration on isolated internal port 3001 for offline message routing.
- Docker / Podman containerization and GHCR release workflow.
