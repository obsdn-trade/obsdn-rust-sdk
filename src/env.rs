//! Environment selection — REST + WebSocket endpoints per env.
//!
//! Internal Twingate-gated hosts are NOT exposed here — those require a
//! private network. SDK consumers should pass an explicit [`Env::Custom`]
//! base URL when targeting `*.int.obsdn.trade`.

/// Target environment for [`crate::Client`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Env {
    /// Local dev: `http://localhost:8080` REST, `ws://localhost:8080/ws`.
    Local,
    /// Staging public: `https://api.staging.obsdn.trade`.
    Staging,
    /// Production public: `https://api.obsdn.trade`.
    Production,
    /// Caller-supplied REST + WS base URLs (e.g., Twingate-gated internal).
    Custom {
        /// REST base URL (e.g. `https://nova.stg.int.obsdn.trade`).
        rest: String,
        /// WebSocket URL (e.g. `wss://pulse.stg.int.obsdn.trade/ws`).
        ws: String,
    },
}

impl Env {
    /// REST base URL with no trailing slash. Endpoint paths are appended raw
    /// (e.g., `"/orders"`).
    pub fn rest_base_url(&self) -> &str {
        match self {
            Env::Local => "http://localhost:8080",
            Env::Staging => "https://nova.staging.obsdn.trade",
            Env::Production => "https://api.obsdn.trade",
            Env::Custom { rest, .. } => rest.as_str(),
        }
    }

    /// WebSocket URL including `/ws` path.
    pub fn ws_url(&self) -> &str {
        match self {
            Env::Local => "ws://localhost:8080/ws",
            Env::Staging => "wss://pulse.staging.obsdn.trade/ws",
            Env::Production => "wss://pulse.obsdn.trade/ws",
            Env::Custom { ws, .. } => ws.as_str(),
        }
    }
}
