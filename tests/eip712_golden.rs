//! Golden hash tests: Rust-side EIP-712 signing must produce byte-equal
//! struct hashes, digests, and signatures against the exchange's reference signer.
//!
//! Fixtures under `tests/fixtures/eip712/*.json` are captured from the
//! exchange's reference signer. Regenerate them when:
//!   - any template changes
//!   - the domain changes (chain id / contract)
//!   - a new sign function is added (also add a fixture)
//!
//! What we assert per fixture:
//! 1. `Eip712Domain::hash_struct() == fixture.domain_separator`
//! 2. `<sol struct>::eip712_hash_struct() == fixture.struct_hash`
//! 3. `<sol struct>::eip712_signing_hash(domain) == fixture.digest`
//! 4. The digest signed with the deterministic key `0x0101..0101` reproduces
//!    `fixture.signature` byte-for-byte (proves the v ∈ {27,28} normalization
//!    matches the gateway's ecrecover expectation).
//!
//! All four together prove that a Rust-signed REST request will be accepted
//! by the matching engine without a silent hash-mismatch.

use alloy_primitives::{Address, B256, U256};
use alloy_sol_types::{eip712_domain, Eip712Domain, SolStruct};
use obsdn_sdk::sign::{
    sign_create_subaccount, sign_create_vault, sign_delegated_signer, sign_order, sign_register,
    sign_register_child_account_signer, sign_stake_vault, sign_transfer, sign_unstake_vault,
    sign_withdraw, CreateSubaccountPayload, CreateVaultPayload, DelegatedSignerPayload,
    OrderPayload, OrderSide, RegisterChildAccountSignerPayload, RegisterPayload, StakeVaultPayload,
    TransferPayload, UnstakeVaultPayload, WithdrawPayload,
};
use obsdn_sdk::LocalSigner;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    template: String,
    domain: FixtureDomain,
    input: serde_json::Value,
    domain_separator: String,
    struct_hash: String,
    digest: String,
    private_key: String,
    signer_address: String,
    signature: String,
}

#[derive(Debug, Deserialize)]
struct FixtureDomain {
    name: String,
    version: String,
    chain_id: String,
    verifying_contract: String,
}

