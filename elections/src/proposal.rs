use std::collections::HashSet;

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupSet;
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, require, AccountId};

pub use crate::constants::*;

#[derive(Serialize, Deserialize, BorshDeserialize, BorshSerialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum HouseType {
    Other,
    HouseOfMerit,
    CouncilOfAdvisors,
    TransparencyCommission,
}

#[derive(BorshDeserialize, BorshSerialize)]
#[cfg_attr(test, derive(Debug))]
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
    pub seats: u16,
    /// list of valid candidates. Must be ordered.
    pub candidates: Vec<AccountId>,
    /// running result (ongoing sum of votes per candidate), in the same order as `candidates`.
    /// result[i] = sum of votes for candidates[i]
    pub result: Vec<u64>,
    pub voters: LookupSet<AccountId>,
    pub voters_num: u32,
}

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct ProposalView {
    pub id: u32,
    pub typ: HouseType,
    pub ref_link: String,
    /// start of voting as Unix timestamp (in seconds)
    pub start: u64,
    /// end of voting as Unix timestamp (in seconds)
    pub end: u64,
    /// min amount of voters to legitimize the voting.
    pub quorum: u32,
    pub voters_num: u32,
    /// max amount of credits each voter has
    pub seats: u16,
    /// list of candidates with sum of votes.
    pub result: Vec<(AccountId, u64)>,
}

impl Proposal {
    pub fn to_view(self, id: u32) -> ProposalView {
        let mut result: Vec<(AccountId, u64)> = Vec::with_capacity(self.candidates.len());
        for i in 0..self.candidates.len() {
            let c = self.candidates[i].clone();
            let r = self.result[i];
            result.push((c, r));
        }
        ProposalView {
            id,
            typ: self.typ,
            ref_link: self.ref_link,
            start: self.start,
            end: self.end,
            quorum: self.quorum,
            voters_num: self.voters_num,
            seats: self.seats,
            result,
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
        require!(self.voters.insert(&user), "user already voted");
        self.voters_num += 1;
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use near_sdk::collections::LookupSet;

    use super::*;
    use crate::{storage::StorageKey, HouseType, ProposalView};

    fn mk_account(i: u16) -> AccountId {
        AccountId::new_unchecked(format!("acc{}", i))
    }

    #[test]
    fn to_proposal_view() {
        let p = Proposal {
            typ: HouseType::CouncilOfAdvisors,
            ref_link: "near.social/abc".to_owned(),
            start: 10,
            end: 111222,
            quorum: 551,
            seats: 2,
            candidates: vec![mk_account(2), mk_account(1), mk_account(3), mk_account(4)],
            result: vec![10000, 5, 321, 121],
            voters: LookupSet::new(StorageKey::ProposalVoters(1)),
            voters_num: 10,
        };
        assert_eq!(
            ProposalView {
                id: 12,
                typ: HouseType::CouncilOfAdvisors,
                ref_link: p.ref_link.clone(),
                start: p.start,
                end: p.end,
                quorum: p.quorum,
                seats: p.seats,
                voters_num: p.voters_num,
                result: vec![
                    (mk_account(2), 10000),
                    (mk_account(1), 5),
                    (mk_account(3), 321),
                    (mk_account(4), 121)
                ]
            },
            p.to_view(12)
        )
    }
}
