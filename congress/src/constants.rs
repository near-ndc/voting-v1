use near_sdk::{Balance, Gas, ONE_NEAR};

pub const MILI_NEAR: Balance = ONE_NEAR / 1_000;

/// bond to create a new proposal
pub const PROPOSAL_BOND: Balance = 100 * MILI_NEAR;

/// Gas reserved for final failure callback which panics if one of the callback fails.
pub const FAILURE_CALLBACK_GAS: Gas = Gas(3 * Gas::ONE_TERA.0);
