//! Windows named pipes with a DACL that admits only the current user.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::pin::Pin;
use std::ptr;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, HANDLE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::{Error, Result};

/// Prefix of every local pipe name.
pub const PIPE_PREFIX: &str = r"\\.\pipe\";

/// SDDL revision understood by `ConvertStringSecurityDescriptorToSecurityDescriptorW`.
const SDDL_REVISION_1: u32 = 1;

/// How many times [`connect`] retries when every pipe instance is busy (50 ms apart).
const BUSY_RETRIES: u32 = 40;

/// The app's endpoint for this user: `\\.\pipe\riptide.<user-sid>`.
pub fn default_endpoint() -> Result<String> {
    Ok(format!("{PIPE_PREFIX}riptide.{}", current_user_sid()?))
}

/// A unique endpoint for tests: `\\.\pipe\riptide-test.<pid>.<tag>`.
pub fn test_endpoint(tag: &str) -> String {
    format!("{PIPE_PREFIX}riptide-test.{}.{tag}", std::process::id())
}

/// Server side: creates pipe instances on demand.
#[derive(Debug)]
pub struct Listener {
    endpoint: String,
    next: Option<NamedPipeServer>,
}

impl Listener {
    pub(crate) fn bind(endpoint: &str) -> Result<Listener> {
        let first = create_instance(endpoint, true)?;
        Ok(Listener {
            endpoint: endpoint.to_string(),
            next: Some(first),
        })
    }

    pub(crate) fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub(crate) async fn accept(&mut self) -> Result<Stream> {
        let server = match self.next.take() {
            Some(s) => s,
            None => create_instance(&self.endpoint, false)?,
        };
        server.connect().await?;
        // Prepare the next instance right away so a second client never sees "not found".
        // If this fails we retry on the next accept instead of failing this one.
        self.next = create_instance(&self.endpoint, false)
            .map_err(|e| tracing::warn!(error = %e, "cannot pre-create next pipe instance"))
            .ok();
        Ok(Stream::Server(server))
    }
}

fn create_instance(endpoint: &str, first: bool) -> Result<NamedPipeServer> {
    let descriptor = SecurityDescriptor::current_user_only()?;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.ptr,
        bInheritHandle: 0,
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true);
    // SAFETY: `attributes` is a valid SECURITY_ATTRIBUTES whose descriptor outlives the call
    // (`descriptor` is dropped after `create_with_security_attributes_raw` returns). The
    // descriptor is only read by CreateNamedPipeW.
    let server = unsafe {
        options.create_with_security_attributes_raw(
            endpoint,
            (&mut attributes as *mut SECURITY_ATTRIBUTES).cast::<c_void>(),
        )
    }?;
    Ok(server)
}

/// Client side: opens the pipe, waiting briefly while all instances are busy.
pub(crate) async fn connect(endpoint: &str) -> Result<Stream> {
    for _ in 0..BUSY_RETRIES {
        match ClientOptions::new().open(endpoint) {
            Ok(client) => return Ok(Stream::Client(client)),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) => {
                return Err(Error::NotRunning(endpoint.to_string()));
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(Error::Busy)
}

/// Either end of a pipe.
#[derive(Debug)]
pub enum Stream {
    /// Accepted by a [`Listener`].
    Server(NamedPipeServer),
    /// Opened by [`connect`].
    Client(NamedPipeClient),
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Server(s) => Pin::new(s).poll_read(cx, buf),
            Stream::Client(c) => Pin::new(c).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Stream::Server(s) => Pin::new(s).poll_write(cx, data),
            Stream::Client(c) => Pin::new(c).poll_write(cx, data),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Server(s) => Pin::new(s).poll_flush(cx),
            Stream::Client(c) => Pin::new(c).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Server(s) => Pin::new(s).poll_shutdown(cx),
            Stream::Client(c) => Pin::new(c).poll_shutdown(cx),
        }
    }
}

// ---------------------------------------------------------------------- security

fn last_error(what: &'static str) -> Error {
    // SAFETY: GetLastError has no preconditions.
    Error::Platform {
        what,
        code: unsafe { GetLastError() },
    }
}

struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcessToken and is closed exactly once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// String SID (`S-1-5-21-…`) of the user running this process.
pub fn current_user_sid() -> Result<String> {
    // SAFETY: plain Win32 token query. Every pointer passed is valid for the stated length and the
    // buffer is 8-byte aligned (Vec<u64>) as TOKEN_USER requires.
    unsafe {
        let mut token: HANDLE = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(last_error("OpenProcessToken"));
        }
        let _token = HandleGuard(token);

        let mut len: u32 = 0;
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut len);
        if len == 0 {
            return Err(last_error("GetTokenInformation(size)"));
        }
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        if GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) == 0 {
            return Err(last_error("GetTokenInformation"));
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);

        let mut sid_wide: *mut u16 = ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid_wide) == 0 {
            return Err(last_error("ConvertSidToStringSidW"));
        }
        let sid = wide_to_string(sid_wide);
        LocalFree(sid_wide.cast());
        Ok(sid)
    }
}

/// Reads a NUL-terminated UTF-16 string.
unsafe fn wide_to_string(p: *const u16) -> String {
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
}

/// An owned SECURITY_DESCRIPTOR built from SDDL, freed on drop.
struct SecurityDescriptor {
    ptr: PSECURITY_DESCRIPTOR,
}

impl SecurityDescriptor {
    /// `D:P(A;;GA;;;<sid>)` — protected DACL, one ACE: full access for the current user, nobody
    /// else (not even administrators, who would have to take ownership first).
    fn current_user_only() -> Result<SecurityDescriptor> {
        Self::from_sddl(&sddl_for_sid(&current_user_sid()?))
    }

    fn from_sddl(sddl: &str) -> Result<SecurityDescriptor> {
        let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let mut ptr: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated and outlives the call; `ptr` receives a LocalAlloc'd
        // descriptor that we free in Drop.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut ptr,
                ptr::null_mut(),
            )
        };
        if ok == 0 || ptr.is_null() {
            return Err(last_error(
                "ConvertStringSecurityDescriptorToSecurityDescriptorW",
            ));
        }
        Ok(SecurityDescriptor { ptr })
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: the pointer came from ConvertStringSecurityDescriptorToSecurityDescriptorW,
        // which documents LocalFree as the release function.
        unsafe {
            LocalFree(self.ptr);
        }
    }
}

/// The SDDL string used for the pipe DACL. Exposed for tests and `docs/ipc-protocol.md`.
pub fn sddl_for_sid(sid: &str) -> String {
    format!("D:P(A;;GA;;;{sid})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sid_and_endpoint_look_right() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "{sid}");
        let ep = default_endpoint().unwrap();
        assert_eq!(ep, format!(r"\\.\pipe\riptide.{sid}"));
        assert!(test_endpoint("x").starts_with(r"\\.\pipe\riptide-test."));
    }

    #[test]
    fn security_descriptor_builds_from_sddl() {
        let sd = SecurityDescriptor::current_user_only().unwrap();
        assert!(!sd.ptr.is_null());
        assert!(SecurityDescriptor::from_sddl("this is not sddl").is_err());
    }

    #[tokio::test]
    async fn connecting_to_a_missing_pipe_reports_not_running() {
        let err = connect(&test_endpoint("does-not-exist")).await.unwrap_err();
        assert!(matches!(err, Error::NotRunning(_)), "{err}");
    }
}
