use near_sdk::{Balance, Gas, ONE_NEAR};

pub const MILI_NEAR: Balance = ONE_NEAR / 1_000;

// storage cost: 1E19 yoctoNEAR ( = 10 microN) per byte --> 1miliN per 100B --> 1 NEAR per 100kB
// vote: max to tokenIDs: 2* (8bytes(tokenID) + prefix(1byte + 4bytes)) = 26B
// vote: user_sbt: prefix 5 bytes, tokenId 5 bytes, accountId 25 bytes = 35B.
// => 61 bytes << 100 bytes
pub const VOTE_COST: Balance = MILI_NEAR;

// 64bytes(accountID) + 1byte (prefix) + 32bytes (hash bytes) = 97B < 100B=1 miliNEAR
pub const ACCEPT_POLICY_COST: Balance = MILI_NEAR;
pub const ACCEPT_POLICY_GAS: Gas = Gas(70 * Gas::ONE_TERA.0);

pub const BOND_AMOUNT: Balance = 3 * ONE_NEAR;
pub const GRAY_BOND_AMOUNT: Balance = 300 * ONE_NEAR;
pub const MINT_COST: Balance = 10 * MILI_NEAR; // 0.01 NEAR

pub const MINT_GAS: Gas = Gas(9 * Gas::ONE_TERA.0);
pub const VOTE_GAS: Gas = Gas(110 * Gas::ONE_TERA.0);
pub const VOTE_GAS_CALLBACK: Gas = Gas(10 * Gas::ONE_TERA.0);
pub const REVOKE_VOTE_GAS_CALLBACK: Gas = Gas(5 * Gas::ONE_TERA.0);

pub const MIN_REF_LINK_LEN: usize = 6;
pub const MAX_REF_LINK_LEN: usize = 120;

/// Gas reserved for final failure callback which panics if one of the callback fails.
pub const FAILURE_CALLBACK_GAS: Gas = Gas(3 * Gas::ONE_TERA.0);

pub const I_VOTED_SBT_CLASS: u64 = 1;
