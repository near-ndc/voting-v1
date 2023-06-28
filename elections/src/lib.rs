use std::collections::HashSet;

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, LookupSet};
use near_sdk::json_types::Base64VecU8;
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault, Promise};
use serde_json::json;

mod constants;
mod ext;
pub mod proposal;
mod storage;
mod view;

pub use crate::constants::*;
pub use crate::ext::*;
pub use crate::proposal::*;
use crate::storage::*;

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Contract {
    pub pause: bool,
    prop_counter: u32,
    pub proposals: LookupMap<u32, Proposal>,

    /// address which can pause the contract and make a new proposal. Should be a multisig / DAO;
    pub authority: AccountId,
    pub sbt_registry: AccountId,
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(authority: AccountId, sbt_registry: AccountId) -> Self {
        Self {
            pause: false,
            authority,
            sbt_registry,
            proposals: LookupMap::new(StorageKey::Proposals),
            prop_counter: 0,
        }
    }

    /**********
     * TRANSACTIONS
     **********/

    /// creates a new empty proposal. `start` and `end` is a timestamp in milliseconds.
    /// returns the new proposal ID
    /// NOTE: storage is paid from the account state
    pub fn create_proposal(
        &mut self,
        typ: HouseType,
        start: u64,
        end: u64,
        ref_link: String,
        quorum: u32,
        seats: u16,
        #[allow(unused_mut)] mut candidates: Vec<AccountId>,
    ) -> u32 {
        self.assert_admin();
        let min_start = env::block_timestamp_ms();
        require!(
            min_start < start,
            format!("proposal start must be in the future")
        );
        require!(start < end, "proposal start must be before end");
        require!(
            MIN_REF_LINK_LEN <= ref_link.len() && ref_link.len() <= MAX_REF_LINK_LEN,
            format!(
                "ref_link length must be between {} and {} bytes",
                MIN_REF_LINK_LEN, MAX_REF_LINK_LEN
            )
        );
        let cs: HashSet<&AccountId> = HashSet::from_iter(candidates.iter());
        require!(cs.len() == candidates.len(), "duplicated candidates");
        candidates.sort();

        self.prop_counter += 1;
        let l = candidates.len();
        let p = Proposal {
            typ,
            start,
            end,
            quorum,
            ref_link,
            seats,
            candidates,
            result: vec![0; l],
            voters: LookupSet::new(StorageKey::ProposalVoters(self.prop_counter)),
            voters_num: 0,
        };

        self.proposals.insert(&self.prop_counter, &p);
        self.prop_counter
    }

    /// election vote using plural vote mechanism
    #[payable]
    pub fn vote(&mut self, prop_id: u32, vote: Vote) -> Promise {
        let p = self._proposal(prop_id);
        p.assert_active();
        let user = env::predecessor_account_id();
        require!(!p.voters.contains(&user), "caller already voted");
        require!(
            env::attached_deposit() >= VOTE_COST,
            format!(
                "requires {} yocto deposit for storage fees for every new vote",
                VOTE_COST
            )
        );
        require!(
            env::prepaid_gas() >= VOTE_GAS,
            format!("not enough gas, min: {:?}", VOTE_GAS)
        );
        validate_vote(&vote, p.seats, &p.candidates);
        // TODO: any suggestions on how to improve this?
        // serialize the args
        let args: serde_json::Value = json!({"prop_id": prop_id, "user": user, "vote": vote});
        let vec = serde_json::to_vec(&args).unwrap();
        let base_64_args: Base64VecU8 = vec.into();

        // call SBT registry to verify SBT
        ext_sbtreg::ext(self.sbt_registry.clone()).is_human_call(
            user.clone(),
            env::current_account_id(),
            REGISTER_VOTE.into(),
            base_64_args,
        )
    }

    /// Registers the vote the vote. Can only be called by the registry.
    /// The method is being called from registry.is_human_call if the voter is a verifed human
    pub fn register_vote(&mut self, prop_id: u32, user: AccountId, vote: Vote) {
        require!(
            env::predecessor_account_id() == self.sbt_registry,
            "only registry can invoke this method"
        );
        let mut p = self._proposal(prop_id);
        p.vote_on_verified(&user, vote);
    }

    /*****************
     * INTERNAL
     ****************/

    #[inline]
    fn assert_admin(&self) {
        require!(
            self.authority == env::predecessor_account_id(),
            "not an admin"
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod unit_tests {
    use near_sdk::{test_utils::VMContextBuilder, testing_env, Gas, VMContext};

    use crate::*;

    /// 1ms in nano seconds
    const MSECOND: u64 = 1_000_000;

    const START: u64 = 10;

    fn alice() -> AccountId {
        AccountId::new_unchecked("alice.near".to_string())
    }

    fn bob() -> AccountId {
        AccountId::new_unchecked("bob.near".to_string())
    }

    fn candidate(idx: u32) -> AccountId {
        AccountId::new_unchecked(format!("candidate{}.near", idx))
    }

    fn admin() -> AccountId {
        AccountId::new_unchecked("admin.near".to_string())
    }

    fn sbt_registry() -> AccountId {
        AccountId::new_unchecked("sbt_registry.near".to_string())
    }

    fn setup(predecessor: &AccountId) -> (VMContext, Contract) {
        let mut ctx = VMContextBuilder::new()
            .predecessor_account_id(admin())
            .block_timestamp(START * MSECOND)
            .is_view(false)
            .build();
        testing_env!(ctx.clone());
        let ctr = Contract::new(admin(), sbt_registry());
        ctx.predecessor_account_id = predecessor.clone();
        testing_env!(ctx.clone());
        return (ctx, ctr);
    }

    #[test]
    fn assert_admin() {
        let (_, ctr) = setup(&admin());
        ctr.assert_admin();
    }

    #[test]
    #[should_panic(expected = "not an admin")]
    fn assert_admin_fail() {
        let (_, ctr) = setup(&alice());
        ctr.assert_admin();
    }

    #[test]
    #[should_panic(expected = "proposal start must be in the future")]
    fn create_proposal_wrong_start_time() {
        let (_, mut ctr) = setup(&admin());
        ctr.create_proposal(
            crate::HouseType::HouseOfMerit,
            START - 1,
            START + 100,
            String::from("ref_link.io"),
            2,
            2,
            vec![candidate(1)],
        );
    }

    #[test]
    #[should_panic(expected = "proposal start must be before end")]
    fn create_proposal_end_before_start() {
        let (_, mut ctr) = setup(&admin());

        ctr.create_proposal(
            crate::HouseType::HouseOfMerit,
            START + 10,
            START,
            String::from("ref_link.io"),
            2,
            2,
            vec![candidate(1)],
        );
    }

    #[test]
    #[should_panic(expected = "ref_link length must be between 6 and 120 bytes")]
    fn create_proposal_wrong_ref_link_length() {
        let (_, mut ctr) = setup(&admin());

        ctr.create_proposal(
            crate::HouseType::HouseOfMerit,
            START + 1,
            START + 10,
            String::from("short"),
            2,
            2,
            vec![candidate(1)],
        );
    }

    #[test]
    #[should_panic(expected = "duplicated candidates")]
    fn create_proposal_duplicated_candidates() {
        let (_, mut ctr) = setup(&admin());

        ctr.create_proposal(
            crate::HouseType::HouseOfMerit,
            START + 1,
            START + 10,
            String::from("ref_link.io"),
            2,
            2,
            vec![candidate(1), candidate(1)],
        );
    }

    fn mk_proposal(ctr: &mut Contract) -> u32 {
        ctr.create_proposal(
            crate::HouseType::HouseOfMerit,
            START + 1,
            START + 10,
            String::from("ref_link.io"),
            2,
            2,
            vec![candidate(1), candidate(2), candidate(3)],
        )
    }

    fn voting_context(ctx: &mut VMContext) {
        ctx.attached_deposit = VOTE_COST;
        ctx.block_timestamp = (START + 2) * MSECOND;
        ctx.prepaid_gas = VOTE_GAS;
        ctx.predecessor_account_id = alice();
        testing_env!(ctx.clone());
    }

    #[test]
    fn create_proposal() {
        let (_, mut ctr) = setup(&admin());

        assert_eq!(ctr.prop_counter, 0);
        let prop_id = mk_proposal(&mut ctr);
        assert_eq!(ctr.prop_counter, 1);
        assert!(ctr.proposals.contains_key(&prop_id));

        let prop_id = mk_proposal(&mut ctr);
        assert_eq!(prop_id, 2);
        assert_eq!(ctr.prop_counter, 2);
        assert!(ctr.proposals.contains_key(&prop_id));

        let proposals = ctr.proposals();
        assert_eq!(proposals.len(), 2);
        assert_eq!(proposals[0].id, 1);
        assert_eq!(proposals[1].id, 2);
    }

    #[test]
    #[should_panic(expected = "can only vote between proposal start and end time")]
    fn vote_wrong_time() {
        let (_, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        let vote: Vote = vec![candidate(1)];
        ctr.vote(prop_id, vote);
    }

    #[test]
    #[should_panic(
        expected = "requires 2000000000000000000000 yocto deposit for storage fees for every new vote"
    )]
    fn vote_wrong_deposit() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);

        ctx.attached_deposit = VOTE_COST - 1;
        ctx.block_timestamp = (START + 2) * MSECOND;
        testing_env!(ctx.clone());
        ctr.vote(prop_id, vec![candidate(1)]);
    }

    #[test]
    #[should_panic(expected = "caller already voted")]
    fn vote_caller_already_voted() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);

        //set bob as voter and attempt double vote
        ctr._proposal(prop_id).voters.insert(&bob());
        ctx.predecessor_account_id = bob();
        ctx.attached_deposit = VOTE_COST;
        ctx.block_timestamp = (START + 2) * MSECOND;
        testing_env!(ctx.clone());
        ctr.vote(prop_id, vec![candidate(1)]);
    }

    #[test]
    #[should_panic(expected = "not enough gas, min: Gas(70000000000000)")]
    fn vote_wrong_gas() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        ctx.attached_deposit = VOTE_COST;
        ctx.block_timestamp = (START + 2) * MSECOND;
        ctx.prepaid_gas = Gas(10 * Gas::ONE_TERA.0);
        testing_env!(ctx.clone());
        ctr.vote(prop_id, vec![candidate(1)]);
    }

    #[test]
    #[should_panic(expected = "double vote for the same candidate")]
    fn vote_double_vote_same_candidate() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        ctx.attached_deposit = VOTE_COST;
        ctx.block_timestamp = (START + 2) * MSECOND;
        ctx.prepaid_gas = VOTE_GAS;
        testing_env!(ctx.clone());
        ctr.vote(prop_id, vec![candidate(1), candidate(1)]);
    }

    #[test]
    #[should_panic(expected = "vote for unknown candidate")]
    fn vote_unknown_candidate() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        voting_context(&mut ctx);
        ctr.vote(prop_id, vec![bob()]);
    }

    #[test]
    #[should_panic(expected = "max vote is 2 seats")]
    fn vote_too_many_credits() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        voting_context(&mut ctx);
        ctr.vote(prop_id, vec![candidate(1), candidate(2), candidate(3)]);
    }

    #[test]
    fn vote_empty_vote() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        voting_context(&mut ctx);
        ctr.vote(prop_id, vec![]);
        // note: we can only check vote result and state change through an integration test.
    }
}
