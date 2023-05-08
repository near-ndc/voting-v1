use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, require, AccountId};

use crate::constants::*;

#[derive(BorshDeserialize, BorshSerialize)]
enum PropType {
    Constitution,
    HouseDismiss(HouseType),
}

#[derive(BorshDeserialize, BorshSerialize)]
enum HouseType {
    HouseOfMerit,
    CouncilOfAdvisors,
    TransparencyCommission,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Consent {
    /// percent of total stake voting required to pass a proposal.
    pub quorum: u8,
    /// #yes votes threshold as percent value (eg 12 = 12%)
    pub threshold: u8,
    // TODO: min amount of accounts
}

/// Simple vote: user uses his all power to vote for a single option.
#[derive(Serialize, Deserialize, BorshDeserialize, BorshSerialize, PartialEq)]
#[serde(crate = "near_sdk::serde")]
pub enum Vote {
    Abstain,
    No,
    Yes,
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct Proposal {
    pub voted: LookupMap<AccountId, Vote>,
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
            voted: LookupMap::new(crate::StorageKey::ProposalVoted),
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
    pub fn vote_on_verified(&mut self, user: &AccountId, vote: Vote) {
        self.assert_active();

        // TODO: save Aggregated Vote to make sure we handle vote overwrite correctly
        if let Some(previous) = self.voted.get(user) {
            if previous == vote {
                return;
            }
            match previous {
                Vote::No => self.no -= 1,
                Vote::Yes => self.no -= 1,
                Vote::Abstain => self.abstain -= 1,
            }
        }

        self.voted.insert(&user, &vote);
        match vote {
            Vote::No => self.no += 1,
            Vote::Yes => self.no += 1,
            Vote::Abstain => self.abstain += 1,
        }
    }
}
