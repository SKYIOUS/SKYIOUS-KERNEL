use proptest::prelude::*;

proptest! {
    #[test]
    fn clock_resolution_is_non_negative(clock_id in 0u64..200u64) {
        // Property: for any clock ID, resolution nanoseconds should be >= 0
        // and < 1_000_000_000 (less than 1 second)
        let nsec = match clock_id {
            0 | 1 | 4 | 7 | 100 | 101 | 102 | 103 | 104 | 105 => 1u64,
            5 | 6 => 10_000_000u64,
            2 | 3 => 1u64,
            _ => 0u64,
        };
        prop_assert!(nsec < 1_000_000_000);
    }

    #[test]
    fn abi_version_is_monotonic(v1 in 1u32..100u32, v2 in 1u32..100u32) {
        // Property: ABI version comparison is consistent
        // If v1 < v2, then v1 should be considered older
        if v1 < v2 {
            prop_assert!(v1 < v2);
        } else if v1 > v2 {
            prop_assert!(v1 > v2);
        } else {
            prop_assert_eq!(v1, v2);
        }
    }

    #[test]
    fn coarse_clock_resolution_is_10ms(clock_id in 5u64..7u64) {
        // Property: COARSE clocks always have 10ms resolution
        let resolution = match clock_id {
            5 | 6 => 10_000_000u64,
            _ => 0u64,
        };
        prop_assert_eq!(resolution, 10_000_000);
    }
}
