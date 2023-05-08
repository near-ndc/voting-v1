use near_sdk::{Balance, Gas};

pub const MICRO_NEAR: Balance = 1_000_000_000_000_000_000; // 1e19 yoctoNEAR
pub const MILI_NEAR: Balance = 1_000 * MICRO_NEAR;

/// 1s in nano seconds.
pub const SECOND: u64 = 1_000_000_000;

// storage cost: 1E19 yoctoNEAR per byte = 1 NEAR 100kB
// vote: 2 (prefix)+64bytes + 8 (proposal id) for key + 3 * 16  = 124 < 150
pub const VOTE_COST: Balance = 150 * MICRO_NEAR;

pub const GAS_VOTE: Gas = Gas(70 * Gas::ONE_TERA.0);

pub const GAS_VOTE_CALLBACK: Gas = Gas(5 * Gas::ONE_TERA.0);
