use near_sdk::Balance;

pub const MILI_NEAR: Balance = 1_000_000_000_000_000_000_000; // 10^21 Yocto NEAR.

// pub const BLACKLIST_COST: Balance = 5 * MILI_NEAR;
// pub const GAS_FOR_BLACKLIST: Gas = Gas(6 * Gas::ONE_TERA.0);

/// 1s in nano seconds.
pub const SECOND: u64 = 1_000_000_000;
