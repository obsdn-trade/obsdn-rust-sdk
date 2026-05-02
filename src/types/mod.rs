//! Wire types generated from `api/proto/nil/v1/*.proto`.
//!
//! Structs are produced by `prost` (protobuf decode/encode) and
//! `Serialize`/`Deserialize` impls by `pbjson` (jsonpb-compatible JSON,
//! matching the gRPC-gateway REST surface).

#[allow(missing_docs)]
pub mod nil {
    #[allow(missing_docs)]
    pub mod v1 {
        // prost-generated message + enum types. Auto-generated; field-level
        // doc comments come from the .proto sources via prost — `missing_docs`
        // is suppressed at the module boundary because we don't control
        // codegen output.
        include!(concat!(env!("OUT_DIR"), "/nil.v1.rs"));
        // pbjson-generated serde impls.
        include!(concat!(env!("OUT_DIR"), "/nil.v1.serde.rs"));
    }
}

pub use nil::v1;
