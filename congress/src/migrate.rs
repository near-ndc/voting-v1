use crate::*;
#[derive(BorshDeserialize, BorshSerialize)]
pub struct OldState {
    pub community_fund: AccountId,
    pub registry: AccountId,
    pub dissolved: bool,
    pub prop_counter: u32,
    pub proposals: LookupMap<u32, Proposal>,
    pub members: LazyOption<(Vec<AccountId>, Vec<PropPerm>)>,
    pub threshold: u8,
    pub hook_auth: LazyOption<HashMap<AccountId, Vec<HookPerm>>>,
    pub start_time: u64,
    pub end_time: u64,
    pub cooldown: u64,
    pub voting_duration: u64,
    pub min_voting_duration: u64,
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
        // new field in the smart contract :
        // + members_len: u8,
        // new field field in proposal

        let members_len = old_state.members.get().unwrap().0.len() as u8;

        Self {
            community_fund: old_state.community_fund,
            registry: old_state.registry,
            dissolved: old_state.dissolved,
            prop_counter: old_state.prop_counter,
            proposals: old_state.proposals,
            members: old_state.members,
            members_len,
            threshold: old_state.threshold,
            hook_auth: old_state.hook_auth,
            start_time: old_state.start_time,
            end_time: old_state.end_time,
            cooldown: old_state.cooldown,
            voting_duration: old_state.voting_duration,
            min_voting_duration: old_state.min_voting_duration,
            budget_spent: old_state.budget_spent,
            budget_cap: old_state.budget_cap,
            big_funding_threshold: old_state.big_funding_threshold,
        }
    }
}
