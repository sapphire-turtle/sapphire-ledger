use rust_decimal::Decimal;
use sapphire_ledger_core::report::round_amount;

fn dec(s: &str) -> Decimal {
    s.parse().unwrap()
}

#[test]
fn round_amount_keeps_two_places_and_drops_trailing_zeros() {
    assert_eq!(
        round_amount(dec("100000") / dec("31")).to_string(),
        "3225.81"
    );
    assert_eq!(round_amount(dec("100000.00")).to_string(), "100000");
    assert_eq!(round_amount(dec("0.005")).to_string(), "0.01");
}
