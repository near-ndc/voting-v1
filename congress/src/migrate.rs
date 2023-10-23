use crate::*;

// congress/v0.1.0

#[derive(BorshDeserialize, BorshSerialize)]
pub struct OldProposal {
    pub proposer: AccountId,
    pub description: String,
    pub kind: PropKind,
    pub status: ProposalStatus,
    pub approve: u8,
    pub reject: u8,
    // abstain missing in the old state
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
    pub fn migrate() -> Self {
        let old_state: OldState = env::state_read().expect("failed");
        // New field in the contract.proposals -> Proposal :
        // + abstain: u8,

        let mut proposals: LookupMap<u32, Proposal> = LookupMap::new(StorageKey::Proposals);
        for prop_id in 1..=old_state.prop_counter {
            if let Some(old_prop) = old_state.proposals.get(&prop_id) {
                let new_prop = Proposal {
                    proposer: old_prop.proposer,
                    description: old_prop.description,
                    kind: old_prop.kind,
                    status: old_prop.status,
                    approve: old_prop.approve,
                    reject: old_prop.reject,
                    abstain: 0.clone(), // Set all to zero
                    votes: old_prop.votes,
                    submission_time: old_prop.submission_time,
                    approved_at: old_prop.approved_at,
                };
                proposals.insert(&prop_id, &new_prop);
            } else {
                // Skip this iteration when the old proposal is missing.
                continue;
            }
        }

        Self {
            community_fund: old_state.community_fund.clone(),
            registry: old_state.registry.clone(),
            dissolved: old_state.dissolved,
            prop_counter: old_state.prop_counter,
            proposals,
            members: old_state.members,
            threshold: old_state.threshold,
            hook_auth: old_state.hook_auth,
            start_time: old_state.start_time,
            end_time: old_state.end_time,
            cooldown: old_state.cooldown,
            voting_duration: old_state.voting_duration,
            budget_spent: old_state.budget_spent,
            budget_cap: old_state.budget_cap,
            big_funding_threshold: old_state.big_funding_threshold,
        }
    }
}
