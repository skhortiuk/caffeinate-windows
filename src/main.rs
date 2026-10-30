//! caffeinate for Windows.
//!
//! Mirrors the macOS `caffeinate(8)` command-line interface on top of the
//! Win32 `SetThreadExecutionState` API. While the process runs, the
//! requested assertions are held; they are released automatically when the
//! process exits (including Ctrl+C), so no cleanup handler is required.

#![cfg(windows)]

use std::env;
use std::fmt;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::process::{self, Command};
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Power::{
    SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
    EXECUTION_STATE,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const INSTALL_URL: &str =
    "https://raw.githubusercontent.com/skhortiuk/caffeinate-windows/main/install.ps1";

const USAGE: &str = "\
usage: caffeinate [-dimsu] [-t timeout] [-w pid] [utility arguments...]

Prevent the system from sleeping while the command runs.

  -d          prevent the display from sleeping
  -i          prevent the system from idle sleeping
  -m          prevent the disk from idle sleeping (same as -i on Windows)
  -s          prevent the system from sleeping while on AC power (same as -i)
  -u          declare the user is active: resets the display idle timer, then
              holds the assertion for -t seconds (default 5 when no -t, -w or
              utility is given)
  -t timeout  release the assertion after `timeout` seconds, even if a
              utility or watched process is still running
  -w pid      release the assertion when process `pid` exits
  -h, --help  show this help
  -V, --version
              show the version
  --update    download and install the latest release

With no flags, -i is assumed. If a utility is given, it is run with the
remaining arguments and the assertion is held until it exits; caffeinate
returns the utility's exit code. -w and a utility cannot be combined.
";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Options {
    display: bool,
    system: bool,
    user_active: bool,
    timeout: Option<u64>,
    wait_pid: Option<u32>,
    command: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum ParseError {
    Help,
    Version,
    Update,
    UnknownFlag(String),
    MissingValue(char),
    BadValue(char, String),
    Conflict(&'static str),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Help => write!(f, "{USAGE}"),
            ParseError::Version => write!(f, "caffeinate {VERSION}"),
            ParseError::Update => write!(f, "caffeinate: updating"),
            ParseError::UnknownFlag(s) => write!(f, "caffeinate: unknown option '{s}'"),
            ParseError::MissingValue(c) => write!(f, "caffeinate: option -{c} requires a value"),
            ParseError::BadValue(c, v) => write!(f, "caffeinate: invalid value for -{c}: '{v}'"),
            ParseError::Conflict(msg) => write!(f, "caffeinate: {msg}"),
        }
    }
}

fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Options, ParseError> {
    let mut opts = Options::default();
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" => {
                opts.command.extend(args);
                break;
            }
            "-h" | "--help" => return Err(ParseError::Help),
            "-V" | "--version" => return Err(ParseError::Version),
            "--update" => return Err(ParseError::Update),
            s if s.starts_with("--") => return Err(ParseError::UnknownFlag(arg)),
            s if !s.starts_with('-') || s.len() < 2 => {
                // First non-option token starts the utility command line.
                opts.command.push(arg);
                opts.command.extend(args);
                break;
            }
            _ => {}
        }

        let mut chars = arg[1..].chars();
        while let Some(c) = chars.next() {
            match c {
                'd' => opts.display = true,
                'i' | 'm' | 's' => opts.system = true,
                'u' => opts.user_active = true,
                't' | 'w' => {
                    // Value is either the rest of this token (-t30) or the next one.
                    let rest: String = chars.by_ref().collect();
                    let value = if rest.is_empty() {
                        args.next().ok_or(ParseError::MissingValue(c))?
                    } else {
                        rest
                    };
                    let n: u64 = value
                        .parse()
                        .map_err(|_| ParseError::BadValue(c, value.clone()))?;
                    if c == 't' {
                        opts.timeout = Some(n);
                    } else {
                        let pid =
                            u32::try_from(n).map_err(|_| ParseError::BadValue(c, value.clone()))?;
                        opts.wait_pid = Some(pid);
                    }
                }
                other => return Err(ParseError::UnknownFlag(format!("-{other}"))),
            }
        }
    }

    if opts.wait_pid.is_some() && !opts.command.is_empty() {
        return Err(ParseError::Conflict(
            "-w cannot be combined with a utility command",
        ));
    }

    // macOS semantics: -u alone defaults to a 5 second timeout.
    if opts.user_active
        && opts.timeout.is_none()
        && opts.wait_pid.is_none()
        && opts.command.is_empty()
    {
        opts.timeout = Some(5);
    }
    // No assertion flags at all means "keep the system awake".
    if !opts.display && !opts.system && !opts.user_active {
        opts.system = true;
    }
    Ok(opts)
}

