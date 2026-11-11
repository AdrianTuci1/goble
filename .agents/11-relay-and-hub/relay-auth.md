# Relay — identity, pairing and transport

**Status:** `[~]` core library landed (`crates/goble-relay`); gateway wiring pending
**Owns:** who a device is, how a new device joins, and what the relay can see
**Depends on:** [`README.md`](README.md), [`../03-workspace-model/shared-secrets-and-toml.md`](../03-workspace-model/shared-secrets-and-toml.md)

## Why the PIN goes away

The gateway's PIN is a six-digit shared secret on a 12-hour cookie. Six digits is about 20 bits: at
a modest 10 attempts per second a non-loopback listener is guessed in hours, and the gateway has no
per-device identity to throttle or revoke — one PIN opens every worker the process knows, and the
worker side presents a single shared bearer token. The frames are copied verbatim inside TLS, so the
relay operator can read them, and all of it lives in memory.

That was acceptable for a loopback development panel. It is not acceptable for "control any VM from
any laptop, phone or web". The replacement has three separate jobs, and no one secret does all three:

| Job | Mechanism |
|---|---|
| Human presence on a new browser | WebAuthn passkey (ES256), user verification required |
| Device identity at the relay | Ed25519 device key + certificate signed by the owner's cluster CA |
| Confidentiality from the relay | Per-connection X25519 ECDH → HKDF-SHA256 → ChaCha20-Poly1305 |

## Identity hierarchy

```
ClusterKey (owner seed, 32 bytes, in the EncryptedWallet)
  └── CA (Ed25519)
        ├── Device certificate  {device_id, pubkey, role, name, created_at, serial}
        ├── Device certificate  {… phone …}
        └── Worker certificate  {worker_id, pubkey, address, tags}

Passkey (WebAuthn credential, per device/browser)
  └── relay account session (short-lived token, never the transport key)
```

- A **device key** is generated on the device and never leaves its keystore: macOS Keychain / Secure
  Enclave, iOS Keychain, Android Keystore, Windows DPAPI/TPM, Linux Secret Service (with an explicit
  file fallback the user is warned about). The relay sees the public key and the certificate.
- A **device certificate** is issued by the cluster CA, exactly as worker certificates are today
  (`goble-core::identity`, `provision`). Its serial is what revocation names. It carries a role
  (`Owner`, `Admin`, `Operator`, `Viewer`) so a phone can be read-only.
- A **passkey** is bound to the relay's origin and is the human factor for browser sessions and for
  approving a new device. It is a WebAuthn credential with `residentKey: required`,
  `userVerification: required`, and `attestation: none` (we care that the authenticator verified the
  user, not which vendor made it).
- **No shared bearer token.** A worker authenticates with its own certificate; a relay node
  authorises it by the certificate chain and the revocation list, not by a token every worker shares.

## Pairing a new device

Pairing is a two-channel ceremony: a QR code carries the rendezvous, and both screens show a short
authentication string (SAS) the user compares. The QR alone never grants access.

1. **Offer.** The already-trusted device creates a one-time offer: 128-bit random `secret`, a
   fresh X25519 ephemeral key pair, an expiry of 120 seconds, and a relay-side record of
   `SHA-256(secret)` (never the secret). The QR payload is:
   `goble://pair?v=1&r=<relay-url>&o=<offer-id>&k=<ephemeral-pub-b64>&s=<secret-b64>`.
