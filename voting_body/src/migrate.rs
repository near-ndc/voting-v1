use crate::*;

#[derive(BorshDeserialize, BorshSerialize)]
pub struct OldState {
    pub prop_counter: u32,
    /// Set of proposals in the pre-vote queue.
    pub pre_vote_proposals: LookupMap<u32, Proposal>,
    /// Set of active proposals.
    pub proposals: LookupMap<u32, Proposal>,
    /// map (prop_id, voter) -> VoteRecord
    pub votes: LookupMap<(u32, AccountId), VoteRecord>,

    /// Near amount required to create a proposal. Will be slashed if the proposal is marked as
    /// spam.
    pub pre_vote_bond: Balance,
    pub active_queue_bond: Balance,
    /// amount of users that need to support a proposal to move it to the active queue;
    pub pre_vote_support: u32,

    /// minimum amount of members to approve the proposal
    /// u32 can hold a number up to 4.2 B. That is enough for many future iterations.
    pub simple_consent: Consent,
    pub super_consent: Consent,

    /// all times below are in milliseconds
    pub pre_vote_duration: u64,
    pub vote_duration: u64,
    pub accounts: LazyOption<Accounts>,
}

#[near_bindgen]
impl Contract {
    #[private]
    #[init(ignore_state)]
    pub fn migrate() -> Self {
        let old_state: OldState = env::state_read().expect("Old state doesn't exist");
        Self {
            prop_counter: old_state.prop_counter,
            pre_vote_proposals: old_state.pre_vote_proposals,
            proposals: old_state.proposals,
            votes: old_state.votes,
            pre_vote_bond: old_state.pre_vote_bond,
            active_queue_bond: old_state.active_queue_bond,
            pre_vote_support: old_state.pre_vote_support,
            simple_consent: old_state.simple_consent,
            super_consent: old_state.super_consent,
            pre_vote_duration: old_state.pre_vote_duration,
            vote_duration: old_state.vote_duration,
            accounts: old_state.accounts,
            iom_whitelist: LookupSet::new(StorageKey::IomWhitelist),
        }
    }
}
