use near_sdk::{Balance, Gas, ONE_NEAR};

pub const MILI_NEAR: Balance = ONE_NEAR / 1_000;

/// Gas reserved for final failure callback which panics if one of the callback fails.
pub const FAILURE_CALLBACK_GAS: Gas = Gas(3 * Gas::ONE_TERA.0);
pub const EXECUTE_CALLBACK_GAS: Gas = Gas(4 * Gas::ONE_TERA.0);

pub const EXECUTE_GAS: Gas = Gas(8 * Gas::ONE_TERA.0);

// 64bytes(accountID) + 1byte (prefix) + 4bytes(proposal_id) + vote(byte) = 72B -> add 20% margin = < 90B
pub const VOTE_STORAGE: u64 = 90;
