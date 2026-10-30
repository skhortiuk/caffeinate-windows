# caffeinate (Windows)

A port of the macOS `caffeinate` command. Keeps Windows from sleeping or
turning off the display for as long as the process runs, a timeout, another
process's lifetime, or a wrapped command.

Built on `SetThreadExecutionState`; no services, no registry, no power-plan
changes. Assertions vanish the instant the process exits.

## Build

```
cargo build --release
copy target\release\caffeinate.exe %USERPROFILE%\bin\
```

## Usage

```
caffeinate [-dimsu] [-t timeout] [-w pid] [utility arguments...]
```

| Flag | Effect |
|------|--------|
| `-d` | keep the display on |
| `-i` | keep the system from idle sleep (default when no flag given) |
| `-m` | same as `-i` (Windows has no disk-idle assertion) |
| `-s` | same as `-i` |
| `-u` | wake the display now; hold for `-t` seconds (default 5) |
| `-t N` | release after N seconds |
| `-w PID` | release when process PID exits |

Examples:

```
caffeinate                      # system awake until Ctrl+C
caffeinate -d -t 3600           # display and system awake for one hour
caffeinate -w 4242              # until PID 4242 exits
caffeinate -i cargo build       # during a build; returns cargo's exit code
```

Check what is held while it runs:

```
powercfg /requests
```

## Notes

- Does not override a lid-close action or a manual sleep request.
- `-d` alone keeps only the display awake; use `-di` for display and system.
