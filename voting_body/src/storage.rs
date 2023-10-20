use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{AccountId, BorshStorageKey};

/// Helper structure for keys of the persistent collections.
#[derive(BorshSerialize, BorshStorageKey)]
pub enum StorageKey {
    PreVoteProposals,
    Proposals,
    Accounts,
}

/// External account required for the Voting Body.
#[derive(BorshSerialize, BorshDeserialize, Deserialize, Serialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug, PartialEq, Clone))]
pub struct Accounts {
    pub iah_registry: AccountId,
    pub community_treasury: AccountId,
    pub congress_hom: AccountId,
    pub congress_coa: AccountId,
    pub congress_tc: AccountId,
    pub admin: AccountId,
}
