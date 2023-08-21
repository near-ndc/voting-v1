use near_sdk::{Balance, Gas, ONE_NEAR};

pub const MICRO_NEAR: Balance = 1_000_000_000_000_000_000; // 1e18 yoctoNEAR
pub const MILI_NEAR: Balance = 1_000 * MICRO_NEAR;

// storage cost: 1E19 yoctoNEAR ( = 10 microN) per byte --> 1miliN per 100B --> 1 NEAR per 100kB
// vote: max to tokenIDs: 2* (8bytes(tokenID) + prefix(1byte + 4bytes)) = 26B << 50B
pub const VOTE_COST: Balance = MILI_NEAR / 2;

// 64bytes(accountID) + 1byte (prefix) + 32bytes (hash bytes) = 97B < 100B=1 miliNEAR
pub const ACCEPT_POLICY_COST: Balance = MILI_NEAR;
pub const BOND_AMOUNT: Balance = 3 * ONE_NEAR;
pub const GRAY_BOND_AMOUNT: Balance = 300 * ONE_NEAR;

pub const VOTE_GAS: Gas = Gas(70 * Gas::ONE_TERA.0);
pub const VOTE_GAS_CALLBACK: Gas = Gas(5 * Gas::ONE_TERA.0);

pub const MIN_REF_LINK_LEN: usize = 6;
pub const MAX_REF_LINK_LEN: usize = 120;
