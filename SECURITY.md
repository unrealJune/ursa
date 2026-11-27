# Security

Ursa's job is to keep your files confidential and intact on machines you don't control, so
security reports are the most useful kind we get.

## Reporting a vulnerability

Please report it privately through GitHub:
[**Report a vulnerability**](https://github.com/unrealJune/ursa/security/advisories/new).
Don't open a public issue for it.

Include what you found, how to reproduce it, and what an attacker gains. You'll get an answer
within a week; a fix and an advisory follow once it is confirmed, crediting you unless you'd
rather not be named.

## Scope

In scope: anything that lets a node, a relay, a network observer or another account read or
silently alter a user's data, impersonate a device or an account, or break recovery; and the
release pipeline (an update or image that isn't what CI built from this repository).

The threat model is in [docs/design.md](docs/design.md#threat-model).

## Supported versions

Only the latest release of each product (Windows, Android, node) gets fixes.
