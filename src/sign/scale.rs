//! Decimal scaling helpers.
//!
//! REST exposes `f64` for size/price; EIP-712 hashes them as `uint128`
//! after a fixed `× 10^18` scale (see
//! `pkg/models/scalar.go::(Amount).X18()` and `(Price).X18()`). Go uses
//! shopspring `decimal.Decimal` for arbitrary precision, then
//! `BigInt()` truncates toward zero. This module mirrors that path with no
//! third-party decimal dep — a tiny string-based scaler that handles the
//! shapes a REST caller can produce.
//!
//! Why string-based? `f64 * 1e18` loses precision near u128 boundaries;
//! shopspring constructs the decimal from `strconv.FormatFloat(v, 'f', -1, 64)`
//! (the shortest round-trip decimal repr) which Rust's `format!("{}", v)`
//! also produces. So `scale_f64(v) == scale_decimal_str(format!("{}", v))`
//! up to the same float-to-decimal rounding Go performs.

use crate::error::{Error, Result};

/// Scale a decimal *string* by `10^18` and truncate toward zero, returning
/// the integer as `u128`.
///
/// Accepts:
/// - integer literals: `"1"`, `"1500"`
/// - decimal fractions: `"1.5"`, `"0.000001"`
/// - long fractions (excess digits truncated): `"1.123456789012345678999"`
///
/// Rejects: leading sign, exponent notation, embedded whitespace, multiple
/// dots. The REST layer already rejects NaN/Inf via
/// `services/nova/order_service_place.go::PlaceOrder`, so we don't see
/// those.
pub fn scale_decimal_str(s: &str) -> Result<u128> {
    if s.is_empty() {
        return Err(Error::Sign("empty decimal".into()));
    }
    if s.starts_with('-') || s.starts_with('+') {
        return Err(Error::Sign(format!("signed decimal not allowed: {s}")));
    }
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return Err(Error::Sign("empty decimal".into()));
    }
    if frac_part.contains('.') {
        return Err(Error::Sign(format!("malformed decimal: {s}")));
    }
    if !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(Error::Sign(format!("non-digit in decimal: {s}")));
    }

    // Pad/truncate fractional part to exactly 18 digits — equivalent to
    // shopspring `Shift(18).BigInt()` which scales then truncates toward
    // zero (see scalar.go).
    let mut padded = String::with_capacity(int_part.len() + 18);
    padded.push_str(if int_part.is_empty() { "0" } else { int_part });
    if frac_part.len() >= 18 {
        padded.push_str(&frac_part[..18]);
    } else {
        padded.push_str(frac_part);
        for _ in 0..(18 - frac_part.len()) {
            padded.push('0');
        }
    }
    padded
        .parse::<u128>()
        .map_err(|e| Error::Sign(format!("scaled value overflows u128: {s} ({e})")))
}

/// Scale an `f64` by `10^18` via the shopspring path: format with the
/// shortest round-trip decimal repr, then call [`scale_decimal_str`].
///
/// Returns `Error::Sign` on NaN/Inf — the REST layer already rejects those
/// upstream, but we re-check here so a misuse from a non-REST caller (e.g.,
/// signing an offline order) is loud.
pub fn scale_f64(v: f64) -> Result<u128> {
    if !v.is_finite() {
        return Err(Error::Sign(format!("non-finite float: {v}")));
    }
    if v < 0.0 {
        return Err(Error::Sign(format!("negative float: {v}")));
    }
    // Rust's default `{}` formatter on f64 emits the shortest decimal that
    // round-trips back to the same f64 (Grisu/Ryu). `strconv.FormatFloat`
    // with prec=-1 in Go does the same. Outputs match for typical exchange
    // sizes / prices.
    let s = format!("{}", v);
    scale_decimal_str(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_string_scales_18_zeros() {
        assert_eq!(
            scale_decimal_str("1").unwrap(),
            1_000_000_000_000_000_000u128
        );
        assert_eq!(
            scale_decimal_str("1500").unwrap(),
            1_500_000_000_000_000_000_000u128
        );
    }

    #[test]
    fn fractional_pads_to_18() {
        assert_eq!(
            scale_decimal_str("1.5").unwrap(),
            1_500_000_000_000_000_000u128
        );
        assert_eq!(scale_decimal_str("0.000000000000000001").unwrap(), 1u128);
    }

    #[test]
    fn excess_fractional_digits_truncate_toward_zero() {
        // 19 fractional digits — last digit dropped, no rounding.
        assert_eq!(
            scale_decimal_str("1.0000000000000000019").unwrap(),
            1_000_000_000_000_000_001u128
        );
    }

    #[test]
    fn rejects_signed_or_nondigit() {
        assert!(scale_decimal_str("-1").is_err());
        assert!(scale_decimal_str("+1").is_err());
        assert!(scale_decimal_str("1e3").is_err());
        assert!(scale_decimal_str("").is_err());
        assert!(scale_decimal_str("1.2.3").is_err());
    }

    #[test]
    fn f64_path_matches_string_path() {
        assert_eq!(scale_f64(1.5).unwrap(), scale_decimal_str("1.5").unwrap());
        assert!(scale_f64(f64::NAN).is_err());
        assert!(scale_f64(-1.0).is_err());
    }
}
