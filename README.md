# caffeinate-windows

[![CI](https://github.com/skhortiuk/caffeinate-windows/actions/workflows/ci.yml/badge.svg)](https://github.com/skhortiuk/caffeinate-windows/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/skhortiuk/caffeinate-windows)](https://github.com/skhortiuk/caffeinate-windows/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

The macOS `caffeinate` command, for Windows. Keeps your PC or display awake
while a command runs, for a set time, until another process exits, or until
you press Ctrl+C.

```
caffeinate -di cargo build      # stay awake while the build runs
caffeinate -d -t 3600           # keep the display on for an hour
```

No service, no registry edits, no power-plan changes, no admin rights. The
hold is released the moment caffeinate exits.

## Install

Paste into PowerShell. Nothing else needed.

```powershell
irm https://raw.githubusercontent.com/skhortiuk/caffeinate-windows/main/install.ps1 | iex
```

This downloads `caffeinate.exe` from the latest release, verifies its SHA256,
puts it in `%LOCALAPPDATA%\Programs\caffeinate`, and adds that folder to your
PATH. It works in that window right away; other open terminals need a restart.

Piping a script from the internet into your shell deserves a look first.
It is short: [install.ps1](install.ps1), or print it with
`irm https://raw.githubusercontent.com/skhortiuk/caffeinate-windows/main/install.ps1`.

**Manual:** download `caffeinate.exe` from
[Releases](https://github.com/skhortiuk/caffeinate-windows/releases/latest) and put
it anywhere on your PATH. The binary is not code-signed, so a browser download
may trigger a SmartScreen warning.

**Update:** `caffeinate --update`, or re-run the install line. Either one
is a no-op when you already have the latest release. `caffeinate -V` shows
the installed version.

**Uninstall:**

```powershell
Remove-Item -Recurse "$env:LOCALAPPDATA\Programs\caffeinate"
$p = (Get-ItemProperty HKCU:\Environment).Path -split ";" | Where-Object { $_ -notlike "*Programs\caffeinate" }
Set-ItemProperty HKCU:\Environment Path ($p -join ";") -Type ExpandString
```

**Requirements:** Windows 10 or 11, 64-bit. ARM64 runs the x64 binary under
emulation.

## Usage

```
caffeinate [-dimsu] [-t timeout] [-w pid] [utility arguments...]
```

| Flag | Effect |
|------|--------|
| `-d` | keep the display on |
| `-i` | keep the system awake (default when no flag is given) |
| `-m` | same as `-i` (Windows has no separate disk-idle hold) |
| `-s` | same as `-i` (held on battery too, unlike macOS) |
| `-u` | reset the display idle timer now, then hold for `-t` seconds (default 5) |
| `-t N` | release after N seconds, even if a utility or watched process is still running |
| `-w PID` | release when process PID exits |
| `-V` | print the version |
| `--update` | install the latest release |

```
caffeinate                      # system awake until Ctrl+C
caffeinate -di -t 3600          # display and system awake for one hour
caffeinate -w 4242              # until PID 4242 exits
caffeinate -i cargo build       # while the build runs; returns cargo's exit code
caffeinate -i -t 7200 robocopy D:\ E:\backup /MIR   # hold for at most two hours
```

Flags combine: `-di` is the same as `-d -i`. Anything after the flags is run
as a command; use `--` if the command itself starts with `-`.

Wrappers like `npm`, `yarn` or `pnpm` are `.cmd` files, so call them as
`npm.cmd` or `cmd /c npm ...`.

## How it works

caffeinate calls the Win32
[`SetThreadExecutionState`](https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-setthreadexecutionstate)
API with `ES_CONTINUOUS` plus `ES_SYSTEM_REQUIRED` (for `-i`) and
`ES_DISPLAY_REQUIRED` (for `-d`). Windows tracks that request per process, so
it disappears as soon as caffeinate exits for any reason, including Ctrl+C or
closing the terminal.

You can see the hold from an elevated PowerShell while caffeinate runs:

```
PS> powercfg /requests
DISPLAY:
[PROCESS] \Device\HarddiskVolume3\Users\you\AppData\Local\Programs\caffeinate\caffeinate.exe

SYSTEM:
[PROCESS] \Device\HarddiskVolume3\Users\you\AppData\Local\Programs\caffeinate\caffeinate.exe
```

## Differences from macOS

- `-d` alone keeps only the display awake. Use `-di` for display and system.
- `-s` holds on battery as well as AC power.
- `-m` is an alias for `-i`; Windows has no disk-idle assertion.
- `-u` resets the display idle timer. It does not power on a display that is
  already off.
- `-w` and a utility command cannot be combined.
- It does not override a lid-close action or a manual sleep.

## From source

Needs Rust from [rustup.rs](https://rustup.rs).

```
cargo install --locked --git https://github.com/skhortiuk/caffeinate-windows
```

## Contributing

Issues and pull requests welcome. Before pushing:

```
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
```

## License

[MIT](LICENSE)
