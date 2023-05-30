use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, LookupSet};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, require, AccountId};

use crate::constants::*;

#[derive(Serialize, Deserialize, BorshDeserialize, BorshSerialize)]
#[serde(crate = "near_sdk::serde")]
pub enum HouseType {
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
    /// total amount of credits each voter will have
    pub credits: u16,
    pub candidates: Vec<AccountId>,
    pub votes: LookupMap<u16, u64>,
    pub voters: LookupSet<AccountId>,
}

// #[derive(Serialize, Deserialize)]
// #[serde(crate = "near_sdk::serde")]
// pub struct ProposalView {
//     pub result: Result,
//     pub yes: u64,
//     pub no: u64,
//     pub abstain: u64,
//     /// start of voting as Unix timestamp (in seconds)
//     pub start: u64,
//     /// end of voting as Unix timestamp (in seconds)
//     pub end: u64,
//     title: String,
//     ref_link: String,
//     ref_hash: String,
// }

impl Proposal {
    //     pub fn compute_result(&self, c: &Consent) -> Result {
    //         if self.end <= env::block_timestamp() / SECOND {
    //             return Result::Ongoing;
    //         }
    //         let yesno = self.yes + self.no;
    //         if yesno + self.abstain >= c.quorum && self.yes > (yesno * c.threshold as u64 / 100) {
    //             Result::Yes
    //         } else {
    //             Result::No
    //         }
    //     }

    //     pub fn to_view(&self, c: &Consent) -> ProposalView {
    //         ProposalView {
    //             result: self.compute_result(c),
    //             yes: self.yes,
    //             no: self.no,
    //             abstain: self.abstain,
    //             start: self.start,
    //             end: self.end,
    //             title: self.title.clone(),
    //             ref_link: self.ref_link.clone(),
    //             ref_hash: hex::encode(&self.ref_hash),
    //         }
    //     }

    pub fn assert_active(&self) {
        let now = env::block_timestamp() / SECOND;
        require!(
            self.start <= now && now <= self.end,
            "can only vote between proposal start and end time"
        )
    }

    //     /// once vote proof has been verify, we call this function to register a vote.
    //     /// User can vote multiple times, as long as the vote is active. Subsequent
    //     /// calls will overwrite previous votes.
    //     pub fn vote_on_verified(&mut self, user: &AccountId, vote: Vote) {
    //         self.assert_active();

    //         // TODO: save Aggregated Vote to make sure we handle vote overwrite correctly
    //         if let Some(previous) = self.votes.get(user) {
    //             if previous == vote {
    //                 return;
    //             }
    //             match previous {
    //                 Vote::No => self.no -= 1,
    //                 Vote::Yes => self.no -= 1,
    //                 Vote::Abstain => self.abstain -= 1,
    //             }
    //         }

    //         self.votes.insert(&user, &vote);
    //         match vote {
    //             Vote::No => self.no += 1,
    //             Vote::Yes => self.no += 1,
    //             Vote::Abstain => self.abstain += 1,
    //         }
    //     }
}

pub type Vote = Vec<(AccountId, u16)>;
