//! Wire types generated from `api/proto/nil/v1/*.proto`.
//!
//! Structs are produced by `prost` (protobuf decode/encode) and
//! `Serialize`/`Deserialize` impls by `pbjson` (jsonpb-compatible JSON,
//! matching the gRPC-gateway REST surface).
//!
//! Files under `generated/` are committed and regenerated on demand via
//! `make sdk.rust.codegen` (the codegen binary lives at
//! `scripts/codegen-rust/`). They are not produced by `cargo build`, so
//! downstream users don't need `buf` or `protoc` installed.

#[allow(missing_docs)]
pub mod nil {
    #[allow(missing_docs, rustdoc::invalid_html_tags)]
    pub mod v1 {
        // prost-generated message + enum types. Auto-generated; field-level
        // doc comments come from the .proto sources via prost - `missing_docs`
        // is suppressed at the module boundary because we don't control
        // codegen output.
        include!("generated/nil.v1.rs");
        // pbjson-generated serde impls.
        include!("generated/nil.v1.serde.rs");
    }
}

pub use nil::v1;
