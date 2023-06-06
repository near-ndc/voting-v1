use near_sdk::{Balance, Gas};

pub const MICRO_NEAR: Balance = 1_000_000_000_000_000_000; // 1e19 yoctoNEAR
pub const MILI_NEAR: Balance = 1_000 * MICRO_NEAR;

/// 1s in nano seconds.
pub const SECOND: u64 = 1_000_000_000;

pub const GAS_NOMINATE: Gas = Gas(70 * Gas::ONE_TERA.0);
pub const GAS_VOTE_CALLBACK: Gas = Gas(5 * Gas::ONE_TERA.0);

pub const MAX_CAMPAIGN_LEN: usize = 200;
