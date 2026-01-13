//! Validator utilities for sorting, filtering, and mock data generation.

use crate::actions::ValidatorSortColumn;
use crate::app::ValidatorInfo;

/// Sort validators by the specified column.
pub fn sort_validators(
    validators: &mut [ValidatorInfo],
    column: ValidatorSortColumn,
    ascending: bool,
) {
    validators.sort_by(|a, b| {
        let cmp = match column {
            ValidatorSortColumn::Name => {
                let a_name = a.name.as_deref().unwrap_or(&a.address);
                let b_name = b.name.as_deref().unwrap_or(&b.address);
                a_name.cmp(b_name)
            }
            ValidatorSortColumn::Commission => a
                .commission
                .partial_cmp(&b.commission)
                .unwrap_or(std::cmp::Ordering::Equal),
            ValidatorSortColumn::TotalStake => a.total_stake.cmp(&b.total_stake),
            ValidatorSortColumn::OwnStake => a.own_stake.cmp(&b.own_stake),
            ValidatorSortColumn::NominatorCount => a.nominator_count.cmp(&b.nominator_count),
            ValidatorSortColumn::Apy => {
                let a_apy = a.apy.unwrap_or(0.0);
                let b_apy = b.apy.unwrap_or(0.0);
                a_apy
                    .partial_cmp(&b_apy)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
        };
        if ascending { cmp } else { cmp.reverse() }
    });
}

/// Filter validators by search query (matches name or address).
pub fn filter_validators<'a>(
    validators: &'a [ValidatorInfo],
    query: &str,
) -> Vec<&'a ValidatorInfo> {
    let query = query.to_lowercase();
    if query.is_empty() {
        return validators.iter().collect();
    }
    validators
        .iter()
        .filter(|v| {
            v.address.to_lowercase().contains(&query)
                || v.name
                    .as_ref()
                    .is_some_and(|n| n.to_lowercase().contains(&query))
        })
        .collect()
}

/// Generate mock validator data for testing/demo purposes.
pub fn generate_mock_validators(count: usize) -> Vec<ValidatorInfo> {
    let names = [
        "Polkadot Foundation",
        "Web3 Foundation",
        "Parity Technologies",
        "Stake Capital",
        "Figment Networks",
        "Chorus One",
        "P2P Validator",
        "Staked",
        "Everstake",
        "HashQuark",
        "Blockdaemon",
        "Bison Trails",
        "Certus One",
        "Dokia Capital",
        "Polychain Labs",
    ];

    (0..count)
        .map(|i| {
            let name_idx = i % names.len();
            let suffix = if i >= names.len() {
                format!(" #{}", i / names.len() + 1)
            } else {
                String::new()
            };

            ValidatorInfo {
                address: format!("1{:0>47}", i),
                name: Some(format!("{}{}", names[name_idx], suffix)),
                commission: (i % 20) as f64 / 100.0,
                total_stake: 1_000_000_000_000_000_000u128 + (i as u128 * 100_000_000_000_000),
                own_stake: 100_000_000_000_000_000u128 + (i as u128 * 10_000_000_000_000),
                nominator_count: 100 + (i as u32 * 5),
                points: 0,
                apy: Some(10.0 + (i % 10) as f64 * 0.5),
                blocked: i % 50 == 0,
            }
        })
        .collect()
}

/// Format stake amount for display (in DOT with 2 decimal places).
pub fn format_stake(planck: u128) -> String {
    let dot = planck as f64 / 10_000_000_000.0;
    if dot >= 1_000_000.0 {
        format!("{:.2}M", dot / 1_000_000.0)
    } else if dot >= 1_000.0 {
        format!("{:.2}K", dot / 1_000.0)
    } else {
        format!("{:.2}", dot)
    }
}

/// Format commission percentage for display.
pub fn format_commission(commission: f64) -> String {
    format!("{:.1}%", commission * 100.0)
}

