//! How a report's amounts are rounded.

use rust_decimal::{Decimal, RoundingStrategy};

/// Two decimal places, midpoint away from zero, trailing zeros dropped: how
/// a report's amounts are answered.
pub fn round_amount(d: Decimal) -> Decimal {
    d.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
        .normalize()
}
