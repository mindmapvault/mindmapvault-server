# Security Policy - MindMapVault Server

This document defines the security model, threat boundaries, and disclosure
process for the self-hosted MindMapVault server (AGPL-3.0-or-later).

## Scope

The server stores and serves encrypted vault artifacts for clients. Content is
encrypted on the client before it is uploaded, so the server is intended to hold
opaque ciphertext rather than readable notes or maps.

Security goals:

- keep vault content unreadable to the server operator and to anyone with
  database or backup access
- authenticate clients without requiring personal data such as an email address
- keep security-relevant behavior auditable in this repository

Out of scope:

- protection after full compromise of the host running the server
- protection against a malicious client that already holds the vault passphrase
- anonymity or anti-traffic-analysis guarantees
- security of deployments that modify the encryption boundary

## Vulnerability Reporting

Please report vulnerabilities privately and do not open a public issue for
active security concerns.

Recommended process:

1. Preferred: open a private report through [GitHub Security Advisories](https://github.com/mindmapvault/mindmapvault-server/security/advisories/new).
2. Alternative: email <security@mindmapvault.com>.
3. Include reproduction steps, impact, affected versions/commits, and proposed
   mitigations if possible.

Response targets (best effort):

- initial triage acknowledgment: within 7 days
- status update after verification: within 14 days

Disclosure policy:

- coordinated disclosure is preferred
- fixes may be released before full technical details are published
- public write-up should follow once operators have practical upgrade guidance

## Operator Responsibilities

A self-hosted deployment is only as secure as the host it runs on. Operators
should:

- terminate TLS in front of the server and refuse plaintext HTTP
- keep the container image and host packages patched
- restrict database and backup access to the smallest possible set of accounts
- treat database backups as sensitive, even though vault content is encrypted
- avoid modifying client-side encryption, which would move plaintext into scope

## Reporting Something That Is Not a Vulnerability

Deployment questions, configuration problems, and bugs without a security impact
belong in the public issue tracker. Reserving the private channel for real
security issues keeps response times meaningful.
