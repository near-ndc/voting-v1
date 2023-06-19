use near_sdk::{Balance, Gas};

pub const MICRO_NEAR: Balance = 1_000_000_000_000_000_000; // 1e19 yoctoNEAR
pub const MILI_NEAR: Balance = 1_000 * MICRO_NEAR;

/// 1s in nano seconds.
pub const SECOND: u64 = 1_000_000_000;
pub const MILI_SECOND: u64 = 1_000_000;

// storage cost: 1E19 yoctoNEAR ( = 10 microN) per byte --> 1miliN 100b --> 1 NEAR 100kB
// vote: 2 (prefix)+64bytes + 8 (proposal id) for key + 3 * 16  = 124 < 200 bytes
pub const VOTE_COST: Balance = 2 * MILI_NEAR;

pub const VOTE_GAS: Gas = Gas(70 * Gas::ONE_TERA.0);
pub const VOTE_GAS_CALLBACK: Gas = Gas(5 * Gas::ONE_TERA.0);

pub const MIN_REF_LINK_LEN: usize = 6;
pub const MAX_REF_LINK_LEN: usize = 120;