fn load(name: &str) -> Fixture {
    let path = format!(
        "{}/tests/fixtures/eip712/{}.json",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

fn parse_b256(hex: &str) -> B256 {
    let stripped = hex.strip_prefix("0x").unwrap();
    let bytes = ::hex::decode(stripped).expect("hex");
    B256::from_slice(&bytes)
}

fn parse_addr(hex: &str) -> Address {
    hex.parse().expect("address")
}

fn parse_sig(hex: &str) -> [u8; 65] {
    let stripped = hex.strip_prefix("0x").unwrap();
    let bytes = ::hex::decode(stripped).expect("hex");
    assert_eq!(bytes.len(), 65, "signature must be 65 bytes");
    let mut arr = [0u8; 65];
    arr.copy_from_slice(&bytes);
    arr
}

fn fixture_domain(d: &FixtureDomain) -> Eip712Domain {
    let chain_id: u64 = d.chain_id.parse().expect("chain_id u64");
    let contract = parse_addr(&d.verifying_contract);
    eip712_domain! {
        name: d.name.clone(),
        version: d.version.clone(),
        chain_id: chain_id,
        verifying_contract: contract,
    }
}

/// Common assertion path: domain separator + struct hash + signing hash.
fn assert_hashes<S: SolStruct>(f: &Fixture, sol_struct: &S, domain: &Eip712Domain) -> B256 {
    // 1. Domain separator
    let our_domain_sep = domain.hash_struct();
    assert_eq!(
        our_domain_sep,
        parse_b256(&f.domain_separator),
        "{}: domain separator mismatch",
        f.template
    );
    // 2. Struct hash (without domain)
    let our_struct_hash = sol_struct.eip712_hash_struct();
    assert_eq!(
        our_struct_hash,
        parse_b256(&f.struct_hash),
        "{}: struct hash mismatch",
        f.template
    );
    // 3. Signing hash (the value passed to ecrecover)
    let our_digest = sol_struct.eip712_signing_hash(domain);
    assert_eq!(
        our_digest,
        parse_b256(&f.digest),
        "{}: digest mismatch",
        f.template
    );
    our_digest
}

fn assert_signature_matches(f: &Fixture, digest: B256) {
    let signer = LocalSigner::from_hex(&f.private_key).expect("local signer");
    assert_eq!(
        format!("{:#x}", signer.inner().address()).to_lowercase(),
        f.signer_address.to_lowercase(),
        "{}: signer address mismatch",
        f.template
    );
    let sig: [u8; 65] = obsdn_sdk::Eip712Signer::sign_hash_sync(&signer, digest).expect("sign");
    let expected = parse_sig(&f.signature);
    assert_eq!(sig, expected, "{}: signature mismatch", f.template);
}

// --- one test per template ------------------------------------------------

#[test]
fn order_hash_matches_go() {
    let f = load("order");
    let domain = fixture_domain(&f.domain);
    let sender = parse_addr(f.input["sender"].as_str().unwrap());
    let market_index = f.input["market_index"].as_u64().unwrap() as u16;
    let side = match f.input["side"].as_str().unwrap() {
        "buy" => OrderSide::Buy,
        "sell" => OrderSide::Sell,
        other => panic!("unexpected side {other}"),
    };
    let size: u128 = f.input["size"].as_str().unwrap().parse().unwrap();
    let price: u128 = f.input["price"].as_str().unwrap().parse().unwrap();
    let nonce: u64 = f.input["nonce"].as_str().unwrap().parse().unwrap();

    let payload = OrderPayload {
        sender,
        market_index,
        side,
        size,
        price,
        nonce,
    };
    // We need the sol struct to assert struct_hash too - recreate via the
    // public helper that returns the digest, then reach the inner via a
    // mirror struct. Easier: build the sol struct directly by re-using the
    // same conversion the public sign path does (one-liner).
    let sol = obsdn_sdk::sign::order::Order {
        sender,
        marketIndex: market_index,
        side: side as u8,
        size,
        price,
        nonce,
    };
    let digest = assert_hashes(&f, &sol, &domain);

    // End-to-end: sign via the public API and compare to fixture.
    let signer = LocalSigner::from_hex(&f.private_key).unwrap();
    let our_sig = sign_order(&signer, &domain, payload).unwrap();
    assert_eq!(our_sig, parse_sig(&f.signature));
    assert_signature_matches(&f, digest);
}

#[test]
fn transfer_hash_matches_go() {
    let f = load("transfer");
    let domain = fixture_domain(&f.domain);
    let from = parse_addr(f.input["from"].as_str().unwrap());
    let to = parse_addr(f.input["to"].as_str().unwrap());
    let token = parse_addr(f.input["token"].as_str().unwrap());
    let amount: u128 = f.input["amount"].as_str().unwrap().parse().unwrap();
    let nonce: u64 = f.input["nonce"].as_str().unwrap().parse().unwrap();

    let sol = obsdn_sdk::sign::transfer::Transfer {
        from,
        to,
        token,
        amount,
        nonce,
    };
    let digest = assert_hashes(&f, &sol, &domain);

    let signer = LocalSigner::from_hex(&f.private_key).unwrap();
    let our_sig = sign_transfer(
        &signer,
        &domain,
        TransferPayload {
            from,
            to,
            token,
            amount,
            nonce,
        },
    )
    .unwrap();
    assert_eq!(our_sig, parse_sig(&f.signature));
    assert_signature_matches(&f, digest);
}

#[test]
fn withdraw_hash_matches_go() {
    let f = load("withdraw");
    let domain = fixture_domain(&f.domain);
    let sender = parse_addr(f.input["sender"].as_str().unwrap());
    let token = parse_addr(f.input["token"].as_str().unwrap());
    let amount: u128 = f.input["amount"].as_str().unwrap().parse().unwrap();
    let nonce: u64 = f.input["nonce"].as_str().unwrap().parse().unwrap();

    let sol = obsdn_sdk::sign::withdraw::Withdraw {
        sender,
        token,
        amount,
        nonce,
    };
    let digest = assert_hashes(&f, &sol, &domain);

    let signer = LocalSigner::from_hex(&f.private_key).unwrap();
    let our_sig = sign_withdraw(
        &signer,
        &domain,
        WithdrawPayload {
            sender,
            token,
            amount,
            nonce,
        },
    )
    .unwrap();
    assert_eq!(our_sig, parse_sig(&f.signature));
    assert_signature_matches(&f, digest);
}

#[test]
fn create_vault_hash_matches_go() {
    let f = load("create_vault");
    let domain = fixture_domain(&f.domain);
    let main = parse_addr(f.input["main"].as_str().unwrap());
    let vault = parse_addr(f.input["vault"].as_str().unwrap());
    let profit_share_bps =
        U256::from_str_radix(f.input["profit_share_bps"].as_str().unwrap(), 10).unwrap();

    let sol = obsdn_sdk::sign::vault::CreateVault {
        main,
        vault,
        profitShareBps: profit_share_bps,
    };
    let digest = assert_hashes(&f, &sol, &domain);

    let signer = LocalSigner::from_hex(&f.private_key).unwrap();
    let our_sig = sign_create_vault(
        &signer,
        &domain,
        CreateVaultPayload {
            main,
            vault,
            profit_share_bps,
        },
    )
    .unwrap();
    assert_eq!(our_sig, parse_sig(&f.signature));
    assert_signature_matches(&f, digest);
}

#[test]
fn stake_vault_hash_matches_go() {
    let f = load("stake_vault");
    let domain = fixture_domain(&f.domain);
    let vault = parse_addr(f.input["vault"].as_str().unwrap());
    let staker = parse_addr(f.input["staker"].as_str().unwrap());
    let token = parse_addr(f.input["token"].as_str().unwrap());
    let amount = U256::from_str_radix(f.input["amount"].as_str().unwrap(), 10).unwrap();
    let nonce: u64 = f.input["nonce"].as_str().unwrap().parse().unwrap();

    let sol = obsdn_sdk::sign::vault::StakeVault {
        vault,
        staker,
        token,
        amount,
        nonce,
    };
    let digest = assert_hashes(&f, &sol, &domain);

    let signer = LocalSigner::from_hex(&f.private_key).unwrap();
    let our_sig = sign_stake_vault(
        &signer,
        &domain,
        StakeVaultPayload {
            vault,
            staker,
            token,
            amount,
            nonce,
        },
    )
    .unwrap();
    assert_eq!(our_sig, parse_sig(&f.signature));
    assert_signature_matches(&f, digest);
}

#[test]
fn unstake_vault_hash_matches_go() {
    let f = load("unstake_vault");
    let domain = fixture_domain(&f.domain);
    let vault = parse_addr(f.input["vault"].as_str().unwrap());
    let staker = parse_addr(f.input["staker"].as_str().unwrap());
    let token = parse_addr(f.input["token"].as_str().unwrap());
    let amount = U256::from_str_radix(f.input["amount"].as_str().unwrap(), 10).unwrap();
    let nonce: u64 = f.input["nonce"].as_str().unwrap().parse().unwrap();

    let sol = obsdn_sdk::sign::vault::UnstakeVault {
        vault,
        staker,
        token,
        amount,
        nonce,
    };
    let digest = assert_hashes(&f, &sol, &domain);

    let signer = LocalSigner::from_hex(&f.private_key).unwrap();
    let our_sig = sign_unstake_vault(
        &signer,
        &domain,
        UnstakeVaultPayload {
            vault,
            staker,
            token,
            amount,
            nonce,
        },
    )
    .unwrap();
    assert_eq!(our_sig, parse_sig(&f.signature));
    assert_signature_matches(&f, digest);
}

#[test]
fn create_subaccount_hash_matches_go() {
    let f = load("create_subaccount");
    let domain = fixture_domain(&f.domain);
    let main = parse_addr(f.input["main"].as_str().unwrap());
    let subaccount = parse_addr(f.input["subaccount"].as_str().unwrap());

    let sol = obsdn_sdk::sign::subaccount::CreateSubaccount { main, subaccount };
    let digest = assert_hashes(&f, &sol, &domain);

    let signer = LocalSigner::from_hex(&f.private_key).unwrap();
    let our_sig = sign_create_subaccount(
        &signer,
        &domain,
        CreateSubaccountPayload { main, subaccount },
    )
    .unwrap();
    assert_eq!(our_sig, parse_sig(&f.signature));
    assert_signature_matches(&f, digest);
}

#[test]
fn register_signed_by_sender_matches_go() {
    let f = load("register_signed_by_sender");
    let domain = fixture_domain(&f.domain);
    let sender_addr = parse_addr(f.input["sender"].as_str().unwrap());
    let signer_addr = parse_addr(f.input["signer"].as_str().unwrap());
    let message = f.input["message"].as_str().unwrap().to_string();
    let nonce: u64 = f.input["nonce"].as_str().unwrap().parse().unwrap();

    let sol = obsdn_sdk::sign::register::Register {
        sender: sender_addr,
        signer: signer_addr,
        message: message.clone(),
        nonce,
    };
    let digest = assert_hashes(&f, &sol, &domain);

    let signer = LocalSigner::from_hex(&f.private_key).unwrap();
    let our_sig = sign_register(
        &signer,
        &domain,
        RegisterPayload {
            sender: sender_addr,
            signer: signer_addr,
            message,
            nonce,
        },
    )
    .unwrap();
    assert_eq!(our_sig, parse_sig(&f.signature));
    assert_signature_matches(&f, digest);
}

#[test]
fn register_signed_by_signer_matches_go() {
    let f = load("register_signed_by_signer");
    let domain = fixture_domain(&f.domain);
    let account = parse_addr(f.input["account"].as_str().unwrap());

    let sol = obsdn_sdk::sign::register::DelegatedSigner { account };
    let digest = assert_hashes(&f, &sol, &domain);

    let signer = LocalSigner::from_hex(&f.private_key).unwrap();
    let our_sig =
        sign_delegated_signer(&signer, &domain, DelegatedSignerPayload { account }).unwrap();
    assert_eq!(our_sig, parse_sig(&f.signature));
    assert_signature_matches(&f, digest);
}

#[test]
fn register_child_account_signer_matches_go() {
    let f = load("register_child_account_signer");
    let domain = fixture_domain(&f.domain);
    let main = parse_addr(f.input["main"].as_str().unwrap());
    let child_account = parse_addr(f.input["child_account"].as_str().unwrap());
    let signer_addr = parse_addr(f.input["signer"].as_str().unwrap());
    let message = f.input["message"].as_str().unwrap().to_string();
    let nonce: u64 = f.input["nonce"].as_str().unwrap().parse().unwrap();

    let sol = obsdn_sdk::sign::subaccount::RegisterChildAccountSigner {
        main,
        childAccount: child_account,
        signer: signer_addr,
        message: message.clone(),
        nonce,
    };
    let digest = assert_hashes(&f, &sol, &domain);

    let signer = LocalSigner::from_hex(&f.private_key).unwrap();
    let our_sig = sign_register_child_account_signer(
        &signer,
        &domain,
        RegisterChildAccountSignerPayload {
            main,
            child_account,
            signer: signer_addr,
            message,
            nonce,
        },
    )
    .unwrap();
    assert_eq!(our_sig, parse_sig(&f.signature));
    assert_signature_matches(&f, digest);
}
