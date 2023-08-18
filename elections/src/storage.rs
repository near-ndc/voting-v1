use near_sdk::borsh::{self, BorshSerialize};
use near_sdk::BorshStorageKey;

/// Helper structure for keys of the persistent collections.
#[derive(BorshSerialize, BorshStorageKey)]
pub enum StorageKey {
    Proposals,
    ProposalVoters(u32),
    VotersCandidates(u32),
    AcceptedPolicy,
    UsersSBT(u32),
}
