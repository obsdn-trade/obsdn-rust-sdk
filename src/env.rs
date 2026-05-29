//! Environment selection - REST + WebSocket endpoints per env.
//!
//! Internal Twingate-gated hosts are NOT exposed here - those require a
//! private network. SDK consumers should pass an explicit [`Env::Custom`]
//! base URL when targeting `*.int.obsdn.trade`.

/// Target environment for [`crate::Client`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Env {
    /// Staging public: `https://nova.staging.obsdn.trade`.
    Staging,
    /// Production public: `https://api.obsdn.trade`.
    Production,
    /// Caller-supplied REST + WS base URLs - for Twingate-gated internal
    /// hosts, a forked staging stack, or a locally-run backend. The caller
    /// is responsible for pairing this with the matching EIP-712 domain via
    /// [`crate::ClientBuilder::eip712_domain`].
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
            Env::Staging => "https://nova.staging.obsdn.trade",
            Env::Production => "https://api.obsdn.trade",
            Env::Custom { rest, .. } => rest.as_str(),
        }
    }

    /// WebSocket URL including `/ws` path.
    pub fn ws_url(&self) -> &str {
        match self {
            Env::Staging => "wss://pulse.staging.obsdn.trade/ws",
            Env::Production => "wss://pulse.obsdn.trade/ws",
            Env::Custom { ws, .. } => ws.as_str(),
        }
    }
}
