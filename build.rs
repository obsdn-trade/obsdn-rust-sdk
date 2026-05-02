//! Build script: generate Rust types from `api/proto/nil/v1/*.proto`.
//!
//! Pipeline:
//!   1. `buf export` flattens the proto module + transitive deps (googleapis,
//!      grpc-gateway openapiv2 annotations) from the local buf cache into
//!      `$OUT_DIR/protos/`. Uses local cache only — no network at build time
//!      provided buf has been run at least once on the host (CI installs buf
//!      and runs `buf mod update` once).
//!   2. `prost-build` compiles only the Rust SDK's input set (currently
//!      `nil/v1/*.proto` minus admin) to Rust structs, with `pbjson-build`
//!      emitting jsonpb-compatible serde derives (enums as strings, base64
//!      bytes, RFC3339 timestamps, camelCase fields).
//!   3. Annotation protos (google.api.*, grpc.gateway.*) are referenced by
//!      `option`s on services/methods but never appear in `nil.v1` message
//!      fields. prost-build still compiles them when imported — that's a
//!      few extra generated structs we never `include!`. Acceptable cost
//!      vs. wiring `extern_path` overrides for every annotation namespace.
//!   4. RPC `service` definitions are emitted by prost but not exposed —
//!      the REST client is hand-written on top of the message types in
//!      Phase 2+. Tonic is not used.
//!   5. `admin.proto` is intentionally excluded — admin RPCs are out of
//!      scope for the public SDK per the brainstorm report.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Tell cargo to re-run when proto sources OR transitive dep pins change.
    // `buf.lock` controls which versions of googleapis / grpc-gateway are
    // exported, so a lock bump must invalidate cached generated code even
    // when no `.proto` file content changed.
    println!("cargo:rerun-if-changed=../../api/proto");
    println!("cargo:rerun-if-changed=../../api/proto/buf.lock");
    println!("cargo:rerun-if-changed=../../api/proto/buf.yaml");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let proto_export_dir = out_dir.join("protos");

    // 1. Export flattened proto tree via buf.
    //    Source: `../../api/proto` relative to the crate root.
    //    Buf resolves transitive deps from the local cache.
    let proto_src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../api/proto")
        .canonicalize()
        .expect("api/proto directory not found");

    if proto_export_dir.exists() {
        std::fs::remove_dir_all(&proto_export_dir).expect("clean previous export dir");
    }
    std::fs::create_dir_all(&proto_export_dir).expect("create export dir");

    let status = Command::new("buf")
        .arg("export")
        .arg(&proto_src)
        .arg("-o")
        .arg(&proto_export_dir)
        .status()
        .expect("failed to execute `buf export` — install buf or run `make codegen` once first");
    assert!(status.success(), "buf export failed");

    // 2. Discover the nil/v1 proto files we want to compile.
    //    Excludes:
    //      - admin.proto: admin RPCs are out of scope for the public SDK,
    //        and the messages they define would expose internal-only types.
    let nil_v1_dir = proto_export_dir.join("nil/v1");
    let proto_files: Vec<PathBuf> = std::fs::read_dir(&nil_v1_dir)
        .expect("read nil/v1 dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "proto").unwrap_or(false))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n != "admin.proto")
                .unwrap_or(false)
        })
        .collect();
    assert!(
        !proto_files.is_empty(),
        "no proto files found under nil/v1/"
    );

    // pbjson-build needs a sibling JSON descriptor file emitted by prost.
    let descriptor_path = out_dir.join("descriptors.bin");

    // 3. Configure prost: emit serde-friendly defaults; pbjson-build adds
    //    the actual Serialize/Deserialize impls. We do NOT add `#[derive(serde::Serialize, Deserialize)]`
    //    here — pbjson generates external impls so we can keep jsonpb fidelity.
    let mut cfg = prost_build::Config::new();
    cfg.file_descriptor_set_path(&descriptor_path);
    // Use HashMap (prost default) — pbjson's serde impls expect HashMap, not BTreeMap.

    // Hide proto fields tagged `(google.api.field_visibility).restriction = "INTERNAL"`
    // from the public crate docs. They're still emitted as `pub` (the gateway
    // round-trips them on internal endpoints we expose with `#[doc(hidden)]`)
    // but `cargo doc` ignores them and rust-analyzer demotes them in
    // completion. Keep this list in sync with `grep field_visibility
    // api/proto/nil/v1/*.proto`.
    for (msg, field) in [
        ("GetPortfolioResponse", "stats"),
        ("GetPortfolioResponse", "priv_nm"),
        ("GetPortfolioResponse", "is_mm"),
        ("GetPortfolioResponse", "dis_post_ord"),
        ("GetPortfolioResponse", "dis_wdraw"),
        ("Market", "tags"),
        ("Order", "trades"),
        ("Order", "sndr_nm"),
        ("RegisterSignerRequest", "eoa_only"),
    ] {
        cfg.field_attribute(format!("nil.v1.{msg}.{field}"), "#[doc(hidden)]");
    }

    cfg.compile_protos(&proto_files, &[&proto_export_dir])
        .expect("prost compile failed");

    // 4. pbjson serde derives, gated to nil.v1 + nested types.
    pbjson_build::Builder::new()
        .register_descriptors(&std::fs::read(&descriptor_path).expect("read descriptors"))
        .expect("register descriptors")
        .build(&[".nil.v1"])
        .expect("pbjson build failed");
}
