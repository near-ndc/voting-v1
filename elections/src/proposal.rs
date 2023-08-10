use std::collections::HashSet;

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, LookupSet};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{env, require, AccountId};
use uint::hex;

pub use crate::constants::*;
use crate::{TokenId, VoteError};

#[derive(Serialize, Deserialize, BorshDeserialize, BorshSerialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum HouseType {
    HouseOfMerit,
    CouncilOfAdvisors,
    TransparencyCommission,
}

#[derive(BorshDeserialize, BorshSerialize)]
#[cfg_attr(test, derive(Debug))]
pub struct Proposal {
    pub typ: HouseType,
    pub ref_link: String,
    /// start of voting as Unix timestamp (in milliseconds)
    pub start: u64,
    /// end of voting as Unix timestamp (in milliseconds)
    pub end: u64,
    /// duration of cooldown after the proposal ends. During this time votes cannot be submitted and
    /// the malicious votes can be revoked by authorities (in milliseconds).
    pub cooldown: u64,
    /// min amount of voters to legitimize the voting.
    pub quorum: u32,
    /// max amount of seats a voter can allocate candidates for.
    pub seats: u16,
    /// list of valid candidates. Must be ordered.
    pub candidates: Vec<AccountId>,
    /// running result (ongoing sum of votes per candidate), in the same order as `candidates`.
    /// result[i] = sum of votes for candidates[i]
    pub result: Vec<u64>,
    /// set of tokenIDs, which were used for voting, as a proof of personhood
    pub voters: LookupSet<TokenId>,
    pub voters_num: u32,
    // map of voters -> candidates they voted for (token IDs used for voting -> candidates index)
    pub voters_candidates: LookupMap<TokenId, Vec<usize>>,
    /// blake2s-256 hash of the Fair Voting Policy text.
    pub policy: [u8; 32],
}

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
#[cfg_attr(test, derive(Debug, PartialEq))]
#[cfg_attr(not(target_arch = "wasm32"), derive(Deserialize))]
pub struct ProposalView {
    pub id: u32,
    pub typ: HouseType,
    pub ref_link: String,
    /// start of voting as Unix timestamp (in milliseconds)
    pub start: u64,
    /// end of voting as Unix timestamp (in milliseconds)
    pub end: u64,
    /// cooldown period after voting ends (in milliseconds)
    pub cooldown: u64,
    /// min amount of voters to legitimize the voting.
    pub quorum: u32,
    pub voters_num: u32,
    /// max amount of credits each voter has
    pub seats: u16,
    /// list of candidates with sum of votes.
    pub result: Vec<(AccountId, u64)>,
    /// blake2s-256 hex-encoded hash of the Fair Voting Policy text.
    pub policy: String,
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
            cooldown: self.cooldown,
            quorum: self.quorum,
            voters_num: self.voters_num,
            seats: self.seats,
            result,
            policy: hex::encode(self.policy),
        }
    }

    pub fn assert_active(&self) {
        let now = env::block_timestamp_ms();
        require!(
            self.start <= now && now <= self.end,
            format!("can only vote between proposal start and end time")
        )
    }

    pub fn assert_active_cooldown(&self) {
        let now = env::block_timestamp_ms();
        require!(
            self.start <= now && now <= (self.end + self.cooldown),
            format!(
                "can only revoke votes between proposal start and end time + cooldown duration"
            )
        )
    }

    pub fn assert_used_token(&self, token_id: TokenId) {
        require!(
            self.voters.contains(&token_id),
            "voter did not vote on this proposal"
        );
    }

    /// once vote proof has been verified, we call this function to register a vote.
    pub fn vote_on_verified(&mut self, sbts: &Vec<TokenId>, vote: Vote) -> Result<(), VoteError> {
        self.assert_active();
        for t in sbts {
            if !self.voters.insert(t) {
                return Err(VoteError::DoubleVote(*t));
            }
        }
        let mut indexes = Vec::new();
        self.voters_num += 1;
        for candidate in vote {
            let idx = self.candidates.binary_search(&candidate).unwrap();
            self.result[idx] += 1;
            indexes.push(idx);
        }
        // TODO: this logic needs to be updated once we use more tokens per user to vote
        self.voters_candidates.insert(&sbts[0], &indexes);
        Ok(())
    }

    pub fn revoke_votes(&mut self, token_id: TokenId) {
        self.assert_active_cooldown();
        self.assert_used_token(token_id);
        for candidate in self
            .voters_candidates
            .get(&token_id)
            .expect("vote already revoked")
        {
            self.result[candidate] -= 1;
        }
        self.voters_num -= 1;
        self.voters_candidates.remove(&token_id);
    }
}

pub type Vote = Vec<AccountId>;

