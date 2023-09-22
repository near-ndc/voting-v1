use near_sdk::{Balance, Gas, ONE_NEAR};

pub const MILI_NEAR: Balance = ONE_NEAR / 1_000;

/// Gas reserved for final failure callback which panics if one of the callback fails.
pub const FAILURE_CALLBACK_GAS: Gas = Gas(3 * Gas::ONE_TERA.0);