fn execution_state(opts: &Options) -> EXECUTION_STATE {
    let mut flags = ES_CONTINUOUS;
    if opts.system {
        flags |= ES_SYSTEM_REQUIRED;
    }
    if opts.display || opts.user_active {
        flags |= ES_DISPLAY_REQUIRED;
    }
    flags
}

fn timeout_millis(timeout: Option<u64>) -> u32 {
    match timeout {
        // INFINITE is u32::MAX; clamp one below so a huge -t never means "forever".
        Some(secs) => secs.saturating_mul(1000).min(u32::MAX as u64 - 1) as u32,
        None => INFINITE,
    }
}

fn assert_awake(flags: EXECUTION_STATE) {
    // SAFETY: plain Win32 call with no pointers.
    let prev = unsafe { SetThreadExecutionState(flags) };
    if prev == 0 {
        eprintln!(
            "caffeinate: SetThreadExecutionState failed: {}",
            io::Error::last_os_error()
        );
        process::exit(1);
    }
    if flags & ES_DISPLAY_REQUIRED != 0 {
        // A one-shot (non-continuous) call resets the display idle timer
        // immediately, which is the closest Windows has to macOS -u.
        // SAFETY: as above.
        unsafe { SetThreadExecutionState(ES_DISPLAY_REQUIRED) };
    }
}

fn release_awake() {
    // SAFETY: plain Win32 call with no pointers.
    unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
}

/// Block until `handle` is signalled. If `timeout` elapses first, release the
/// assertion and keep waiting so the caller still observes the exit.
fn hold_until_signalled(handle: HANDLE, timeout: Option<u64>) -> io::Result<()> {
    // SAFETY: caller guarantees `handle` is a valid, open process handle.
    let rc = unsafe { WaitForSingleObject(handle, timeout_millis(timeout)) };
    match rc {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => {
            release_awake();
            // SAFETY: as above.
            match unsafe { WaitForSingleObject(handle, INFINITE) } {
                WAIT_OBJECT_0 => Ok(()),
                _ => Err(io::Error::last_os_error()),
            }
        }
        _ => Err(io::Error::last_os_error()),
    }
}

fn wait_for_pid(pid: u32, timeout: Option<u64>) -> i32 {
    // SAFETY: handle is null-checked and closed; no memory is shared.
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            eprintln!(
                "caffeinate: cannot open process {pid}: {}",
                io::Error::last_os_error()
            );
            return 1;
        }
        let result = hold_until_signalled(handle, timeout);
        CloseHandle(handle);
        match result {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("caffeinate: wait on process {pid} failed: {e}");
                1
            }
        }
    }
}

fn run_command(argv: &[String], timeout: Option<u64>) -> i32 {
    let (program, args) = argv.split_first().expect("non-empty command");
    let mut child = match Command::new(program).args(args).spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!("caffeinate: cannot run '{program}': {e}");
            return 127;
        }
    };
    if let Err(e) = hold_until_signalled(child.as_raw_handle() as HANDLE, timeout) {
        eprintln!("caffeinate: wait on '{program}' failed: {e}");
    }
    match child.wait() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("caffeinate: wait on '{program}' failed: {e}");
            1
        }
    }
}

/// Re-run the published installer. It verifies the SHA256 and replaces the
/// running exe safely, so delegating to it keeps one source of truth.
fn self_update() -> i32 {
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &format!("irm {INSTALL_URL} | iex"),
        ])
        .status();
    match status {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("caffeinate: cannot start powershell: {e}");
            1
        }
    }
}

