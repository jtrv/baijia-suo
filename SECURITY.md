# Security Policy

## Status

`baijia-suo` is a personal, open-source project. It has **not** been
independently security-audited. It is a screen locker — a security boundary —
so please treat it accordingly and evaluate it for your own threat model before
relying on it.

The design goal is **fail-closed**: every abnormal exit (crash, signal, Wayland
disconnect, config error, auth-init failure) leaves the compositor holding the
`ext-session-lock`, so the screen stays locked. The session unlocks only after a
successful PAM authentication. If you find a path that violates this, it is a
security bug — please report it.

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
