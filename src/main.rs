//! caffeinate for Windows.
//!
//! Mirrors the macOS `caffeinate(8)` command-line interface on top of the
//! Win32 `SetThreadExecutionState` API. While the process runs, the
//! requested assertions are held; they are released automatically when the
//! process exits (including Ctrl+C), so no cleanup handler is required.

use std::env;
use std::fmt;
use std::process::{self, Command};
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Power::{
    SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
    EXECUTION_STATE,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
};

const USAGE: &str = "\
usage: caffeinate [-dimsu] [-t timeout] [-w pid] [utility arguments...]

Prevent the system from sleeping while the command runs.

  -d          prevent the display from sleeping
  -i          prevent the system from idle sleeping
  -m          prevent the disk from idle sleeping (same as -i on Windows)
  -s          prevent the system from sleeping while on AC power (same as -i)
  -u          declare the user is active: wakes the display, then holds the
              assertion for -t seconds (default 5)
  -t timeout  release the assertion after `timeout` seconds
  -w pid      release the assertion when process `pid` exits
  -h, --help  show this help

With no flags, -i is assumed. If a utility is given, it is run with the
remaining arguments and the assertion is held until it exits; caffeinate
returns the utility's exit code.
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
    UnknownFlag(String),
    MissingValue(char),
    BadValue(char, String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Help => write!(f, "{USAGE}"),
            ParseError::UnknownFlag(s) => write!(f, "caffeinate: unknown option '{s}'"),
            ParseError::MissingValue(c) => write!(f, "caffeinate: option -{c} requires a value"),
            ParseError::BadValue(c, v) => write!(f, "caffeinate: invalid value for -{c}: '{v}'"),
        }
    }
}

fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Options, ParseError> {
    let mut opts = Options::default();
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        if arg == "--" {
            opts.command.extend(args);
            break;
        }
        if arg == "-h" || arg == "--help" {
            return Err(ParseError::Help);
        }
        if !arg.starts_with('-') || arg.len() < 2 {
            opts.command.push(arg);
            opts.command.extend(args);
            break;
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

fn assert_awake(flags: EXECUTION_STATE) {
    // SAFETY: plain Win32 call with no pointers.
    let prev = unsafe { SetThreadExecutionState(flags) };
    if prev == 0 {
        eprintln!("caffeinate: SetThreadExecutionState failed");
        process::exit(1);
    }
    if flags & ES_DISPLAY_REQUIRED != 0 {
        // A non-continuous pulse resets the display idle timer right now,
        // which is what "user is active" (-u) means on macOS.
        // SAFETY: as above.
        unsafe { SetThreadExecutionState(ES_DISPLAY_REQUIRED) };
    }
}

fn wait_for_pid(pid: u32) -> i32 {
    // SAFETY: handle is null-checked and closed; no memory is shared.
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            eprintln!("caffeinate: cannot open process {pid} (not found or access denied)");
            return 1;
        }
        let rc = WaitForSingleObject(handle, INFINITE);
        CloseHandle(handle);
        match rc {
            WAIT_OBJECT_0 => 0,
            WAIT_FAILED | WAIT_TIMEOUT => {
                eprintln!("caffeinate: wait on process {pid} failed");
                1
            }
            _ => 0,
        }
    }
}

fn run_command(argv: &[String]) -> i32 {
    let (program, args) = argv.split_first().expect("non-empty command");
    match Command::new(program).args(args).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("caffeinate: cannot run '{program}': {e}");
            127
        }
    }
}

fn main() {
    let opts = match parse_args(env::args().skip(1)) {
        Ok(o) => o,
        Err(ParseError::Help) => {
            print!("{USAGE}");
            return;
        }
        Err(e) => {
            eprintln!("{e}");
            eprintln!("{USAGE}");
            process::exit(2);
        }
    };

    assert_awake(execution_state(&opts));

    let code = if !opts.command.is_empty() {
        run_command(&opts.command)
    } else if let Some(pid) = opts.wait_pid {
        wait_for_pid(pid)
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

    // Explicit release before exit; harmless since the process is about to die.
    // SAFETY: plain Win32 call with no pointers.
    unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
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
    fn user_active_defaults_five_seconds() {
        let o = p(&["-u"]).unwrap();
        assert!(o.user_active);
        assert_eq!(o.timeout, Some(5));
        let o = p(&["-u", "-t", "9"]).unwrap();
        assert_eq!(o.timeout, Some(9));
    }

    #[test]
    fn command_takes_rest() {
        let o = p(&["-d", "ping", "-n", "3", "localhost"]).unwrap();
        assert!(o.display);
        assert_eq!(o.command, vec!["ping", "-n", "3", "localhost"]);
    }

    #[test]
    fn double_dash_separates_command() {
        let o = p(&["-i", "--", "-weird", "arg"]).unwrap();
        assert_eq!(o.command, vec!["-weird", "arg"]);
    }

    #[test]
    fn errors() {
        assert_eq!(p(&["-x"]), Err(ParseError::UnknownFlag("-x".into())));
        assert_eq!(p(&["-t"]), Err(ParseError::MissingValue('t')));
        assert_eq!(
            p(&["-t", "abc"]),
            Err(ParseError::BadValue('t', "abc".into()))
        );
        assert_eq!(p(&["-h"]), Err(ParseError::Help));
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
}
