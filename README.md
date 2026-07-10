# baijia-suo
*A secure, high-performance Wayland screen locker.*

`baijia-suo` (百家锁 - "The Lock of a Hundred Families") is a screen locker for Wayland compositors supporting the `ext-session-lock-v1` protocol. It is built in Rust with a focus on security: out-of-process PAM authentication, mlocked and zeroized secret buffers, process hardening, and a fail-closed design.

> **Status:** personal open-source project, **not** independently security-audited. A screen locker is a security boundary — evaluate it against your own threat model before relying on it. See [`SECURITY.md`](SECURITY.md).

## Features

- **Secure by Design:**
  - Out-of-process PAM authentication to prevent memory leaks in the compositor.
  - Zeroized password buffers to ensure secrets are securely wiped.
  - Page-aligned, `mlock`ed password buffers; the auth child is fully pinned in RAM with `mlockall`.
  - Core dumps and non-root `ptrace` disabled (`PR_SET_DUMPABLE`).
- **Modern Wayland:**
  - Uses the `ext-session-lock-v1` protocol.
  - Graceful crash handling; Wayland session locks cannot be bypassed even if the locker terminates unexpectedly.
- **Customizable Rendering:**
  - RGBA background colors and a large set of animated backgrounds
    (xlockmore / xscreensaver ports).
  - Configurable typing indicator animations.

## Usage

Run the locker from a terminal within your Wayland session:

```sh
baijia-suo --color 1a2b3cFF --animation spiral --daemonize
```

Pass several modes to cycle through them (shuffled), with `--cycle` seconds
of airtime each; `random` (or `all`) selects every mode:

```sh
baijia-suo --animation spiral,flame,worm --cycle 45
baijia-suo --animation random --cycle 30
```

### Configuration Options

Options can be passed on the command line or set in a TOML config file,
loaded from `-C <path>` if given, else `~/.config/baijia-suo/config.toml`
(respecting `$XDG_CONFIG_HOME`), else `/etc/baijia-suo/config.toml`.
Command-line flags override file values.

```toml
color = "1a2b3c"                 # background, RRGGBB(AA)
animation = ["ripple", "flame"]  # one mode, a list to cycle, or "random"
cycle = 60                       # seconds per mode when cycling
daemonize = false

[indicator]
mode = "fade"          # fade, pin-tumbler, comet, breath, dots, scope, ripple
opacity = 1.0          # 0.0–1.0
color = "3c3c3c"       # idle/typing disk color
```

See `config.example.toml` in this repository for a commented example.

For a full list of options, run `baijia-suo --help` or see the
`baijia-suo(1)` man page.

## Architecture

1. **Main Process:** Handles Wayland protocol events, connects to the compositor, renders the UI, and captures keyboard inputs. It operates unprivileged, with memory locked and core dumps / `ptrace` disabled (`PR_SET_DUMPABLE`).
2. **Authentication Child:** An isolated process that safely manages password verification via PAM. It drops any inherited elevated gid/uid before running the PAM stack, and communicates with the main process over an anonymous pipe.

## Security

See [`SECURITY.md`](SECURITY.md) for the fail-closed design intent and how to
report a vulnerability privately.

## License

`baijia-suo`'s own code is MIT-licensed — see [`LICENSE`](LICENSE).

The binary also includes third-party work under its own terms, summarized in
`LICENSE` and retained in place:

- **Animation modes** (`src/animation/modes/`) are ports of xlockmore /
  xscreensaver animations; each file preserves its original author's permissive
  copyright notice.
- **Liberation Sans** (`fonts/LiberationSans-Regular.ttf`) is under the SIL Open
  Font License 1.1 — see [`fonts/LICENSE`](fonts/LICENSE).
