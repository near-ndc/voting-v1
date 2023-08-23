use near_sdk::borsh::{self, BorshSerialize};
use near_sdk::BorshStorageKey;
use near_sdk::serde::Deserialize;
/// Helper structure for keys of the persistent collections.
#[derive(BorshSerialize, BorshStorageKey)]
pub enum StorageKey {
    Proposals,
    ProposalVoters(u32),
    AcceptedPolicy,
    BondedAmount,
    UserSBT(u32),
}

#[derive(PartialEq, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub enum AccountFlag {
    /// Account is "blacklisted" when it was marked as a scam or breaking the IAH rules.
    Blacklisted,
    Verified,
}