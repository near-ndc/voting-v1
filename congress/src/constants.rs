use near_sdk::{Balance, Gas, ONE_NEAR};

pub const MILI_NEAR: Balance = ONE_NEAR / 1_000;

/// Gas reserved for final failure callback which panics if one of the callback fails.
pub const FAILURE_CALLBACK_GAS: Gas = Gas(3 * Gas::ONE_TERA.0);
pub const EXECUTE_CALLBACK_GAS: Gas = Gas(4 * Gas::ONE_TERA.0);

pub const EXEC_CTR_CALL_GAS: Gas = Gas(8 * Gas::ONE_TERA.0);
pub const EXEC_SELF_GAS: Gas = Gas(20 * Gas::ONE_TERA.0);
pub const MAX_EXEC_FUN_CALL_GAS: Gas =
    Gas(300 * Gas::ONE_TERA.0 - EXEC_SELF_GAS.0 - EXECUTE_CALLBACK_GAS.0);

// 64bytes(accountID) + 1byte (prefix) + 4bytes(proposal_id) + vote(byte) = 72B -> add 20% margin = < 90B
pub const VOTE_STORAGE: u64 = 90;
