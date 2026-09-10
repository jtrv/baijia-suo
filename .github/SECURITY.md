# Security Policy

## Status

`baijia-suo` is a personal, open-source project. It has **not** been
independently security-audited. It is a screen locker — a security boundary —
so please treat it accordingly and evaluate it for your own threat model before
relying on it.

## What the guarantee is

The design goal is **fail-closed**, in two phases that are worth separating
because only the second is absolute.

**Before the lock is acquired**, startup can refuse. If the authentication
backend cannot be initialised, the locker exits rather than locking a screen it
might not be able to unlock, and a malformed command line exits with an error
you see in your terminal. In both cases nothing is locked. A broken *config
file* is deliberately not in this category: it is discarded with a warning and
the lock proceeds on an opaque black background with no animation, because a
typo in a colour silently leaving your screen unlocked is a worse failure than
an ugly lock screen.

**After the lock is acquired**, nothing releases it except a settled successful
PAM authentication. A crash, a panic (the release profile aborts), any signal,
a Wayland disconnect, a render error, a hung or killed authentication helper —
all of these end the process without sending an unlock request, and the
compositor keeps the session locked. The failure mode is a lockout requiring a
virtual terminal, never an exposed desktop.

If you find a path that violates the second guarantee, it is a security bug —
please report it.

## How unlocking works

The whole property lives in three hops, and you can check it with one grep:

1. `src/app.rs` sets `AuthState::Success` only from the verifier's reply, which
   the child sends only when `pam_authenticate` **and** `pam_acct_mgmt` both
   succeed (`src/auth/pam_ffi.rs`).
2. `src/wayland.rs` has a single `should_unlock(auth_state, auth_settled)`,
   true only for settled `Success`, and it guards the only assignment of
   `unlock_on_exit = true`.
3. That flag is read once, and gates the only `unlock_and_destroy()` call in
   the codebase.

```sh
rg 'unlock_and_destroy|unlock_on_exit = true' src/
```

Three matches, all in `src/wayland.rs`: the flag's single assignment, the
single `unlock_and_destroy()` call, and one doc comment mentioning it.
`session_unlock_has_one_authentication_gated_path` asserts those counts stay
at one each after stripping comments, so adding a second unlock path fails CI
rather than passing unnoticed. `only_settled_success_unlocks` walks every
`AuthState` in both settled and unsettled forms against `should_unlock`.

## Threat model

**In scope.** An attacker at the keyboard of a locked machine. A malicious or
buggy process running as the same user. A hostile or broken PAM stack, and the
message text it returns, which is rendered on the lock screen.

**Out of scope.** The compositor, which is trusted: it enforces
`ext-session-lock-v1` and can see everything anyway. Root. Physical access
beyond the keyboard, including virtual-terminal switching, which is the
compositor's and the kernel's policy rather than this program's. Attacks on
PAM modules themselves.

## What to read if you are auditing this

The repository is about 45,000 lines, but roughly 35,000 of those are animation
modes ported from xlockmore and xscreensaver, which cannot reach passwords or
the lock. They receive a pixel buffer and its dimensions, nothing else, and
`#![forbid(unsafe_code)]` on `src/animation/mod.rs` makes that structural
rather than a promise.

The security-relevant surface is 11 files:

| Area | Files | Lines (excl. tests) |
|---|---|---|
| Lock lifecycle | `wayland.rs`, `app.rs` | 1,669 |
| Authentication | `auth/{mod,verifier,ipc,pam_ffi}.rs` | 1,247 |
| Password memory | `secure/buffer.rs`, `password/buffer.rs` | 332 |
| Startup, input, buffers | `cli.rs`, `input/keyboard.rs`, `render/pool.rs` | 690 |
| **Total** | **11 files** | **3,938** |

Add `args.rs` and `config.rs` if you want to see everything that parses
untrusted input.

## Unsafe code

`unsafe` exists only for FFI and is confined to named modules. The crate root
denies it, with explicit exceptions on `auth`, `secure`, `cli`, `wayland`,
`input`, `render` and `rng`. Every other module — including all of `animation`,
the indicator, `app`, `args`, `config` and `password` — carries
`#![forbid(unsafe_code)]`, which cannot be overridden from inside.

```sh
rg -c '\bunsafe\b' src/
```

Note this says nothing about `unsafe` inside dependencies.

## Memory handling, stated precisely

Password buffers are page-aligned, `mlock`ed, and wiped with volatile writes
before being freed. But locking is **best effort**: `mlock` failing with
`EPERM` or `ENOMEM` is tolerated rather than fatal, the child's `mlockall`
return is not checked, and process hardening only warns on failure. PAM itself
holds a copy of the password whose lifetime and erasure belong to PAM, not to
this program. Treat "the password is never swapped" as an intention, not a
guarantee.

## Dependencies

Nine direct dependencies; 33 crates in the shipped binary's tree. The lockfile
lists more because of development and benchmarking dependencies, which never
reach the binary.

```sh
cargo tree -e normal --prefix none | sed 's/ (.*//' | sort -u | wc -l
```

The bundled font is Liberation Sans under the SIL Open Font License; its glyph
outlines are extracted at development time by `scripts/gen-glyphs` and
committed, and CI regenerates them and compares byte for byte, so the generated
table can be verified rather than trusted. Native libraries linked at runtime
are PAM and xkbcommon.

## Reporting a vulnerability

Please report suspected vulnerabilities **privately** rather than opening a
public issue:

- Use GitHub's [private vulnerability reporting][gh-report] on this repository
  ("Security" tab → "Report a vulnerability"), or
- Open a minimal public issue asking for a private contact **without** details
  if the above is unavailable.

Please include the compositor and version, reproduction steps, and the expected
vs. actual behavior. There is no bounty; this is a best-effort hobby project,
but security reports will be prioritized over feature work.

[gh-report]: https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability
