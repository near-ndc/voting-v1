use crate::*;

#[derive(BorshDeserialize, PanicOnDefault)]
pub struct OldState {
    pub pause: bool,
    pub prop_counter: u32,
    pub proposals: LookupMap<u32, Proposal>,
    pub policy: [u8; 32],
    pub accepted_policy: LookupMap<AccountId, [u8; 32]>,
    pub bonded_amounts: LookupMap<TokenId, u128>,
    pub total_slashed: u128,
    pub finish_time: u64,
    pub authority: AccountId,
    pub sbt_registry: AccountId,
    pub disqualified_candidates: LazyOption<HashSet<AccountId>>,
}

#[near_bindgen]
impl Contract {
    #[private]
    #[init(ignore_state)]
    /* pub  */
    pub fn migrate(class_metadata: sbt::ClassMetadata) -> Self {
        let old_state: OldState = env::state_read().expect("failed");
        // new field in the smart contract :
        // + class_metadata: ClassMetadata,

        Self {
            pause: old_state.pause,
            prop_counter: old_state.prop_counter,
            proposals: old_state.proposals,
            policy: old_state.policy,
            accepted_policy: old_state.accepted_policy,
            bonded_amounts: old_state.bonded_amounts,
            total_slashed: old_state.total_slashed,
            finish_time: old_state.finish_time,
            authority: old_state.authority,
            sbt_registry: old_state.sbt_registry,
            disqualified_candidates: old_state.disqualified_candidates,
            class_metadata,
        }
    }
}
