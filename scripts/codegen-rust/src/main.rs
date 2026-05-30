//! Standalone codegen for `obsdn-sdk` wire types.
//!
//! Replaces the previous `build.rs`. Run on demand (not on every
//! `cargo build`) so the published crate ships pre-generated `.rs` files
//! and downstream users don't need `buf` installed.
//!
//! Pipeline mirrors the old `build.rs`:
//!   1. `buf export` flattens `api/proto/nil/v1/*.proto` (+ deps) into a
//!      temp dir.
//!   2. `prost-build` compiles the SDK's input set (excluding admin) and
//!      emits a descriptor file.
//!   3. `pbjson-build` walks the descriptors and emits jsonpb-compatible
//!      `Serialize`/`Deserialize` impls.
//!   4. Output written to `<crate>/src/types/generated/{nil.v1.rs, nil.v1.serde.rs}`.
//!
//! Usage:
//!   cargo run --manifest-path sdk/rust/scripts/codegen-rust/Cargo.toml -- \
//!     --proto-dir <path-to-api/proto> \
//!     --out-dir   <path-to-sdk/rust/src/types/generated>
//!
//! Defaults assume the standard backend monorepo layout:
//!   proto-dir = ../../../../api/proto    (relative to scripts/codegen-rust)
//!   out-dir   = ../../src/types/generated

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let mut proto_dir: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--proto-dir" => proto_dir = args.next().map(PathBuf::from),
            "--out-dir" => out_dir = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!(
                    "codegen-rust --proto-dir <api/proto> --out-dir <src/types/generated>"
                );
                return;
            }
            other => {
                eprintln!("unknown arg: {other}");
                std::process::exit(2);
            }
        }
    }

    // Default to the in-monorepo layout. Both paths are resolved relative
    // to the codegen crate's manifest dir so `cargo run` works regardless
    // of the user's CWD.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let proto_dir = proto_dir
        .unwrap_or_else(|| manifest.join("../../../../api/proto"))
        .canonicalize()
        .expect("proto-dir not found (pass --proto-dir <path>)");
    let out_dir = out_dir.unwrap_or_else(|| manifest.join("../../src/types/generated"));

    eprintln!("proto-dir: {}", proto_dir.display());
    eprintln!("out-dir:   {}", out_dir.display());

    std::fs::create_dir_all(&out_dir).expect("create out-dir");
    let out_dir = out_dir.canonicalize().expect("canonicalize out-dir");

    // 1. `buf export` flattens the proto module + transitive deps into a
    //    scratch dir. Use a sibling tempdir under out-dir's parent so we
    //    don't pollute the workspace.
    let export_dir = std::env::temp_dir().join("obsdn-sdk-codegen-protos");
    if export_dir.exists() {
        std::fs::remove_dir_all(&export_dir).expect("clean export dir");
    }
    std::fs::create_dir_all(&export_dir).expect("create export dir");

    let status = Command::new("buf")
        .arg("export")
        .arg(&proto_dir)
        .arg("-o")
        .arg(&export_dir)
        .status()
        .expect("`buf export` failed - install buf and run `make codegen` once first");
    assert!(status.success(), "buf export exited non-zero");

    // 2. Discover compilable .proto files under nil/v1, excluding RPC
    //    surfaces that are out of scope for the public SDK (no handlers
    //    wrap them, and they carry private/internal endpoints). Excluding
    //    the whole file is correct because the SDK exposes none of their
    //    messages.
    const EXCLUDED_PROTOS: &[&str] = &["admin.proto", "referral.proto", "whitelist.proto"];
    let nil_v1_dir = export_dir.join("nil/v1");
    let proto_files: Vec<PathBuf> = std::fs::read_dir(&nil_v1_dir)
        .expect("read nil/v1 dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "proto"))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| !EXCLUDED_PROTOS.contains(&n))
                .unwrap_or(false)
        })
        .collect();
    assert!(!proto_files.is_empty(), "no proto files under nil/v1/");

    // 3. prost-build emits message + enum types, plus a descriptor set
    //    file pbjson reads in step 4.
    let descriptor_path = out_dir.join("descriptors.bin");
    let mut cfg = prost_build::Config::new();
    cfg.out_dir(&out_dir);
    cfg.file_descriptor_set_path(&descriptor_path);

    // Hide proto fields tagged `(google.api.field_visibility).restriction
    // = "INTERNAL"` from public crate docs. The fields stay `pub` but
    // `cargo doc` ignores them. Keep this list in sync with:
    //   grep field_visibility api/proto/nil/v1/*.proto
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

    cfg.compile_protos(&proto_files, &[&export_dir])
        .expect("prost compile failed");

    // 4. pbjson serde derives, gated to nil.v1 + nested types.
    pbjson_build::Builder::new()
        .out_dir(&out_dir)
        .register_descriptors(&std::fs::read(&descriptor_path).expect("read descriptors"))
        .expect("register descriptors")
        .build(&[".nil.v1"])
        .expect("pbjson build failed");

    // descriptors.bin is an internal artifact - drop it so the committed
    // tree stays minimal. Keep only the human-readable .rs sources.
    let _ = std::fs::remove_file(&descriptor_path);

    // Sanity-check expected outputs exist.
    for f in ["nil.v1.rs", "nil.v1.serde.rs"] {
        let p = out_dir.join(f);
        assert!(p.exists(), "expected output missing: {}", p.display());
    }

    eprintln!("ok - wrote nil.v1.rs + nil.v1.serde.rs");
    eprintln!("commit the result under {}", out_dir.display());

    let _ = Path::new(""); // silence unused import on some toolchains
}
