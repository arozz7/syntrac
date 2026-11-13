//! Local IPC between the SYNTRAC collector service and its clients (the UI, `--status`).
//!
//! * `protocol`: the JSON-lines wire format (pure, tested).
//! * `serve`: serving one connection over any `Read + Write` (pure, tested).
//! * `sddl`: building the pipe's access list from validated SIDs (pure, tested).
//! * `pipe` (Windows): the named-pipe server and client, a thin layer over the above.

pub mod pipe;
pub mod protocol;
pub mod sddl;
pub mod serve;
