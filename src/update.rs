//! Self-update without spawning anything.
//!
//! Downloads the published SHA256 for the latest release, compares it with the
//! running binary, and only then downloads and swaps in the new exe. Uses
//! WinHTTP for HTTPS (system proxy settings respected) and BCrypt for SHA256,
//! both from the OS, so there is no shell, no script, no extra dependency.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::ptr;

use windows_sys::Win32::Foundation::{GetLastError, WIN32_ERROR};
use windows_sys::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, INTERNET_DEFAULT_HTTPS_PORT,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE,
};
use windows_sys::Win32::Security::Cryptography::{
    BCryptCloseAlgorithmProvider, BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash,
    BCryptHashData, BCryptOpenAlgorithmProvider, BCRYPT_ALG_HANDLE, BCRYPT_HASH_HANDLE,
    BCRYPT_SHA256_ALGORITHM,
};

const HOST: &str = "github.com";
const RELEASE_PATH: &str = "/skhortiuk/caffeinate-windows/releases/latest/download";
const USER_AGENT: &str = concat!("caffeinate-windows/", env!("CARGO_PKG_VERSION"));

/// Entry point for `caffeinate --update`. Returns a process exit code.
pub fn run() -> i32 {
    match update() {
        Ok(Outcome::UpToDate(path)) => {
            println!(
                "caffeinate {} is already the latest version ({})",
                env!("CARGO_PKG_VERSION"),
                path.display()
            );
            0
        }
        Ok(Outcome::Updated(path)) => {
            println!("Updated {}", path.display());
            0
        }
        Err(e) => {
            eprintln!("caffeinate: update failed: {e}");
            1
        }
    }
}

enum Outcome {
    UpToDate(PathBuf),
    Updated(PathBuf),
}

fn update() -> io::Result<Outcome> {
    let exe = env::current_exe()?;
    let old = exe.with_extension("exe.old");
    let tmp = exe.with_extension("exe.tmp");
    // Leftover from a previous update whose old exe was still running.
    let _ = fs::remove_file(&old);

    let published = https_get(&format!("{RELEASE_PATH}/caffeinate.exe.sha256"))?;
    let expected = parse_sha256_line(&published)
        .ok_or_else(|| io::Error::other("published checksum file is malformed"))?;

    let current = sha256_hex(&fs::read(&exe)?)?;
    if current == expected {
        return Ok(Outcome::UpToDate(exe));
    }

    let body = https_get(&format!("{RELEASE_PATH}/caffeinate.exe"))?;
    let actual = sha256_hex(&body)?;
    if actual != expected {
        return Err(io::Error::other(format!(
            "SHA256 mismatch\n expected {expected}\n got      {actual}"
        )));
    }

    fs::write(&tmp, &body)?;
    swap_in(&exe, &old, &tmp)?;
    Ok(Outcome::Updated(exe))
}

/// A running exe cannot be overwritten, but it can be renamed. Move the
/// current one aside, move the new one in, and restore on failure.
fn swap_in(exe: &Path, old: &Path, tmp: &Path) -> io::Result<()> {
    fs::rename(exe, old)?;
    if let Err(e) = fs::rename(tmp, exe) {
        let _ = fs::rename(old, exe);
        let _ = fs::remove_file(tmp);
        return Err(e);
    }
    // Fails while the old exe is still running; cleaned up on the next update.
    let _ = fs::remove_file(old);
    Ok(())
}