fn main() {
    let opts = match parse_args(env::args().skip(1)) {
        Ok(o) => o,
        Err(e @ (ParseError::Help | ParseError::Version)) => {
            println!("{e}");
            return;
        }
        Err(ParseError::Update) => process::exit(self_update()),
        Err(e) => {
            eprintln!("{e}");
            eprintln!("{USAGE}");
            process::exit(2);
        }
    };

    assert_awake(execution_state(&opts));

    let code = if !opts.command.is_empty() {
        run_command(&opts.command, opts.timeout)
    } else if let Some(pid) = opts.wait_pid {
        wait_for_pid(pid, opts.timeout)
    } else if let Some(secs) = opts.timeout {
        thread::sleep(Duration::from_secs(secs));
        0
    } else {
        // Hold forever. Ctrl+C or closing the console ends the process and
        // the OS drops the assertion with it.
        loop {
            thread::sleep(Duration::from_secs(3600));
        }
    };

    release_awake();
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Options, ParseError> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_args_defaults_to_system() {
        let o = p(&[]).unwrap();
        assert!(o.system);
        assert!(!o.display);
        assert_eq!(o.timeout, None);
        assert!(o.command.is_empty());
    }

    #[test]
    fn combined_flags_and_timeout() {
        let o = p(&["-di", "-t", "30"]).unwrap();
        assert!(o.display && o.system);
        assert_eq!(o.timeout, Some(30));
    }

    #[test]
    fn attached_value() {
        let o = p(&["-t30"]).unwrap();
        assert_eq!(o.timeout, Some(30));
        let o = p(&["-w1234"]).unwrap();
        assert_eq!(o.wait_pid, Some(1234));
    }

    #[test]
    fn m_and_s_map_to_system() {
        assert!(p(&["-m"]).unwrap().system);
        assert!(p(&["-s"]).unwrap().system);
    }

    #[test]
    fn user_active_defaults_five_seconds_only_when_alone() {
        let o = p(&["-u"]).unwrap();
        assert!(o.user_active);
        assert_eq!(o.timeout, Some(5));
        assert_eq!(p(&["-u", "-t", "9"]).unwrap().timeout, Some(9));
        assert_eq!(p(&["-u", "-w", "1"]).unwrap().timeout, None);
        assert_eq!(p(&["-u", "cmd"]).unwrap().timeout, None);
    }

    #[test]
    fn command_takes_rest() {
        let o = p(&["-d", "ping", "-n", "3", "localhost"]).unwrap();
        assert!(o.display);
        assert_eq!(o.command, vec!["ping", "-n", "3", "localhost"]);
    }

    #[test]
    fn timeout_with_command_is_kept() {
        let o = p(&["-t", "5", "cargo", "build"]).unwrap();
        assert_eq!(o.timeout, Some(5));
        assert_eq!(o.command, vec!["cargo", "build"]);
    }

    #[test]
    fn double_dash_separates_command() {
        let o = p(&["-i", "--", "-weird", "arg"]).unwrap();
        assert_eq!(o.command, vec!["-weird", "arg"]);
        let o = p(&["-t", "3", "--", "x"]).unwrap();
        assert_eq!(o.timeout, Some(3));
        assert_eq!(o.command, vec!["x"]);
    }

    #[test]
    fn lone_dash_is_a_command() {
        assert_eq!(p(&["-"]).unwrap().command, vec!["-"]);
    }

    #[test]
    fn errors() {
        assert_eq!(p(&["-x"]), Err(ParseError::UnknownFlag("-x".into())));
        assert_eq!(
            p(&["--bogus"]),
            Err(ParseError::UnknownFlag("--bogus".into()))
        );
        assert_eq!(p(&["-t"]), Err(ParseError::MissingValue('t')));
        assert_eq!(
            p(&["-t", "abc"]),
            Err(ParseError::BadValue('t', "abc".into()))
        );
        assert_eq!(
            p(&["-w", "99999999999"]),
            Err(ParseError::BadValue('w', "99999999999".into()))
        );
        assert!(matches!(
            p(&["-w", "1", "cmd"]),
            Err(ParseError::Conflict(_))
        ));
        assert_eq!(p(&["-h"]), Err(ParseError::Help));
        assert_eq!(p(&["--help"]), Err(ParseError::Help));
        assert_eq!(p(&["-V"]), Err(ParseError::Version));
        assert_eq!(p(&["--version"]), Err(ParseError::Version));
        assert_eq!(p(&["--update"]), Err(ParseError::Update));
    }

    #[test]
    fn execution_state_flags() {
        let o = p(&["-d"]).unwrap();
        assert_eq!(execution_state(&o), ES_CONTINUOUS | ES_DISPLAY_REQUIRED);
        let o = p(&["-di"]).unwrap();
        assert_eq!(
            execution_state(&o),
            ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED
        );
        let o = p(&[]).unwrap();
        assert_eq!(execution_state(&o), ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
    }

    #[test]
    fn timeout_conversion() {
        assert_eq!(timeout_millis(None), INFINITE);
        assert_eq!(timeout_millis(Some(2)), 2000);
        assert_eq!(timeout_millis(Some(u64::MAX)), u32::MAX - 1);
    }
}
