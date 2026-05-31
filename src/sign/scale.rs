//! Decimal scaling helpers.
//!
//! REST exposes `f64` for size/price; EIP-712 hashes them as `uint128`
//! scaled by `10^18` and truncated toward zero.
//!
//! Why string-based? `f64 * 1e18` loses precision near `u128` boundaries.
//! The reference implementation converts via the shortest round-trip decimal
//! representation, which Rust's `format!("{}", v)` also produces.
//! So `scale_f64(v) == scale_decimal_str(format!("{}", v))` for all values
//! a REST caller can supply.

use crate::error::{Error, Result};

/// Scale a decimal string by `10^18`, truncating toward zero, and return
/// the result as `u128`.
///
/// Accepts integer literals (`"1"`, `"1500"`), decimal fractions
/// (`"1.5"`, `"0.000001"`), and long fractions where excess digits are
/// truncated (`"1.123456789012345678999"`).
///
/// Rejects leading signs, exponent notation, embedded whitespace, and
/// multiple dots. NaN/Inf cannot appear here; the REST layer rejects them
/// before they reach signing.
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
    // Require an integer part: only `digits` and `digits.digits` are accepted,
    // not leading-dot forms like `.5` (which `scale_f64`'s formatter never
    // produces). This keeps the accepted grammar explicit and unambiguous.
    if int_part.is_empty() {
        return Err(Error::Sign(format!(
            "decimal must have an integer part: {s}"
        )));
    }
    if frac_part.contains('.') {
        return Err(Error::Sign(format!("malformed decimal: {s}")));
    }
    if !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(Error::Sign(format!("non-digit in decimal: {s}")));
    }

    // Pad/truncate fractional part to exactly 18 digits: scale by 10^18,
    // truncate toward zero.
    let mut padded = String::with_capacity(int_part.len() + 18);
    padded.push_str(int_part);
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

/// Scale an `f64` by `10^18` via the shortest round-trip decimal
/// representation, then delegate to [`scale_decimal_str`].
///
/// Returns `Error::Sign` on NaN/Inf. The REST layer rejects these upstream,
/// but the check is repeated here so non-REST callers (e.g. offline signing)
/// get a clear error instead of silent undefined behavior.
pub fn scale_f64(v: f64) -> Result<u128> {
    if !v.is_finite() {
        return Err(Error::Sign(format!("non-finite float: {v}")));
    }
    if v < 0.0 {
        return Err(Error::Sign(format!("negative float: {v}")));
    }
    // Rust's `{}` formatter emits the shortest decimal that round-trips
    // back to the same f64 (Grisu/Ryu), matching the reference
    // implementation's behavior for typical exchange sizes and prices.
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
        // 19 fractional digits - last digit dropped, no rounding.
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
    fn rejects_leading_dot() {
        // Only `digits` and `digits.digits` are accepted - not `.5` or `.`.
        assert!(scale_decimal_str(".5").is_err());
        assert!(scale_decimal_str(".").is_err());
        // The well-formed equivalent is accepted.
        assert_eq!(
            scale_decimal_str("0.5").unwrap(),
            500_000_000_000_000_000u128
        );
    }

    #[test]
    fn f64_path_matches_string_path() {
        assert_eq!(scale_f64(1.5).unwrap(), scale_decimal_str("1.5").unwrap());
        assert!(scale_f64(f64::NAN).is_err());
        assert!(scale_f64(-1.0).is_err());
    }
}
