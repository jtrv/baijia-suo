baijia-suo(1)

# NAME

baijia-suo - a secure Wayland screen locker

# SYNOPSIS

*baijia-suo* [options...]

# DESCRIPTION

*baijia-suo* is a screen locker for Wayland compositors that support the
*ext-session-lock-v1* protocol. Authentication runs out-of-process in an
isolated, privilege-dropping child via PAM, password buffers are locked into
RAM and zeroized, and the session is fail-closed: if the locker exits
abnormally the compositor keeps the screen locked.

With no animation configured the background is a solid color; one or more
animation modes may be shown instead, optionally cycling between them.

# OPTIONS

*-h, --help*
	Print help and exit.

*-V, --version*
	Print version and exit.

*-C, --config* <path>
	Read configuration from <path> instead of the default locations (see
	*FILES*).

*-c, --color* <color>
	Background color as an RRGGBB or RRGGBBAA hex string.

*-A, --animation* <modes>
	Background animation mode(s). Repeat the flag or comma-separate to cycle
	through several (e.g. *-A spiral,flame*); *random* or *all* selects every
	registered mode. Omit for a solid background. See *--list-animations*.

*--cycle* <seconds>
	Seconds each animation plays before cycling to the next. Only applies when
	more than one mode is selected. Default 60.

*--list-animations*
	Print the available animation mode names and exit.

*--indicator-mode* <style>
	Typing-indicator style: *fade* (default), *pin-tumbler*, *comet*, *breath*,
	*dots*, *scope*, or *ripple*. All but *pin-tumbler* avoid revealing the
	password length on screen.

*--indicator-opacity* <value>
	Indicator opacity from *0.0* to *1.0*. Default *1.0* (fully opaque).

*--indicator-color* <color>
	Indicator disk color for the idle and typing states, as an RRGGBB hex
	string.

*-d, --daemonize*
	Detach from the terminal after the screen is successfully locked.

*-R, --ready-fd* <fd>
	Write a newline to file descriptor <fd> once the lock is fully active, then
	close it. Useful for scripts that must wait until the screen is locked
	before, for example, suspending the system.

*--debug*
	Enable verbose debug logging on stderr.

*--auth-backend* <backend>
	Authentication backend. *pam* (default); *shadow* is an accepted alias for
	the same unprivileged PAM path (baijia-suo never reads _/etc/shadow_
	directly).

*--username* <username>
	Authenticate as <username> instead of the current user.

*--auth-test*
	Prompt for a password in the terminal and verify it against the selected
	backend without locking the screen. Useful for testing PAM configuration.

# FILES

_$XDG_CONFIG_HOME/baijia-suo/config.toml_++
_~/.config/baijia-suo/config.toml_++
_/etc/baijia-suo/config.toml_
	Configuration file. The first that exists is used unless *-C* is given.
	Command-line flags override values from the file.

_/etc/pam.d/baijia-suo_
	PAM service definition. If absent, baijia-suo falls back to
	_/etc/pam.d/login_.

# CONFIGURATION

Keys mirror the long option names. Example:

	color = "1a2b3c"
	animation = ["spiral", "flame"]   # a string for one mode, or "random"
	cycle = 60
	daemonize = false

	\[indicator]
	mode = "pin-tumbler"
	opacity = 1.0
	color = "3c3c3c"

Unknown keys are rejected. See _config.example.toml_ in the distribution for a
fully commented example.

# EXAMPLES

Lock with a solid color and daemonize:

	baijia-suo --color 1a2b3c --daemonize

Cycle through several animations, 45 seconds each:

	baijia-suo --animation spiral,flame,worm --cycle 45

Shuffle every mode:

	baijia-suo --animation random --cycle 30

# SECURITY

Passwords are held in page-aligned, *mlock*'d buffers and zeroized after use;
the authentication child additionally uses *mlockall* and disables core dumps
and *ptrace* (*PR_SET_DUMPABLE*). Authentication runs in a forked child that
drops any inherited elevated gid/uid before invoking PAM and communicates with
the main process over an anonymous pipe. The session is fail-closed: any
abnormal exit leaves the compositor holding the lock.

# AUTHORS

The baijia-suo contributors.
