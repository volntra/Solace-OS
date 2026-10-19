# Service unit format (sol-init)

A unit is a TOML file named `<name>.toml`. `<name>` is the unit's name
(`[a-z0-9][a-z0-9@._-]*`, at most 64 bytes).

Units are read from, in order (a later file with the same name wins):

1. `/usr/lib/sol/units/` (shipped by packages)
2. `/etc/sol/units/` (administrator overrides)

A unit starts at boot when it is **enabled**: a file named `<name>`
exists in `/etc/sol/enabled/` (written by `solctl enable`), or the unit
sets `enabled = true` and has not been disabled (`/etc/sol/disabled/<name>`).
Starting a unit starts everything it `requires` first.

```toml
description = "Login prompt on tty1"
enabled = true            # start at boot unless disabled

requires = ["early"]      # must be running (or done, for oneshot) first;
                          # if a requirement fails, this unit is not started
after = ["udev"]          # ordering only: if both start, start after these

[service]
exec = ["/usr/sbin/agetty", "--noclear", "tty1", "linux"]
type = "simple"           # simple | oneshot | notify
user = "root"             # name or numeric uid (default root)
group = "root"
working_dir = "/"
environment = { TERM = "linux" }
restart = "always"        # no | on-failure | always (default on-failure)
restart_delay_ms = 500    # first delay; doubles per crash up to 30 s
stop_timeout_ms = 5000    # SIGTERM, then SIGKILL after this long

[sandbox]
no_new_privs = true       # default true
read_only_paths = ["/usr"]

[limits]                  # cgroups v2, /sys/fs/cgroup/sol/<name>/
memory_max = "512M"       # bytes, or K/M/G suffix
tasks_max = 256
cpu_weight = 100          # 1..10000
```

## Types and readiness

| type | ready when | done when |
|---|---|---|
| `simple` | the process has been started | (runs until stopped) |
| `oneshot` | it exits with status 0 | same; a non-zero exit is a failure |
| `notify` | it sends a datagram `READY=1` to the Unix socket at `$NOTIFY_SOCKET` | (runs until stopped) |

A unit is not started until every unit in `requires` is ready.
Independent units start in parallel.

## Restarts

`on-failure` restarts after a non-zero exit or a signal; `always` also
after a clean exit (not for oneshot). Delays double per consecutive crash
(capped at 30 s) and reset after the unit stays up for 30 s. After 10
crashes within 5 minutes the unit is marked `failed` and left alone until
`solctl restart`.

## Shutdown

Units stop in reverse start order: SIGTERM to the whole cgroup, SIGKILL
after `stop_timeout_ms`. Then sol-init saves the random seed, syncs,
remounts filesystems read-only, unmounts what it can, and reboots or
powers off.

## Logs

Each service's stdout and stderr are captured line by line into sol-log
(`/var/log/sol/log.jsonl`, rotated at 16 MiB, plus an in-memory ring for
the current boot). `solctl logs [unit] [-n N]` reads them. Every record is
one JSON object: `{"t": unix_ms, "unit": "...", "stream": "out|err|init",
"msg": "..."}`.

## Phase 2 status

Implemented: everything above except `[sandbox] read_only_paths`, which
is parsed and validated but **not yet applied** (it needs a private mount
namespace per service; planned for Phase 6 with the session services).
`no_new_privs` and the `[limits]` cgroup settings are applied.
