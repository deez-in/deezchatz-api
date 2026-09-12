# Concepts Guide 💬

> **Start here.** This document explains the core ideas behind DeezChatz before you dive into the technical details. It covers the security model, identity system, and how registration and messaging work end-to-end.

For the detailed sequence diagrams and verification flowcharts, see [Cryptographic Flows](cryptographic_flows.md).
For the database schema and infrastructure layout, see [Architecture](architecture.md).

---

## The DeezChatz Philosophy: Zero-Trust

DeezChatz operates on a simple principle: **the server is a mailbox, not a reader.** Got privacy? **DeezChatz**. 🔒

The server stores and forwards encrypted messages, but it cannot:
- Read message content (it's encrypted client-side before it ever touches the network).
- Generate or access private keys (clients generate all keys locally).
- Forge signatures (authentication uses cryptographic signatures, not passwords or tokens the server issues).

If the server were fully compromised, an attacker would see:
- Public keys (which are, by definition, public).
- Encrypted blobs (indistinguishable from random noise without the private keys).
- Metadata (who is messaging whom, and when — this is a known limitation of the current design).

---

## Identity Model

DeezChatz has three types of entities:

### Individuals

A human user, registered via Google OAuth. This is the primary identity — every Individual has:
- A `userId` (UUID, generated at registration).
- An `email` (from Google, used for lookup).
- An `Identity Key` (long-term Curve25519 public key, generated on the client).

### Devices

A physical or virtual client owned by an Individual. Currently, each Individual has a single Device (multi-device is on the roadmap). A Device has:
- A `deviceId` (UUID).
- A `Signed Device Key` (signed by the Individual's Identity Key).
- An `FCM token` (for push notifications).

### Agents (Future)

AI bots or programmatic scripts that operate with their own cryptographic identities. They would be owned by an Individual but act autonomously.

---

## Key Hierarchy

Each user's cryptographic identity is built from a chain of keys, where each key is signed by the one above it:

```
Identity Key (iKey)                    ← Long-term, generated once
  ├── Signed Pre-Key (signedPreKey)    ← Medium-term, used for X3DH + API auth
  │     └── One-Time Pre-Keys (OPKs)  ← Ephemeral, consumed during first contact
  └── Signed Device Key                ← Per-device, signed by Identity Key
```

| Key | Size | Lifetime | Purpose |
|-----|------|----------|---------|
| **Identity Key** | 33 bytes | Permanent (until re-registration) | The root of trust — signs all other keys |
| **Signed Pre-Key** | 33 bytes | Medium-term | Used in X3DH key exchange; also used as the signing key for API authentication |
| **One-Time Pre-Keys** | 33 bytes each | Single-use | Consumed during X3DH — each one is used exactly once, then deleted |
| **Signed Device Key** | 33 bytes | Per-device | Identifies a specific device; signed by the Identity Key |

All keys are Curve25519 (compressed, with a `0x05` prefix byte). All signatures use **VXEdDSA**, which produces both a signature (96 bytes) and a VRF output (32 bytes).

For the detailed verification flowcharts, see [Key Hierarchy in Cryptographic Flows](cryptographic_flows.md#1-key-hierarchy).

---

## Registration Flow

Registration is a two-phase handshake. Phase 1 verifies the user's identity via Google. Phase 2 uploads their cryptographic keys.

### Phase 1: "Who are you?" (OAuth Verification)

Alice installs the DeezChatz app and taps "Sign In with Google."

1. The app triggers the native Google Sign-In flow (via `expo-google-native-oauth`).
2. Google returns an `idToken` — a JWT containing Alice's email, name, and profile picture.
3. The app sends this `idToken` and Alice's generated `iKey` to the DeezChatz API: `POST /register/google/id_token`.
4. The server verifies the token against Google's JWKS (public key set).
5. The server checks DynamoDB for an existing profile with Alice's email:
   - **Existing user?** → Reuse the existing `userId`.
   - **New user?** → Generate a new `userId` (UUID).
6. The server generates a random `state` session token (UUID v4) and stores Alice's pending claims and `iKey` in Redis with a 10-minute TTL, keyed by `reg:pending:{state}`.
7. The server returns `{ status: "success", userId, state, email, name, picture }` to the app.

### Phase 2: "Prove you can encrypt." (Key Upload)

Now the app generates the rest of the cryptographic keys locally (using `expo-libsignal-dezire`):

1. Generate a Signed Pre-Key pair, a Signed Device Key pair, and a batch of One-Time Pre-Keys.
2. VXEdDSA sign the random `state` token with the Identity Key → produces `stateSignature` + `stateVrf`.
3. VXEdDSA sign the Signed Pre-Key with the Identity Key → produces `preKeySign` + `preKeyVrf`.
4. VXEdDSA sign the Signed Device Key with the Identity Key → produces `devKeySign` + `devKeyVrf`.
5. Send everything to the API: `POST /register/device`.
6. The server:
   - Fetches Alice's pending registration from Redis using the `state` token.
   - **Verifies all three signatures + VRF outputs** (state token, pre-key, and device key — if any fail, registration is rejected).
   - Writes the Profile, Device, and lookup pointer items to DynamoDB in a single transaction.
   - Deletes the pending registration from Redis.
7. Returns `{ status: "success", userId, deviceId }`.

Alice is now registered and can send/receive encrypted messages.

For the detailed sequence diagrams, see [Registration in Cryptographic Flows](cryptographic_flows.md#2-registration-triple-key-verification).

---

## Messaging Flow

### First Message (X3DH Session Establishment)

Alice wants to message Bob for the first time. They don't share a secret yet, and Bob might be offline.

1. **Alice fetches Bob's pre-key bundle** from the server: `POST /bundle/{bob_id}`.
   - The server returns Bob's Identity Key, Signed Pre-Key, signature, and one One-Time Pre-Key (which is atomically removed from Bob's OPK list).
2. **Alice performs X3DH locally** using `expo-libsignal-dezire`:
   - Performs four Diffie-Hellman computations between her keys and Bob's bundle.
   - Derives a shared secret (`SK`).
   - Initializes a Double Ratchet session.
3. **Alice encrypts her message** using the Double Ratchet and publishes it via MQTT to:
   ```
   /deezchatz/{bob_id}/{bob_device_id}/{alice_id}/{alice_device_id}
   ```
4. **Bob receives the message** (if online) or reconnects and receives it (if offline).
5. **Bob performs the matching X3DH** to derive the same shared secret and initializes his Double Ratchet session.
6. From this point, all messages between Alice and Bob use the Double Ratchet — a new key for every message.

### Ongoing Messages (Double Ratchet)

After the initial X3DH, every subsequent message:
- Advances the symmetric ratchet (hash chain) — new message key per message.
- Periodically advances the DH ratchet (new key exchange) — provides forward secrecy and break-in recovery.
- Is encrypted with AES-256-CBC + HMAC-SHA256 and has its header encrypted with AES-256-GCM.

### Offline Delivery

When Bob is offline:
1. The RMQTT broker stores the message.
2. RMQTT fires an `offline_message` webhook to the DeezChatz API's **Private API** (port 3001).
3. The API parses the recipient ID from the MQTT topic, looks up Bob's FCM token in DynamoDB, and sends a **data-only push notification** (no message content is included).
4. Bob's device wakes up, reconnects via MQTT, and retrieves the stored message from the broker.

---

## Authentication: Why Not JWTs?

DeezChatz uses **stateless signature-based authentication** instead of JWTs. Here's why:

| | Signature Auth (DeezChatz) | JWT Auth (traditional) |
|---|---|---|
| **Who issues the credential?** | The client signs with its own key | The server issues a token |
| **Server state needed?** | None — each request is independently verifiable | Token blacklists, refresh token rotation |
| **Tied to crypto identity?** | Yes — the same keys used for encryption verify the request | No — the JWT and encryption keys are separate systems |
| **Replay protection** | Timestamp-based (±10s drift) + Redis signature cache for critical endpoints | Token expiration + refresh flow |

### How It Works

For every authenticated request, the client:

1. Constructs a payload: `userId + timestamp` (seconds UTC).
2. VXEdDSA signs the payload with its `signedPreKey` → produces a `signature` (96 bytes) + `vrf` (32 bytes).
3. Sends four headers: `X-User-Id`, `X-Timestamp`, `X-Signature`, `X-Vrf`.

The server:

1. Checks the timestamp is within ±10 seconds of server time.
2. Fetches the user's `signedPreKey` from DynamoDB.
3. Verifies the VXEdDSA signature against the payload.
4. Confirms the VRF output matches the `X-Vrf` header.
5. For critical endpoints (like FCM token updates), additionally checks Redis to prevent signature replay.

No sessions, no cookies, no tokens. Every request is a self-contained proof of identity.
