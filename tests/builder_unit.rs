//! Unit coverage for `ClientBuilder` configuration edges.

use alloy_primitives::address;
use obsdn_sdk::sign::custom_domain;
use obsdn_sdk::{Client, Env, Error};

fn custom_env() -> Env {
    Env::Custom {
        rest: "https://rest.example".into(),
        ws: "wss://ws.example/ws".into(),
    }
}

#[test]
fn default_env_is_production() {
    let client = Client::builder().build().expect("defaults build");
    // Production domain → Monad mainnet (chain 143).
    assert_eq!(client.eip712_domain().chain_id.unwrap().to_string(), "143");
}

#[test]
fn custom_env_without_domain_is_config_error() {
    let err = Client::builder()
        .env(custom_env())
        .build()
        .expect_err("Custom env needs an explicit eip712_domain");
    assert!(matches!(err, Error::Config(_)), "got {err:?}");
}

#[test]
fn custom_env_with_domain_builds() {
    let client = Client::builder()
        .env(custom_env())
        .eip712_domain(custom_domain(
            "Obsidian",
            "1",
            999,
            address!("0000000000000000000000000000000000000001"),
        ))
        .build()
        .expect("custom env + domain builds");
    assert_eq!(client.eip712_domain().chain_id.unwrap().to_string(), "999");
}

#[test]
fn eip712_domain_override_threads_through() {
    let client = Client::builder()
        .env(Env::Production)
        .eip712_domain(custom_domain(
            "Override",
            "2",
            7,
            address!("0000000000000000000000000000000000000002"),
        ))
        .build()
        .expect("build");
    let d = client.eip712_domain();
    assert_eq!(d.chain_id.unwrap().to_string(), "7");
    assert_eq!(d.version.as_deref(), Some("2"));
    assert_eq!(d.name.as_deref(), Some("Override"));
}