pub fn parse_sha256_line(text: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(text).ok()?;
    let hash = text.split_whitespace().next()?;
    if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(hash.to_ascii_lowercase())
    } else {
        None
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error(what: &str) -> io::Error {
    // SAFETY: plain Win32 call.
    let code: WIN32_ERROR = unsafe { GetLastError() };
    io::Error::other(format!(
        "{what}: {}",
        io::Error::from_raw_os_error(code as i32)
    ))
}

struct HInternet(*mut core::ffi::c_void);

impl Drop for HInternet {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: handle came from WinHTTP and is closed exactly once.
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

/// GET `https://github.com{path}`, following redirects (GitHub release
/// downloads bounce to objects.githubusercontent.com). Returns the body.
fn https_get(path: &str) -> io::Result<Vec<u8>> {
    // SAFETY: every pointer passed is either null (documented as allowed) or
    // points at a NUL-terminated UTF-16 buffer / correctly sized out-param
    // that outlives the call. Handles are released by `HInternet`.
    unsafe {
        let session = HInternet(WinHttpOpen(
            wide(USER_AGENT).as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        ));
        if session.0.is_null() {
            return Err(last_error("WinHttpOpen"));
        }

        let connect = HInternet(WinHttpConnect(
            session.0,
            wide(HOST).as_ptr(),
            INTERNET_DEFAULT_HTTPS_PORT,
            0,
        ));
        if connect.0.is_null() {
            return Err(last_error("WinHttpConnect"));
        }

        let request = HInternet(WinHttpOpenRequest(
            connect.0,
            wide("GET").as_ptr(),
            wide(path).as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return Err(last_error("WinHttpOpenRequest"));
        }

        if WinHttpSendRequest(request.0, ptr::null(), 0, ptr::null(), 0, 0, 0) == 0 {
            return Err(last_error("WinHttpSendRequest"));
        }
        if WinHttpReceiveResponse(request.0, ptr::null_mut()) == 0 {
            return Err(last_error("WinHttpReceiveResponse"));
        }

        let mut status: u32 = 0;
        let mut status_len = std::mem::size_of::<u32>() as u32;
        if WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            &mut status as *mut u32 as *mut _,
            &mut status_len,
            ptr::null_mut(),
        ) == 0
        {
            return Err(last_error("WinHttpQueryHeaders"));
        }
        if status != 200 {
            return Err(io::Error::other(format!(
                "HTTP {status} for https://{HOST}{path}"
            )));
        }

        let mut body = Vec::new();
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let mut read: u32 = 0;
            if WinHttpReadData(
                request.0,
                chunk.as_mut_ptr() as *mut _,
                chunk.len() as u32,
                &mut read,
            ) == 0
            {
                return Err(last_error("WinHttpReadData"));
            }
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
        }
        Ok(body)
    }
}

pub fn sha256_hex(data: &[u8]) -> io::Result<String> {
    fn nt(status: i32, what: &str) -> io::Result<()> {
        if status >= 0 {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{what} failed (NTSTATUS {status:#x})"
            )))
        }
    }

    // SAFETY: handles are checked and released; buffers are sized to what the
    // API documents (32 bytes for SHA-256).
    unsafe {
        let mut alg: BCRYPT_ALG_HANDLE = ptr::null_mut();
        nt(
            BCryptOpenAlgorithmProvider(&mut alg, BCRYPT_SHA256_ALGORITHM, ptr::null(), 0),
            "BCryptOpenAlgorithmProvider",
        )?;

        let mut hash: BCRYPT_HASH_HANDLE = ptr::null_mut();
        let result: io::Result<[u8; 32]> = (|| {
            nt(
                BCryptCreateHash(alg, &mut hash, ptr::null_mut(), 0, ptr::null(), 0, 0),
                "BCryptCreateHash",
            )?;
            nt(
                BCryptHashData(hash, data.as_ptr(), data.len() as u32, 0),
                "BCryptHashData",
            )?;
            let mut out = [0u8; 32];
            nt(
                BCryptFinishHash(hash, out.as_mut_ptr(), out.len() as u32, 0),
                "BCryptFinishHash",
            )?;
            Ok(out)
        })();

        if !hash.is_null() {
            BCryptDestroyHash(hash);
        }
        BCryptCloseAlgorithmProvider(alg, 0);

        let out = result?;
        let mut hex = String::with_capacity(64);
        for b in out {
            use std::fmt::Write;
            let _ = write!(hex, "{b:02x}");
        }
        Ok(hex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            sha256_hex(b"").unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn parses_checksum_file() {
        let h = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(
            parse_sha256_line(format!("{h}  caffeinate.exe").as_bytes()).as_deref(),
            Some(h)
        );
        assert_eq!(
            parse_sha256_line(h.to_uppercase().as_bytes()).as_deref(),
            Some(h)
        );
        assert_eq!(parse_sha256_line(b"97"), None);
        assert_eq!(parse_sha256_line(b"nothex".repeat(11).as_slice()), None);
    }
}
