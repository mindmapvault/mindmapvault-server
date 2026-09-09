# Plan: auth surface hardening, with room for SSO

Written 2026-09-09, after the rate limiter locked out a test run and the
investigation found two real defects behind it. Same discipline — see
`CLAUDE.md`. Nothing here is implemented yet.

## How this started

Repeated E2E runs began failing with `429` on `GET /api/auth/salt`, and the
unlock dialog sat there showing "too many attempts". That is not a hang: a
vault unlock costs **two** of the 30-per-minute auth allowance — `/auth/salt`
and `/auth/login` — and it happens on every full page load, because session
keys are memory-only.

The obvious fix is to stop the client spending an anonymous budget while it
already holds a token. That fixes the false positive and **does nothing for
security**, because an attacker never uses our client. Chasing the rest turned
up the two defects below.

## What is already right, and should not be disturbed

- `/auth/login` is hardened against a username oracle: an unknown username
  still records a failure, because skipping it would make a wrong username
  measurably cheaper than a wrong password.
- The lockout in `AuthThrottle::record_failure` keys on the **username**, not
  the address, so spreading an attack across a botnet still locks the account.
- `AuthThrottle::prune` is scheduled from `main.rs`, so neither map grows
  without bound.
- `client_ip::resolve` falls back to one shared bucket when connection info is
  missing, rather than failing open.

## The defects

### D1 — `/auth/salt` is an unauthenticated username oracle

`get_salt` returns `404 "user not found"` for an unknown username and `200`
with the salt for a real one. The oracle closed on `/login` is wide open one
route away. Rate limiting narrows the funnel; it does not close it.

### D2 — the proxy setting is bypassable, and it is the setting that fixes NAT

`client_ip::resolve` takes the **left-most** `X-Forwarded-For` entry when
`trust_proxy_headers` is on. That entry is chosen by the caller. The comment in
the file already says so.

This forces a choice with no good side:

| `trust_proxy_headers` | Consequence |
|---|---|
| `false` (default) | Behind any reverse proxy every user collapses into one 30/min bucket — the false positive that started this |
| `true` | `X-Forwarded-For: <random>` yields a fresh bucket per request; the limiter is decoration |

Both halves of the reported problem are this one bug.

### D3 — one budget for endpoints of very different cost, and no aggregate ceiling

`verify_auth_token` runs a **server-side Argon2** on every login attempt. Good
for credential security, but it makes `/login` a CPU amplifier, and the rate
limit is the only thing bounding it. `/auth/salt` is a cheap indexed lookup
sharing the same allowance. The window is fixed rather than sliding, so a burst
straddling a boundary gets 2× the limit in a couple of seconds.

There is also no server-wide ceiling. Per-address budgets bound one attacker;
nothing bounds ten thousand of them in aggregate.

**This is why "only count failures" was rejected.** Argon2 runs on the success
path too, so unlimited valid logins are the same CPU DoS.

## What SSO does to this

SSO belongs to `mindmapvault-enterprise-server` — this repo's README says it is
not included here, and `SURFACE_OWNERSHIP.md` assigns it to the enterprise
overlay. **Nothing below implements SSO.** The point is to avoid pouring
concrete over the seams it will need.

The collision is not authentication, it is encryption:

```
master_key = Argon2id(password, salt, params)
auth_token = HKDF(master_key, "crypt-mind-auth-v1")   → the server stores Argon2(auth_token)
master_aes_key → decrypts classical_priv / pq_priv
```

The password is the root of **both** authentication and encryption. An IdP can
assert who someone is; it cannot produce their master key. So SSO alone can
never unlock a vault, and whichever answer the enterprise layer picks — a
separate vault passphrase after SSO, device-bound key enrolment, WebAuthn PRF,
or key escrow under a KMS — the account model has to tolerate **users with no
password-derived credential at all**.

### The two live side by side, at two levels

The interesting case is not "SSO instead of passwords". It is **both at once**,
and it happens at two independent levels:

- **Per instance.** A server offers local passwords, SSO, or both. A community
  instance is password-only today; an enterprise one will commonly run both
  during a migration, then perhaps disable local passwords by policy.
- **Per account.** A *single user* may hold a password **and** a linked SSO
  identity. This is the case the rest of this document originally missed.

