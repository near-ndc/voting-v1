use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, require, AccountId};

use crate::consent::Consent;
use crate::constants::*;
use uint::hex;

#[derive(Serialize, Deserialize, BorshDeserialize, BorshSerialize)]
#[serde(crate = "near_sdk::serde")]
pub enum ProposalType {
    Constitution,
    HouseDismiss(HouseType),
    // TODO: consider TextProposal
}

#[derive(Serialize, Deserialize, BorshDeserialize, BorshSerialize)]
#[serde(crate = "near_sdk::serde")]
pub enum HouseType {
    HouseOfMerit,
    CouncilOfAdvisors,
    TransparencyCommission,
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
    pub proposal_type: ProposalType,
    pub title: String,
    pub ref_link: String,
    pub ref_hash: Vec<u8>,
    pub votes: LookupMap<AccountId, Vote>,
    pub yes: u64,
    pub no: u64,
    pub abstain: u64,
    /// start of voting as Unix timestamp (in seconds)
    pub start: u64,
    /// end of voting as Unix timestamp (in seconds)
    pub end: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct ProposalView {
    pub result: Result,
    pub yes: u64,
    pub no: u64,
    pub abstain: u64,
    /// start of voting as Unix timestamp (in seconds)
    pub start: u64,
    /// end of voting as Unix timestamp (in seconds)
    pub end: u64,
    title: String,
    ref_link: String,
    ref_hash: String,
}

impl Proposal {
    pub fn new(
        prop_type: ProposalType,
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
            prop_type,
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

    pub fn compute_result(&self, c: &Consent) -> Result {
        if self.end <= env::block_timestamp() / SECOND {
            return Result::Ongoing;
        }
        let yesno = self.yes + self.no;
        if yesno + self.abstain >= c.quorum && self.yes > (yesno * c.threshold as u64 / 100) {
            Result::Yes
        } else {
            Result::No
        }
    }

    pub fn to_view(&self, c: &Consent) -> ProposalView {
        ProposalView {
            result: self.compute_result(c),
            yes: self.yes,
            no: self.no,
            abstain: self.abstain,
            start: self.start,
            end: self.end,
            title: self.title.clone(),
            ref_link: self.ref_link.clone(),
            ref_hash: hex::encode(&self.ref_hash),
        }
    }

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
