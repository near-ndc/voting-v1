use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::UnorderedMap;
// use near_sdk::json_types::U64;
// use near_sdk::serde::{Deserialize, Serialize};
// use near_sdk::CryptoHash;
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault};

mod constants;
mod proposal;
mod storage;
mod types;
mod view;
pub use crate::constants::*;
pub use crate::proposal::*;
use crate::storage::*;
pub use crate::types::*;

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Contract {
    pub pause: bool, // TODO: do we need pause? Then we will need admin.
    /// supermajority quorum
    pub sup_consent: Consent,
    pub consent: Consent,
    pub proposals: UnorderedMap<u64, Proposal>,
    prop_counter: u64,
    /// proposal voting duration in seconds
    pub prop_duration: u64,
    /// start_margin is a minimum duration in seconds before a proposal is submitted
    /// and proposal voting start.
    pub start_margin: u64,
    /// address which can pause the contract and make constitution proposal.
    /// Should be multisig / DAO;
    pub gwg: AccountId,
    /// stake proof verifier smart contract
    pub stake_verifier: AccountId,
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(
        gwg: AccountId,
        stake_verifier: AccountId,
        sup_consent: Consent,
        consent: Consent,
    ) -> Self {
        Self {
            pause: false,
            gwg,
            stake_verifier,
            sup_consent,
            consent,
            prop_duration: 10 * 60, // 10min, TODO: update this
            start_margin: 30,       // 30s
            proposals: UnorderedMap::new(StorageKey::Proposals),
            prop_counter: 0,
        }
    }

    /// creates new empty proposal
    /// returns proposal id
    pub fn creat_proposal(&mut self, start: u64) -> u64 {
        // TODO: check permissions
        // - add fun parameters (description ...)
        let min_start = self.start_margin + env::block_timestamp() / SECOND;
        require!(
            start >= min_start,
            format!("proposal start after {} unix time", min_start)
        );
        self.prop_counter += 1;
        self.proposals.insert(
            &self.prop_counter,
            &Proposal::new(start, start + self.prop_duration),
        );
        self.prop_counter
    }

    /// cast a simple vote for a binary proposal
    pub fn vote(&mut self, prop_id: u64, vote: SimpleVote, proof: Proof) {
        self.vote_aggregated(prop_id, vote.to_aggregated(proof.total_credits), proof)
    }

    /// aggregated vote for a binary proposal
    pub fn vote_aggregated(&mut self, prop_id: u64, vote: AggregateVote, proof: Proof) {
        require!(
            vote.yes + vote.abstain + vote.no == proof.total_credits,
            "total credits must equal sum of vote options"
        );
        let p = self._proposal(prop_id);
        p.assert_active();
    }

    /// cast a vote for elections
    pub fn elect(&mut self) {}
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {}
