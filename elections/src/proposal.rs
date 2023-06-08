use std::collections::HashSet;

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupSet;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, require, AccountId};

use crate::constants::*;

#[derive(Serialize, Deserialize, BorshDeserialize, BorshSerialize)]
#[serde(crate = "near_sdk::serde")]
pub enum HouseType {
    Other,
    HouseOfMerit,
    CouncilOfAdvisors,
    TransparencyCommission,
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct Proposal {
    pub typ: HouseType,
    pub ref_link: String,
    /// start of voting as Unix timestamp (in seconds)
    pub start: u64,
    /// end of voting as Unix timestamp (in seconds)
    pub end: u64,
    /// min amount of voters to legitimize the voting.
    pub quorum: u32,
    /// max amount of credits each voter has
    pub credits: u16,
    /// list of valid candidates. Must be ordered.
    pub candidates: Vec<AccountId>,
    /// running result (ongoing sum of votes per candidate), in the same order as `candidates`.
    /// result[i] = sum of votes for candidates[i]
    pub result: Vec<u64>,
    pub voters: LookupSet<AccountId>,
}

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct ProposalView {
    pub typ: HouseType,
    pub ref_link: String,
    /// start of voting as Unix timestamp (in seconds)
    pub start: u64,
    /// end of voting as Unix timestamp (in seconds)
    pub end: u64,
    /// min amount of voters to legitimize the voting.
    pub quorum: u32,
    /// max amount of credits each voter has
    pub credits: u16,
    pub candidates: Vec<AccountId>,
    /// sum of votes per candidate in the same as self.candidates
    pub result: Vec<u64>,
}

impl Proposal {
    pub fn to_view(self) -> ProposalView {
        ProposalView {
            typ: self.typ,
            ref_link: self.ref_link,
            start: self.start,
            end: self.end,
            quorum: self.quorum,
            credits: self.credits,
            candidates: self.candidates,
            result: self.result,
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
        require!(!self.voters.contains(&user), "user already voted");
        for candidate in vote {
            let idx = self.candidates.binary_search(&candidate).unwrap() as usize;
            self.result[idx] += 1;
        }
    }
}

pub type Vote = Vec<AccountId>;

/// * valid_candidates must be a sorted slice.
pub fn validate_vote(vs: &Vote, max_credits: u16, valid_candidates: &Vec<AccountId>) {
    require!(
        vs.len() <= max_credits as usize,
        format!("max vote is {} seats", max_credits)
    );
    let mut vote_for = HashSet::new();
    for candidate in vs {
        require!(
            vote_for.insert(candidate),
            "double vote for the same candidate"
        );
        require!(
            valid_candidates.binary_search(candidate).is_ok(),
            "vote for unknown candidate"
        );
    }
}