/// * valid_candidates must be a sorted slice.
pub fn validate_vote(vs: &Vote, max_credits: u16, valid_candidates: &[AccountId]) {
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

/// Decodes hex string into bytes. Panics if `s` is not a 64byte hex string.
pub fn assert_hash_hex_string(s: &str) -> [u8; 32] {
    require!(s.len() == 64, "policy must be a 64byte hex string");
    let mut a: [u8; 32] = [0u8; 32];
    hex::decode_to_slice(s, &mut a).expect("policy must be a proper hex string");
    a
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use near_sdk::collections::LookupSet;

    use super::*;
    use crate::{storage::StorageKey, HouseType, ProposalView};

    fn mk_account(i: u16) -> AccountId {
        AccountId::new_unchecked(format!("acc{}", i))
    }

    fn policy1() -> [u8; 32] {
        assert_hash_hex_string("f1c09f8686fe7d0d798517111a66675da0012d8ad1693a47e0e2a7d3ae1c69d4")
    }

    #[test]
    fn test_assert_hash_hex_string() {
        let h = "f1c09f8686fe7d0d798517111a66675da0012d8ad1693a47e0e2a7d3ae1c69d4";
        let b1 = assert_hash_hex_string(h);
        let b2 = hex::decode(h).unwrap();
        assert_eq!(b1.to_vec(), b2);
    }

    #[test]
    #[should_panic(expected = "policy must be a 64byte hex string")]
    fn test_assert_hash_hex_string_not_64bytes() {
        let h = "f1c09f8";
        assert_hash_hex_string(h);
    }

    #[test]
    fn to_proposal_view() {
        let p = Proposal {
            typ: HouseType::CouncilOfAdvisors,
            ref_link: "near.social/abc".to_owned(),
            start: 10,
            end: 111222,
            cooldown: 1000,
            quorum: 551,
            seats: 2,
            candidates: vec![mk_account(2), mk_account(1), mk_account(3), mk_account(4)],
            result: vec![10000, 5, 321, 121],
            voters: LookupSet::new(StorageKey::ProposalVoters(1)),
            voters_num: 10,
            voters_candidates: LookupMap::new(StorageKey::VotersCandidates(1)),
            policy: policy1(),
        };
        assert_eq!(
            ProposalView {
                id: 12,
                typ: HouseType::CouncilOfAdvisors,
                ref_link: p.ref_link.clone(),
                start: p.start,
                end: p.end,
                cooldown: p.cooldown,
                quorum: p.quorum,
                seats: p.seats,
                voters_num: p.voters_num,
                result: vec![
                    (mk_account(2), 10000),
                    (mk_account(1), 5),
                    (mk_account(3), 321),
                    (mk_account(4), 121)
                ],
                policy: "f1c09f8686fe7d0d798517111a66675da0012d8ad1693a47e0e2a7d3ae1c69d4"
                    .to_owned()
            },
            p.to_view(12)
        )
    }

    #[test]
    fn revoke_votes() {
        let mut p = Proposal {
            typ: HouseType::CouncilOfAdvisors,
            ref_link: "near.social/abc".to_owned(),
            start: 0,
            end: 100,
            cooldown: 10,
            quorum: 551,
            seats: 2,
            candidates: vec![mk_account(1), mk_account(2)],
            result: vec![3, 1],
            voters: LookupSet::new(StorageKey::ProposalVoters(1)),
            voters_num: 3,
            voters_candidates: LookupMap::new(StorageKey::VotersCandidates(1)),
        };
        p.voters.insert(&1);
        p.voters.insert(&2);
        p.voters.insert(&3);
        p.voters_candidates.insert(&1, &vec![0, 1]);
        p.voters_candidates.insert(&2, &vec![0]);
        p.voters_candidates.insert(&3, &vec![0]);

        p.revoke_votes(1);
        assert_eq!(p.result, vec![2, 0]);
        p.revoke_votes(2);
        assert_eq!(p.result, vec![1, 0]);
        p.revoke_votes(3);
        assert_eq!(p.result, vec![0, 0]);
    }

    #[test]
    #[should_panic(expected = "vote already revoked")]
    fn revoke_revoked_votes() {
        let mut p = Proposal {
            typ: HouseType::CouncilOfAdvisors,
            ref_link: "near.social/abc".to_owned(),
            start: 0,
            end: 100,
            cooldown: 10,
            quorum: 551,
            seats: 2,
            candidates: vec![mk_account(1), mk_account(2)],
            result: vec![1, 1],
            voters: LookupSet::new(StorageKey::ProposalVoters(1)),
            voters_num: 1,
            voters_candidates: LookupMap::new(StorageKey::VotersCandidates(1)),
        };
        p.voters.insert(&1);
        p.voters_candidates.insert(&1, &vec![0, 1]);

        p.revoke_votes(1);
        assert_eq!(p.result, vec![0, 0]);
        p.revoke_votes(1);
    }

    #[test]
    #[should_panic(expected = "voter did not vote on this proposal")]
    fn revoke_non_exising_votes() {
        let mut p = Proposal {
            typ: HouseType::CouncilOfAdvisors,
            ref_link: "near.social/abc".to_owned(),
            start: 0,
            end: 100,
            cooldown: 10,
            quorum: 551,
            seats: 2,
            candidates: vec![mk_account(1), mk_account(2)],
            result: vec![1, 1],
            voters: LookupSet::new(StorageKey::ProposalVoters(1)),
            voters_num: 1,
            voters_candidates: LookupMap::new(StorageKey::VotersCandidates(1)),
        };
        p.voters.insert(&1);
        p.voters_candidates.insert(&1, &vec![0, 1]);

        p.revoke_votes(2);
    }
}
