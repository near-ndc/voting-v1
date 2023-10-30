use crate::*;
use near_sdk::serde::{Deserialize, Serialize};

#[near_bindgen]
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct OldProposal {
    pub proposer: AccountId,
    pub description: String,
    pub kind: PropKind,
    pub status: ProposalStatus,
    pub approve: u8,
    pub reject: u8,
    pub abstain: u8,
    pub votes: HashMap<AccountId, Vote>,
    pub submission_time: u64,
    pub approved_at: Option<u64>,
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct OldState {
    pub community_fund: AccountId,
    pub registry: AccountId,
    pub dissolved: bool,
    pub prop_counter: u32,
    pub proposals: LookupMap<u32, OldProposal>,
    pub members: LazyOption<(Vec<AccountId>, Vec<PropPerm>)>,
    pub threshold: u8,
    pub hook_auth: LazyOption<HashMap<AccountId, Vec<HookPerm>>>,
    pub start_time: u64,
    pub end_time: u64,
    pub cooldown: u64,
    pub voting_duration: u64,
    pub budget_spent: Balance,
    pub budget_cap: Balance,
    pub big_funding_threshold: Balance,
}

#[near_bindgen]
impl Contract {
    #[private]
    #[init(ignore_state)]
    /* pub  */
    pub fn migrate(min_voting_duration: u64) -> Self {
        let old_state: OldState = env::state_read().expect("failed");
        // new field in the smart contract :
        // + min_voting_duration: u64,
        // new field field in proposal
        // votes HashMap<AccountId, Vote> -> HashMap<AccountId, VoteRecord>, where
        // pub struct VoteRecord {
        //     pub timestamp: u64,
        //     pub vote: Vote,
        // }
        let mut new_proposals: LookupMap<u32, Proposal> = LookupMap::new(b"p");

        for id in 1..=old_state.prop_counter.clone() {
            if let Some(proposal) = old_state.proposals.get(&id) {
                let mut new_votes: HashMap<AccountId, VoteRecord> = HashMap::new();
                for (account_id, vote) in proposal.votes {
                    new_votes.insert(account_id, VoteRecord { timestamp: 0, vote });
                }

                new_proposals.insert(
                    &id,
                    &Proposal {
                        proposer: proposal.proposer,
                        description: proposal.description,
                        kind: proposal.kind,
                        status: proposal.status,
                        approve: proposal.approve,
                        reject: proposal.reject,
                        abstain: proposal.abstain,
                        votes: new_votes,
                        submission_time: proposal.submission_time,
                        approved_at: proposal.approved_at,
                    },
                );
            }
        }

        Self {
            community_fund: old_state.community_fund,
            registry: old_state.registry,
            dissolved: old_state.dissolved,
            prop_counter: old_state.prop_counter,
            proposals: new_proposals,
            members: old_state.members,
            threshold: old_state.threshold,
            hook_auth: old_state.hook_auth,
            start_time: old_state.start_time,
            end_time: old_state.end_time,
            cooldown: old_state.cooldown,
            voting_duration: old_state.voting_duration,
            min_voting_duration,
            budget_spent: old_state.budget_spent,
            budget_cap: old_state.budget_cap,
            big_funding_threshold: old_state.big_funding_threshold,
        }
    }
}
