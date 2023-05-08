use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, require, AccountId};

use crate::constants::*;
use uint::hex;

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
    Yes,
    No,
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub enum Result {
    Ongoing,
    Yes,
    No,
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct Proposal {
    pub title: String,
    pub ref_link: String,
    pub ref_hash: Vec<u8>,
    pub votes: LookupMap<AccountId, Vote>,
    pub yes: u128,
    pub no: u128,
    pub abstain: u128,
    /// start of voting as Unix timestamp (in seconds)
    pub start: u64,
    /// end of voting as Unix timestamp (in seconds)
    pub end: u64,
}

impl Proposal {
    pub fn new(
        prop_id: u32,
        start: u64,
        end: u64,
        title: String,
        ref_link: String,
        ref_hash: String,
    ) -> Self {
        require!(
            10 <= title.len() && title.len() <= 250,
            "title length must be between 10 and 250 bytes"
        );
        require!(
            6 <= ref_link.len() && ref_link.len() <= 120,
            "ref_link length must be between 6 and 120 bytes"
        );
        require!(ref_hash.len() == 64, "ref_hash length must be 64 hex");
        let ref_hash = hex::decode(ref_hash).expect("ref_hash must be a proper hex string");
        Self {
            title,
            ref_link,
            ref_hash,
            votes: LookupMap::new(crate::StorageKey::ProposalVotes(prop_id)),
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
pub struct ProposalView {
    pub result: Result,
    pub yes: u128,
    pub no: u128,
    pub abstain: u128,
    /// start of voting as Unix timestamp (in seconds)
    pub start: u64,
    /// end of voting as Unix timestamp (in seconds)
    pub end: u64,
    title: String,
    ref_link: String,
    ref_hash: String,
}

impl From<Proposal> for ProposalView {
    fn from(p: Proposal) -> Self {
        ProposalView {
            result: Result::No, // TODO
            yes: p.yes,
            no: p.no,
            abstain: p.abstain,
            start: p.start,
            end: p.end,
            title: p.title,
            ref_link: p.ref_link,
            ref_hash: hex::encode(&p.ref_hash),
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
        if let Some(previous) = self.votes.get(user) {
            if previous == vote {
                return;
            }
            match previous {
                Vote::No => self.no -= 1,
                Vote::Yes => self.no -= 1,
                Vote::Abstain => self.abstain -= 1,
            }
        }

        self.votes.insert(&user, &vote);
        match vote {
            Vote::No => self.no += 1,
            Vote::Yes => self.no += 1,
            Vote::Abstain => self.abstain += 1,
        }
    }
}
