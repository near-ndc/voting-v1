use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::UnorderedSet;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, require, AccountId};

use crate::constants::*;
use crate::types::*;

#[derive(BorshDeserialize, BorshSerialize)]
pub struct Proposal {
    pub voted: UnorderedSet<AccountId>,
    pub yes: u128,
    pub no: u128,
    pub abstain: u128,
    /// start of voting as Unix timestamp (in seconds)
    pub start: u64,
    /// end of voting as Unix timestamp (in seconds)
    pub end: u64,
}

impl Proposal {
    pub fn new(start: u64, end: u64) -> Self {
        Self {
            voted: UnorderedSet::new(crate::StorageKey::ProposalVoted),
            yes: 0,
            no: 0,
            abstain: 0,
            start,
            end,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub enum Result {
    Ongoing,
    Yes,
    No,
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct ProposalView {
    pub yes: u128,
    pub no: u128,
    pub abstain: u128,
    /// start of voting as Unix timestamp (in seconds)
    pub start: u64,
    /// end of voting as Unix timestamp (in seconds)
    pub end: u64,
    pub result: Result,
}

impl From<Proposal> for ProposalView {
    fn from(p: Proposal) -> Self {
        ProposalView {
            yes: p.yes,
            no: p.no,
            abstain: p.abstain,
            start: p.start,
            end: p.end,
            result: Result::No, // TODO
        }
    }
}

impl Proposal {
    pub fn assert_active(&self) {
        let now = env::block_timestamp() / SECOND;
        require!(
            self.start <= now && now <= self.end,
            "can only vote between proposal start and end time"
        )
    }

    /// once vote proof has been verify, we call this function to register a vote.
    /// User can vote multiple times, as long as the vote is active. Subsequent
    /// calls will overwrite previous votes.
    pub fn vote_on_verify(&mut self, user: &AccountId, vote: AggregateVote) {
        self.assert_active();

        // TODO: save Aggregated Vote to make sure we handle vote overwrite correctly
        self.voted.insert(&user);
        self.yes += vote.yes;
        self.no += vote.no;
        self.abstain += vote.abstain;
    }
}