2. **Join.** The new device scans the QR (phone camera, or the desktop's camera for a phone-side QR),
   generates its own X25519 ephemeral key and its Ed25519 device key in the keystore, and connects
   to the relay with `offer_id` and `HMAC-SHA256(secret, transcript)`.
3. **Agreement.** Both sides derive
   `K = HKDF-SHA256(X25519(ephemeral_self, ephemeral_peer), salt=offer_id, info="goble-pair-v1")`.
   Each proves possession of `K` with a key-confirmation MAC over the transcript (both public keys,
   both device ids, the offer id).
4. **SAS.** Both screens show six digits derived from `SHA-256(K || transcript)`, grouped `123 456`.
   The user compares them and confirms on the trusted device.
5. **Issue.** The trusted device signs the new device certificate and sends it over the `K`-encrypted
   channel with the relay endpoints and the current revocation list. The new device stores the
   certificate and its private key in the keystore, and burns the offer.
6. **Passkey.** On a phone or browser, the new device then registers a passkey for the relay origin.
   From that point the device can open a browser session without the trusted device present.

Both directions use the same payload: the desktop shows a QR for the phone, and the phone can show a
QR for a desktop that has a camera. The trusted device may also approve a pending offer from its own
UI (the "waiting for approval" row), which is what makes pairing work when the camera is on the other
side.

### Why a SAS and not just the QR

A QR photographed from across the room, a shoulder-surfed screen, or a malicious QR sticker still
gets the attacker a valid `secret`. The SAS is the human check that the two ends derived the same
key; it is the same defence Signal's safety numbers use, reduced to six digits because the window is
120 seconds and the offer is single-use.

### Recovery

There is no PIN fallback. Losing every paired device means restoring the `EncryptedWallet` (exported
as an encrypted blob, or as a 24-word phrase at setup) on a new device, which re-creates the cluster
CA and can issue fresh certificates. A wallet restore is an owner action and is recorded in the audit
log.

## Browser sessions

1. The browser calls `POST /auth/webauthn/begin` with the device id it claims (or none, to list the
   credentials for this origin).
2. The relay answers a WebAuthn challenge; the browser signs it with the passkey.
3. The relay verifies the assertion, checks the credential belongs to the device, and issues:
   - an access token (15 minutes) in an `HttpOnly; Secure; SameSite=Strict` cookie scoped to the
     relay origin;
   - a refresh token bound to the device certificate's public key (30 days, rotating on use).
4. Sensitive actions (approve a run, open a ReadWrite grant, add a device, revoke a device) require a
   fresh assertion (`userVerification`), max age 5 minutes — a step-up, not a new login.

A browser cannot hold an Ed25519 keystore key, so for a browser the passkey *is* the device identity.
For the desktop and mobile clients, the passkey gates the session and the device key authenticates
the transport; both must be present.

## Relay transport

- **Endpoint:** `wss://<relay>/v1/worker` for workers, `wss://<relay>/v1/client` for clients.
  Both dial outbound; neither side needs an inbound port.
- **Registration:** the first frame after the TLS handshake is a `hello` carrying the certificate
  chain, a proof-of-possession signature over the handshake transcript, the protocol version, and the
  connection's capabilities. The relay verifies the chain, the revocation list, the PoP signature,
  and the device/worker role.
- **Routing:** the relay keeps `worker_id → connection` and `client_id → connection` in a durable
  registry (SQLite for one node, Postgres/Redis for a cluster). A client opens a channel with
  `attach {worker_id}`; the relay pairs the sockets and forwards frames as opaque bytes.
- **End-to-end encryption:** after `hello`, both ends run an X25519 ECDH with ephemeral keys signed
  by their certificates, derive `K = HKDF-SHA256(shared, salt=handshake_hash, info="goble-e2ee-v1")`,
  and wrap every frame in ChaCha20-Poly1305 with a 64-bit counter nonce. Rekey after 2^20 frames or
  one hour, whichever comes first. The relay forwards ciphertext; it parses only the small routing
  header (`channel_id`, `direction`, `length`).
- **No plaintext metadata beyond routing:** ids, frame sizes, and timing remain visible to the relay.
  That is documented as the trust boundary; a relay that must not learn even that is out of scope.
- **Limits:** 1 MiB per frame (terminal output is chunked), 32 MiB per connection buffer, per-device
  connection quotas, and `1013` backpressure closes rather than unbounded queueing.
- **Revocation:** a revoked serial is rejected at `hello` and every live connection for it is closed
  with `1008`. The list is signed by the cluster CA and pushed to nodes; nodes also pull it on start.

## Threat model

| Adversary | Capability | Defence |
|---|---|---|
| Network attacker | Reads/modifies TLS | TLS 1.3 + E2EE inside it |
| Relay operator | Reads frames, controls routing | E2EE; sees ids/sizes/timing only |
| Guesser | Tries PINs or offer secrets | No PIN; 128-bit offers; single use; 120 s expiry; rate limits |
| Shoulder-surfer | Photographs a QR | SAS confirmation on the trusted device; short expiry |
| Stolen unlocked phone | Has the device | Keystore key requires user presence; passkey requires biometric/PIN; device can be revoked |
| Malicious page | Cross-site requests | `SameSite=Strict`, origin-bound passkey, no CORS credentials, step-up assertions |
| Replayed frame | Re-sends a captured frame | Counter nonces and 1-hour rekey; a replay fails authentication |
| Insider with hub DB | Reads workflow config | Workflows hold secret *references*, never keys; vault stays on devices/workers |

## Migration from `goble-gateway`

The gateway keeps its role as a development surface, with the insecure parts fenced:

- `--pin` is accepted only with a loopback bind, and the CLI prints the deprecation. A non-loopback
  bind refuses to start without a device certificate and a passkey registration.
- The shared `GOBLE_WORKER_TOKEN` is replaced by the worker certificate. The env var is read only
  when `GOBLE_GATEWAY_DEV=1` is also set, and the relay logs a warning for every connection that
  authenticates that way.
- Sessions and worker registrations move from the in-memory maps to the registry, so a relay restart
  no longer drops every route and forces a PIN re-entry.
- The relay's frame codec stays the daemon protocol; `goble-web` keeps working against the new
  endpoint with an added `hello`/E2EE shim. Terminal replay is unchanged.

## Tasks

- [~] Define `DeviceCertificate` and `WorkerCertificate` wire types with role, serial and expiry —
      the registry records and `goble-core::identity` material exist; the wire types are pending.
- [x] Add the WebAuthn ceremonies (registration, assertion, step-up) to the relay.
- [x] Implement the pairing offer, QR payload, SAS and certificate issuance — the certificate is
      delivered in an encrypted `DeviceCredential` envelope; the caller signs it with `ClusterCa`.
- [x] Implement the E2EE frame layer and rekeying; keep the relay's frame codec opaque.
- [x] Move worker/device registration and revocation to the durable registry.
- [ ] Remove the shared worker token from the shipped path; keep the dev escape hatch fenced.
- [~] Test: replay, revoked device, expired offer, wrong SAS, relay restart, NAT reconnect — all but
      NAT reconnect are covered; the reconnect test needs a live relay socket.