/// Format APY percentage for display.
pub fn format_apy(apy: Option<f64>) -> String {
    match apy {
        Some(a) => format!("{:.2}%", a),
        None => "N/A".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_validators() -> Vec<ValidatorInfo> {
        vec![
            ValidatorInfo {
                address: "1abc".to_string(),
                name: Some("Charlie".to_string()),
                commission: 0.05,
                total_stake: 1000,
                own_stake: 100,
                nominator_count: 50,
                points: 0,
                apy: Some(12.0),
                blocked: false,
            },
            ValidatorInfo {
                address: "1def".to_string(),
                name: Some("Alice".to_string()),
                commission: 0.10,
                total_stake: 2000,
                own_stake: 200,
                nominator_count: 100,
                points: 0,
                apy: Some(10.0),
                blocked: false,
            },
            ValidatorInfo {
                address: "1ghi".to_string(),
                name: Some("Bob".to_string()),
                commission: 0.02,
                total_stake: 500,
                own_stake: 50,
                nominator_count: 25,
                points: 0,
                apy: None,
                blocked: false,
            },
        ]
    }

    #[test]
    fn test_sort_by_name_ascending() {
        let mut validators = sample_validators();
        sort_validators(&mut validators, ValidatorSortColumn::Name, true);
        assert_eq!(validators[0].name.as_deref(), Some("Alice"));
        assert_eq!(validators[1].name.as_deref(), Some("Bob"));
        assert_eq!(validators[2].name.as_deref(), Some("Charlie"));
    }

    #[test]
    fn test_sort_by_name_descending() {
        let mut validators = sample_validators();
        sort_validators(&mut validators, ValidatorSortColumn::Name, false);
        assert_eq!(validators[0].name.as_deref(), Some("Charlie"));
        assert_eq!(validators[1].name.as_deref(), Some("Bob"));
        assert_eq!(validators[2].name.as_deref(), Some("Alice"));
    }

    #[test]
    fn test_sort_by_commission() {
        let mut validators = sample_validators();
        sort_validators(&mut validators, ValidatorSortColumn::Commission, true);
        assert_eq!(validators[0].commission, 0.02);
        assert_eq!(validators[2].commission, 0.10);
    }

    #[test]
    fn test_sort_by_total_stake() {
        let mut validators = sample_validators();
        sort_validators(&mut validators, ValidatorSortColumn::TotalStake, false);
        assert_eq!(validators[0].total_stake, 2000);
        assert_eq!(validators[2].total_stake, 500);
    }

    #[test]
    fn test_sort_by_apy() {
        let mut validators = sample_validators();
        sort_validators(&mut validators, ValidatorSortColumn::Apy, false);
        assert_eq!(validators[0].apy, Some(12.0));
        assert_eq!(validators[1].apy, Some(10.0));
        // None APY should sort to bottom when descending
    }

    #[test]
    fn test_filter_by_name() {
        let validators = sample_validators();
        let filtered = filter_validators(&validators, "alice");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name.as_deref(), Some("Alice"));
    }

    #[test]
    fn test_filter_by_address() {
        let validators = sample_validators();
        let filtered = filter_validators(&validators, "1def");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].address, "1def");
    }

    #[test]
    fn test_filter_empty_query() {
        let validators = sample_validators();
        let filtered = filter_validators(&validators, "");
        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn test_filter_no_match() {
        let validators = sample_validators();
        let filtered = filter_validators(&validators, "xyz");
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_generate_mock_validators() {
        let validators = generate_mock_validators(10);
        assert_eq!(validators.len(), 10);
        assert!(validators[0].name.is_some());
        assert!(!validators[0].address.is_empty());
    }

    #[test]
    fn test_format_stake() {
        assert_eq!(format_stake(10_000_000_000), "1.00");
        assert_eq!(format_stake(10_000_000_000_000), "1.00K");
        assert_eq!(format_stake(10_000_000_000_000_000), "1.00M");
    }

    #[test]
    fn test_format_commission() {
        assert_eq!(format_commission(0.05), "5.0%");
        assert_eq!(format_commission(0.10), "10.0%");
        assert_eq!(format_commission(0.0), "0.0%");
    }

    #[test]
    fn test_format_apy() {
        assert_eq!(format_apy(Some(12.5)), "12.50%");
        assert_eq!(format_apy(None), "N/A");
    }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_sort_column() -> impl Strategy<Value = ValidatorSortColumn> {
        prop_oneof![
            Just(ValidatorSortColumn::Name),
            Just(ValidatorSortColumn::Commission),
            Just(ValidatorSortColumn::TotalStake),
            Just(ValidatorSortColumn::OwnStake),
            Just(ValidatorSortColumn::NominatorCount),
            Just(ValidatorSortColumn::Apy),
        ]
    }

    proptest! {
        #[test]
        fn test_sort_preserves_length(count in 0usize..100, column in arb_sort_column(), asc in any::<bool>()) {
            let mut validators = generate_mock_validators(count);
            let original_len = validators.len();
            sort_validators(&mut validators, column, asc);
            prop_assert_eq!(validators.len(), original_len);
        }

        #[test]
        fn test_filter_never_increases_length(count in 0usize..50, query in ".*") {
            let validators = generate_mock_validators(count);
            let filtered = filter_validators(&validators, &query);
            prop_assert!(filtered.len() <= validators.len());
        }

        #[test]
        fn test_format_stake_never_panics(planck in any::<u128>()) {
            let _ = format_stake(planck);
        }

        #[test]
        fn test_format_commission_never_panics(commission in any::<f64>()) {
            let _ = format_commission(commission);
        }
    }
}
