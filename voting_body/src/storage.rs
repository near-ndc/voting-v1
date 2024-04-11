use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{AccountId, BorshStorageKey};

/// Helper structure for keys of the persistent collections.
#[derive(BorshSerialize, BorshStorageKey)]
pub enum StorageKey {
    PreVoteProposals,
    Proposals,
    Accounts,
    Votes,
    IomWhitelist,
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod unit_tests2 {
    use near_sdk::IntoStorageKey;

    use crate::storage::StorageKey;

    #[test]
    fn check_storage() {
        assert_eq!(StorageKey::PreVoteProposals.into_storage_key(), vec![0]);
        assert_eq!(StorageKey::Accounts.into_storage_key(), vec![2]);
        assert_eq!(StorageKey::Votes.into_storage_key(), vec![3]);
    }
}
