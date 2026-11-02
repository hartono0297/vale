pub mod hook;
pub mod host;
pub mod install;
pub mod origin;
pub mod pipe_server;

/// Local-only pipe the island listens on and `--hook` writes one JSON line to.
const PIPE_NAME: &str = r"\\.\pipe\vale";