So an account has a **set** of credentials, not a type. Four states matter:

| Account | Password? | SSO? | `/auth/salt` must look like |
|---|---|---|---|
| unknown | – | – | a password account |
| local | yes | no | itself |
| SSO-only | no | yes | **unknown** |
| dual | yes | yes | a password account |

The uniformity rule is therefore not "unknown vs SSO"; it is: **the response
reveals nothing beyond whether a password credential exists, and unknown
accounts are made to look as though one does.** Unknown and SSO-only are
indistinguishable; local and dual are indistinguishable. Nothing tells an
attacker which accounts are worth phishing rather than brute forcing.

### What a dual account means for the vault

Authentication and encryption come apart here, and it is worth stating plainly
because it drives the UX:

> For a dual account, signing in through the IdP authenticates the **session**.
> It does **not** unlock the vault. The master key still derives from the
> password, so the user is still asked for it at the unlock step.

That is two prompts, and it is not a defect — it is the honest consequence of
the server never holding a key. The alternative, deriving the vault key from
anything the IdP controls, hands the IdP the ability to read every vault. The
enterprise layer may later add device enrolment, WebAuthn PRF or escrow so the
second prompt is not a password; none of that changes this plan.

### Consequences that bind the work

1. **The D1 fix keys on "has a password credential", not on "exists".** See the
   table above. A dual account answers exactly like a local one.
2. **Throttling keys on a credential attempt, not on a route.** An OIDC
   callback is another unauthenticated, IdP-round-tripping entry point. Budget
   expressed per named route retrofits badly; per *class of attempt* the
   callback slots in.
3. **Lockout is per credential, never per account.** This is the sharp one. If
   failed password attempts lock the whole account, then on a dual account an
   attacker who cannot guess the password can still **deny the user their SSO
   login** by deliberately failing password attempts until the lockout trips.
   That converts a brute-force defence into a denial-of-service tool. The
   counter must be scoped to the password credential and leave the SSO path
   untouched, and vice versa.
4. **SSO-only accounts refuse password login outright** rather than counting
   failures toward a lockout — there is no password to guess, so a counter
   there is pure DoS surface.
5. **Enabling SSO must not silently widen the door.** If an instance runs both,
   a user who has only ever used a password must not become reachable through
   SSO merely because an IdP asserts a matching email. Linking is an explicit
   act; matching on an unverified email claim is a known account-takeover
   route. The community server does not implement linking, but the account
   model must not make the unsafe version the easy one.

## The work

Four steps. Each is a commit leaving `cargo test` green.

### Step 1 — close the salt oracle (D1)

`get_salt` stops distinguishing. For any username with no password credential —
unknown today, and unknown-or-SSO-only once SSO exists — return a
**deterministic pseudo-salt**:

```
pseudo_salt = HKDF(server_secret, info = "salt-oracle-v1" || normalized_username)[0..16]
```

with the instance's default Argon2 params. Deterministic, so repeat lookups
agree; server-secret-keyed, so it cannot be computed offline; indistinguishable
from a real salt. The caller then derives a key, fails to authenticate, and
learns nothing it did not already know.

- No 404 from this route any more.
- Do the DB lookup either way, so timing does not restore the oracle.
- The predicate is **"this account has a password credential"**, not "this
  account exists". Written that way now, it needs no revisiting when SSO-only
  and dual accounts appear — an SSO-only account simply falls to the same
  branch as an unknown one, and a dual account to the same branch as a local
  one.
- Tests: unknown and real users are shape-identical; the same unknown username
  returns the same salt twice; two different unknown usernames differ.

Self-contained. No config, no migration, no operator action.

### Step 2 — make the client address trustworthy (D2)

Replace the `trust_proxy_headers` boolean with a list of trusted proxy CIDRs,
and walk `X-Forwarded-For` from the **right**, returning the first entry that
is not within a trusted range. An untrusted caller can then prepend whatever it
likes and still be counted as itself.

- Setting: `trusted_proxy_cidrs`, a list. Empty means trust nothing and use the
  peer address — the safe default, and what an unconfigured instance gets.
