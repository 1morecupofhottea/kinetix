//! Cost computation from admin-supplied prices (FR-6.3).
//!
//! No vendor prices are bundled. Models with no price configured produce
//! `None` (unknown), never `0.0`.

use crate::types::{Prices, TokenUsage};

/// Compute USD cost for a request. Returns `None` when prices are not configured.
pub fn compute_cost(prices: &Prices, usage: &TokenUsage) -> Option<f64> {
    if !prices.is_configured() {
        return None;
    }
    let input = usage.input.unwrap_or(0) as f64;
    let cached = usage.cached.unwrap_or(0) as f64;
    let output = usage.output.unwrap_or(0) as f64;
    let thinking = usage.thinking.unwrap_or(0) as f64;

    let billable_input = (input - cached).max(0.0);

    let input_price = prices.input_per_1m.unwrap_or(0.0);
    let cached_price = prices.cached_per_1m.unwrap_or(input_price);
    let output_price = prices.output_per_1m.unwrap_or(0.0);
    let thinking_price = prices.thinking_per_1m.unwrap_or(output_price);

    let cost = (billable_input * input_price
        + cached * cached_price
        + output * output_price
        + thinking * thinking_price)
        / 1_000_000.0;

    Some(cost)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_when_unpriced() {
        let p = Prices::default();
        let u = TokenUsage {
            input: Some(100),
            output: Some(50),
            ..Default::default()
        };
        assert!(compute_cost(&p, &u).is_none());
    }

    #[test]
    fn computes_with_cache_and_thinking() {
        let p = Prices {
            input_per_1m: Some(1.0),
            output_per_1m: Some(2.0),
            cached_per_1m: Some(0.1),
            thinking_per_1m: Some(2.0),
        };
        let u = TokenUsage {
            input: Some(1_000_000),
            output: Some(1_000_000),
            cached: Some(500_000),
            thinking: Some(100_000),
        };
        // 500k*1 + 500k*0.1 + 1M*2 + 100k*2 = 0.5 + 0.05 + 2.0 + 0.2 = 2.75
        let cost = compute_cost(&p, &u).unwrap();
        assert!((cost - 2.75).abs() < 1e-9, "got {cost}");
    }
}
