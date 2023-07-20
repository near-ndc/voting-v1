use near_sdk::{Balance, Gas};

pub const MICRO_NEAR: Balance = 1_000_000_000_000_000_000; // 1e18 yoctoNEAR
pub const MILI_NEAR: Balance = 1_000 * MICRO_NEAR;

// storage cost: 1E19 yoctoNEAR ( = 10 microN) per byte --> 1miliN per 100b --> 1 NEAR per 100kB
// vote: max to tokenIDs: 2* (8bytes(tokenID) + prefix(1byte + 4bytes)) = 26b << 50b
pub const VOTE_COST: Balance = MILI_NEAR / 2;

pub const VOTE_GAS: Gas = Gas(70 * Gas::ONE_TERA.0);
pub const VOTE_GAS_CALLBACK: Gas = Gas(5 * Gas::ONE_TERA.0);

pub const MIN_REF_LINK_LEN: usize = 6;
pub const MAX_REF_LINK_LEN: usize = 120;