- Migration: an existing `trust_proxy_headers = true` cannot be silently
  carried over, because it is exactly the unsafe behaviour. Migrate it to empty
  with a startup warning naming the setting, so a proxied deployment is
  loud-broken rather than quietly bypassable.
- The existing startup warning about shared allowances gets replaced by one
  that fires when the peer address is a private range and no CIDRs are
  configured — the signature of "there is a proxy in front and it is not
  declared".
- Admin console gains the field; `DEPLOYMENT.md` gains the paragraph.
- Tests: spoofed prepends are ignored; a chain through two declared proxies
  resolves the real client; an undeclared proxy resolves to the proxy.

### Step 3 — budget by cost, and bound the expensive path (D3)

- Split the single allowance into **cheap** (salt lookups) and **expensive**
  (login, register, and later the OIDC callback). Two settings, each 0 to
  disable, both still per-address.
- Replace the fixed window with a token bucket: steady refill plus a burst
  allowance, so a reload flurry is absorbed but a sustained flood is not.
- Add a **server-wide semaphore** bounding concurrent Argon2 verifies, sized
  from available parallelism. This is the only control here that a distributed
  attacker cannot sidestep by spreading across addresses; over the cap, callers
  queue briefly and then get `429`.
- Express the budget as a `CredentialAttempt` class rather than per route, so
  the OIDC callback has somewhere to attach.
- Scope the failure counter to the **credential**, not the account: today that
  is a no-op refactor of `record_failure`'s key from `username` to
  `(username, credential_kind)`, and it is what stops a future dual account
  from having its SSO login denied by deliberate password failures. Doing it
  now costs one struct field; doing it later means reasoning about live
  lockout state.

### Step 4 — stop the client spending the anonymous budget

`UnlockModal` currently calls `/auth/salt` then `/auth/login`, and **throws away
the tokens `/login` returns** — it only wants the key bundle. `GET /auth/keys`
is authenticated, unthrottled, and already returns `argon2_salt`,
`argon2_params` and the encrypted private keys; its doc comment says it exists
so a client can re-derive the master key "without a separate unauthenticated
salt request". Use it, and fall back to the current path on `401`.

Unlock then costs zero anonymous calls.

**The trade-off, which needs a decision.** Password verification moves from the
server's Argon2 check to a local AES-GCM tag check, so unlock attempts stop
feeding `failed_login_threshold`. Brute-forcing the unlock screen becomes
unmetered. The counter-argument is that whoever is at that screen already holds
a valid session token and can fetch the encrypted bundle and attack it offline
regardless, so the meter was not buying much. Recorded as an open question
rather than assumed.

## Order, and what can ship alone

Step 1 is independent and could ship today. Step 2 is the one that matters most
for a real deployment and is the only one needing operator action. Step 3 is
the largest and touches the throttle core. Step 4 is comfort, and deliberately
last so a security change and a UX change are not reviewed as one diff.

## Open questions

- **Step 4's trade-off** above: accept the unmetered unlock screen, or keep a
  local attempt counter, or leave `/login` in the unlock path and accept the
  cost.
- **Pseudo-salt params.** Returning the instance default advertises what the
  default is. Harmless, but worth a second opinion.
- **Semaphore sizing.** A cap that is too low turns a busy legitimate morning
  into `429`s. Wants a number measured on real hardware, not guessed.
- **Lockout and SSO-only accounts.** Refusing password login outright is the
  clean answer, but it needs the account model to carry a credential kind,
  which does not exist yet and is arguably the enterprise layer's to define.
- **How much of the credential model belongs here.** Step 1 and step 3 both
  read better if an account can say "I have a password" and "I have an SSO
  identity" separately. The minimum this repo needs is a single
  `credential_kind` discriminator on the existing auth columns; the full
  identity-linking model belongs to the enterprise overlay. Drawing that line
  in the wrong place either blocks the enterprise layer or drags enterprise
  concepts into the AGPL server. Worth settling before step 3.
- **Whether a dual account may unlock without its password.** Everything above
  assumes not, and says so plainly to users. If the enterprise layer later
  wants single-prompt SSO unlock, that requires escrow or device enrolment and
  is a product decision about zero-knowledge, not a technical gap here.
- Whether `trusted_proxy_cidrs` should also gate other IP-derived behaviour
  (audit log entries, the status page) or stay scoped to throttling.
