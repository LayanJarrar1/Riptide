//! Local IPC for Riptide: length-prefixed JSON over a named pipe (Windows) or a Unix socket.
//!
//! Used by the desktop app (server), the CLI and the Native Messaging host (clients). The wire
//! format is documented in `docs/ipc-protocol.md`:
//!
//! * one connection carries any number of [`Request`] / [`Response`] frames;
//! * a frame is a 4-byte little-endian length followed by that many bytes of UTF-8 JSON;
//! * a frame is at most [`MAX_FRAME_BYTES`];
//! * every request carries a caller-chosen `id` that the matching response echoes.
//!
//! The endpoint is `\\.\pipe\riptide.<user-sid>` on Windows (DACL: the current user only, remote
//! clients rejected) and `$XDG_RUNTIME_DIR/riptide.sock` (mode 0600) elsewhere. Nothing that
//! arrives over this channel is ever executed or used as a path; it is data for the engine.
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![deny(unsafe_code)]

mod error;
pub mod frame;
pub mod message;
mod server;
pub mod transport;

pub use error::Error;
pub use frame::{decode_frame, encode_frame, read_frame, write_frame, MAX_FRAME_BYTES};
pub use message::{
    ErrorCode, ProbeResult, Request, RequestBody, Response, ResponseBody, PROTOCOL_VERSION,
};
pub use server::{serve, serve_with_shutdown};
pub use transport::{connect, default_endpoint, test_endpoint, Connection, Listener};

/// `Result` with this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;
