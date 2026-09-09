complete -c baijia-suo -s C -l config -d 'Path to config file' -r
complete -c baijia-suo -s c -l color -d 'Background color in RRGGBB or RRGGBBAA format' -r
complete -c baijia-suo -s R -l ready-fd -d 'File descriptor to send readiness notification' -r
complete -c baijia-suo -s A -l animation -d 'Animation mode(s). Repeat or comma-separate to cycle through several (e.g. `-A spiral,flame`); "random" or "all" uses every mode. Omit for a solid background' -r
complete -c baijia-suo -l cycle -d 'Seconds each animation plays before cycling to the next; only applies with multiple modes. Default 60' -r
complete -c baijia-suo -l max-fps -d 'Cap animation frame rates (fps). Unset = uncapped, each mode\'s own clock (some run at 100+ fps)' -r
complete -c baijia-suo -l low-battery-percent -d 'Suspend animations (solid background) when the battery is discharging at or below this percent. Unset = never' -r
complete -c baijia-suo -l indicator-mode -d 'Typing indicator style: fade (default), pin-tumbler, comet, breath, dots, scope, or ripple. All but pin-tumbler avoid revealing the password length on screen' -r
complete -c baijia-suo -l indicator-opacity -d 'Indicator opacity, 0.0-1.0 (default 1.0 = fully opaque)' -r
complete -c baijia-suo -l indicator-color -d 'Indicator disk color for idle/typing states, RRGGBB' -r
complete -c baijia-suo -l auth-backend -d 'Authentication backend to use' -r
complete -c baijia-suo -l username -d 'Username for authentication' -r
complete -c baijia-suo -l debug -d 'Enable debug logging'
complete -c baijia-suo -s d -l daemonize -d 'Detach from terminal after locking'
complete -c baijia-suo -l no-daemonize -d 'Stay in the foreground even if the config file sets daemonize. Lets a supervisor (systemd-inhibit, systemd-run --wait, shell `wait`) hold the locker\'s lifetime per-invocation'
complete -c baijia-suo -l debug-timing -d 'Log per-frame timing (animation advance vs present cost, 1/s aggregate) for performance work'
complete -c baijia-suo -l list-animations -d 'List available animation modes and exit'
complete -c baijia-suo -l auth-test -d 'Test authentication only (does not lock screen)'
complete -c baijia-suo -s V -l version -d 'Print version'
