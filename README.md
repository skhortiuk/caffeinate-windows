# caffeinate-windows

The macOS `caffeinate` command, for Windows. Keeps the system or display
awake while a command runs, for a timeout, until a process exits, or until
you press Ctrl+C.

No service, no registry edits, no power-plan changes. Uses the Win32
`SetThreadExecutionState` API; the hold is released the moment the process
exits.

## Install

Paste into PowerShell. Nothing else needed.

```powershell
irm https://raw.githubusercontent.com/OWNER/caffeinate-windows/main/install.ps1 | iex
```

Downloads `caffeinate.exe` from the latest release into
`%LOCALAPPDATA%\Programs\caffeinate` and adds it to your PATH. Open a new
terminal and run `caffeinate`.

Manual: grab `caffeinate.exe` from [Releases](https://github.com/OWNER/caffeinate-windows/releases) and put it anywhere on your PATH.

Uninstall: delete that folder and remove it from PATH.

## Usage

```
caffeinate [-dimsu] [-t timeout] [-w pid] [utility arguments...]
```

| Flag | Effect |
|------|--------|
| `-d` | keep the display on |
| `-i` | keep the system awake (default when no flag is given) |
| `-m` | same as `-i` (Windows has no disk-idle assertion) |
| `-s` | same as `-i` |
| `-u` | wake the display now and hold for `-t` seconds (default 5) |
| `-t N` | release after N seconds |
| `-w PID` | release when process PID exits |

```
caffeinate                      # awake until Ctrl+C
caffeinate -d -t 3600           # display and system awake for one hour
caffeinate -w 4242              # until PID 4242 exits
caffeinate -di cargo build      # during a build; returns cargo's exit code
```

Confirm the hold from an elevated shell:

```
powercfg /requests
```

## Notes

- Does not override a lid-close action or a manual sleep.
- `-d` alone keeps only the display awake. Use `-di` for both.

## From source

```
cargo install --git https://github.com/OWNER/caffeinate-windows
```

## License

MIT
