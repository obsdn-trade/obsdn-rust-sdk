//! Wire types for the REST and WebSocket JSON surface.
//!
//! These are generated from the API schema and committed to the repo, so
//! building the SDK needs no code-generation toolchain. The consumer-facing
//! path is [`v1`]; hand-written ergonomic accessors (e.g.
//! [`v1::Market::mark_price`]) live alongside the generated structs.

#[allow(missing_docs)]
mod generated {
    #[allow(missing_docs, rustdoc::invalid_html_tags)]
    pub mod v1 {
        // Message + enum types and their JSON (de)serializers. Auto-generated;
        // field-level docs are carried over from the schema. `missing_docs` is
        // suppressed at the module boundary since we don't control the output.
        include!("generated/nil.v1.rs");
        include!("generated/nil.v1.serde.rs");
    }
}

pub use generated::v1;

impl v1::Market {
    /// Mark price as `f64`, or `None` if the wire value can't be parsed.
    pub fn mark_price(&self) -> Option<f64> {
        self.mark_px.parse().ok()
    }

    /// Index (oracle) price as `f64`.
    pub fn index_price(&self) -> Option<f64> {
        self.idx_px.parse().ok()
    }

    /// Last traded price as `f64`.
    pub fn last_price(&self) -> Option<f64> {
        self.last_px.parse().ok()
    }

    /// Minimum order size as `f64`.
    pub fn min_size(&self) -> Option<f64> {
        self.min_sz.parse().ok()
    }

    /// Base-asset size increment as `f64`.
    pub fn base_increment(&self) -> Option<f64> {
        self.base_incr.parse().ok()
    }

    /// Price increment as `f64`.
    pub fn price_increment(&self) -> Option<f64> {
        self.price_incr.parse().ok()
    }

    /// Maximum leverage as `f64`.
    pub fn max_leverage(&self) -> Option<f64> {
        self.max_lev.parse().ok()
    }
}
