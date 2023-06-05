use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::{AccountId, BorshStorageKey};

/// Helper structure for keys of the persistent collections.
#[derive(BorshSerialize, BorshStorageKey)]
pub enum StorageKey {
    Nominations,
    NominationsPerUser,
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct NominationKey {
    pub nominator: AccountId,
    pub nominee: AccountId,
}
