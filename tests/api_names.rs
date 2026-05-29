//! Compile-time contract for the public API names.
//!
//! If any public rename regresses, this file fails to compile. Most of it is
//! never executed — compiling is the assertion.
#![allow(dead_code, unused_imports)]

use std::sync::Arc;
use std::time::Duration;

use obsdn_sdk::rest::account::Account;
use obsdn_sdk::rest::asset::Asset;
use obsdn_sdk::rest::auth::Auth;
use obsdn_sdk::rest::chain::Chain;
use obsdn_sdk::rest::general::General;
use obsdn_sdk::rest::markets::Markets;
use obsdn_sdk::rest::orders::{LimitOrder, Orders};
use obsdn_sdk::rest::portfolio::Portfolio;
use obsdn_sdk::rest::price::Price;
use obsdn_sdk::rest::subaccount::Subaccount;
use obsdn_sdk::rest::vault::Vault;
use obsdn_sdk::types::v1::{SelfTradePrevention, TimeInForce};
use obsdn_sdk::ws::{
    Book, Channel, ChannelName, CollateralAsset, Event, Oracle, Order, Portfolio as WsPortfolio,
    Position, Session, SubscriptionStream, Ticker, TickerLevel, Trade, Update, UpdateKind,
};
use obsdn_sdk::{
    Client, ClientBuilder, Eip712Signer, Env, Error, LocalSigner, OrderSide, Result, Side,
};

// Every bare-noun service handle is reachable via its accessor.
fn _handles(c: &Client) {
    let _: Orders = c.orders();
    let _: Markets = c.markets();
    let _: Account = c.account();
    let _: Asset = c.asset();
    let _: Auth = c.auth();
    let _: Chain = c.chain();
    let _: General = c.general();
    let _: Portfolio = c.portfolio();
    let _: Price = c.price();
    let _: Subaccount = c.subaccount();
    let _: Vault = c.vault();
    let _: Session = c.ws();
}

// Builder entry + fluent setters.
fn _builder() -> Result<Client> {
    let _: ClientBuilder = Client::builder();
    Client::builder()
        .env(Env::Production)
        .api_key("k", "s")
        .timeout(Duration::from_secs(5))
        .user_agent("ua")
        .build()
}

// Signer trait + local impl.
fn _signers(_: Arc<dyn Eip712Signer>) {
    let _: Result<LocalSigner> = LocalSigner::from_hex("0x01");
}

// All ws types resolve by their de-stuttered names.
fn _ws_types() {
    let _: Option<SubscriptionStream> = None;
    let _: Option<Book> = None;
    let _: Option<Ticker> = None;
    let _: Option<Oracle> = None;
    let _: Option<Trade> = None;
    let _: Option<Order> = None;
    let _: Option<Position> = None;
    let _: Option<WsPortfolio> = None;
    let _: Option<CollateralAsset> = None;
    let _: Option<TickerLevel> = None;
    let _: Option<Update> = None;
}

fn _ws_match(e: Event) {
    match e {
        Event::Update(u) => {
            let _: UpdateKind = u.kind;
            let _: ChannelName = u.channel;
            let _: u64 = u.gsn;
        }
        Event::Reconnected => {}
        Event::Unauthorized(_) => {}
    }
}

#[test]
fn constructors_and_variants_compile() {
    // Order builder, full chain.
    let _ = LimitOrder::new("BTC-PERP", Side::Buy, 100.0, 1.0)
        .post_only(true)
        .reduce_only(false)
        .time_in_force(TimeInForce::Gtc)
        .self_trade_prevention(SelfTradePrevention::Unspecified)
        .client_order_id("c1")
        .nonce(1)
        .await_match(false);

    // Channel constructors.
    let _ = Channel::book("BTC-PERP");
    let _ = Channel::ticker("BTC-PERP");
    let _ = Channel::oracle("BTC");
    let _ = Channel::trade(None);
    let _ = Channel::order(Some("BTC-PERP"));
    let _ = Channel::position(None);
    let _ = Channel::event(None);

    // Renamed error variant.
    let _ = Error::UnparsedBody {
        status: 500,
        body: "x".into(),
    };

    // `Side` is the ergonomic alias for the full `OrderSide`.
    let _: Side = OrderSide::Buy;
}
