use near_sdk::{Balance, Gas, ONE_NEAR};

pub const MILI_NEAR: Balance = ONE_NEAR / 1_000;
/// 0.9N
pub const SLASH_REWARD: Balance = 900 * MILI_NEAR;

/// Gas reserved for final failure callback which panics if one of the callback fails.
pub const FAILURE_CALLBACK_GAS: Gas = Gas(3 * Gas::ONE_TERA.0);
pub const EXECUTE_CALLBACK_GAS: Gas = Gas(4 * Gas::ONE_TERA.0);

pub const EXECUTE_GAS: Gas = Gas(8 * Gas::ONE_TERA.0);

// 64bytes(accountID) + 1byte (prefix) + 4bytes(proposal_id) + vote(byte) = 72B -> add 20% margin = < 90B
pub const VOTE_STORAGE: u64 = 90;

/// max voting duration to prevent common mistake with time unit. 90 days in milliseconds
pub const MAX_DURATION: u64 = 7776000000;
/// min voting duration to prevent common mistake with time unit. 1 day in milliseconds
pub const MIN_DURATION: u64 = 86400000;
