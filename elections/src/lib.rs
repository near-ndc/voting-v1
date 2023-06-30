use std::collections::HashSet;

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, LookupSet};
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault, Promise};

mod constants;
mod errors;
mod ext;
pub mod proposal;
mod storage;
mod view;

pub use crate::constants::*;
pub use crate::errors::*;
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
        // call SBT registry to verify SBT
        ext_sbtreg::ext(self.sbt_registry.clone())
            .is_human(user)
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(VOTE_GAS_CALLBACK)
                    .on_vote_verified(prop_id, vote),
            )
    }

    /*****************
     * PRIVATE
     ****************/

    #[private]
    #[handle_result]
    pub fn on_vote_verified(
        &mut self,
        #[callback_unwrap] tokens: HumanSBTs,
        prop_id: u32,
        vote: Vote,
    ) -> Result<(), VoteError> {
        if tokens.is_empty() || tokens[0].1.is_empty() {
            return Err(VoteError::NoSBTs);
        }
        if tokens.len() != 1 {
            // in current version we support only one proof of personhood issuer: Fractal, so here
            // we simplify by requiring that the result contains tokens only from one issuer.
            return Err(VoteError::WrongIssuer);
        }
        let mut p = self._proposal(prop_id);
        p.vote_on_verified(&tokens[0].1, vote)?;
        self.proposals.insert(&prop_id, &p);
        Ok(())
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

    fn human_issuer() -> AccountId {
        AccountId::new_unchecked("h_isser.near".to_string())
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

    fn mk_human_sbt(sbt: TokenId) -> HumanSBTs {
        vec![(human_issuer(), vec![sbt])]
    }

    fn mk_human_sbts(sbt: Vec<TokenId>) -> HumanSBTs {
        vec![(human_issuer(), sbt)]
    }

    fn mk_nohuman_sbt(sbt: TokenId) -> HumanSBTs {
        vec![(human_issuer(), vec![sbt]), (admin(), vec![sbt])]
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
    fn on_vote_verified() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        let vote = vec![candidate(1)];
        ctx.block_timestamp = (START + 2) * MSECOND;
        testing_env!(ctx.clone());

        // check initial state
        let p = ctr._proposal(prop_id);
        assert_eq!(p.voters_num, 0, "voters num should be zero");
        assert_eq!(p.result, vec![0, 0, 0],);

        // successful vote
        match ctr.on_vote_verified(mk_human_sbt(1), prop_id, vote.clone()) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        let p = ctr._proposal(prop_id);
        assert!(p.voters.contains(&1));
        assert_eq!(p.voters_num, 1, "voters num should increment");
        assert_eq!(p.result, vec![1, 0, 0], "vote should be counted");

        // attempt double vote
        match ctr.on_vote_verified(mk_human_sbt(1), prop_id, vote.clone()) {
            Err(VoteError::DoubleVote(1)) => (),
            x => panic!("expected DoubleVote(1), got: {:?}", x),
        };
        assert_eq!(p.voters_num, 1, "voters num should not increment");
        assert_eq!(p.result, vec![1, 0, 0], "vote result should not change");

        //set sbt=4 and attempt double vote
        ctr._proposal(prop_id).voters.insert(&4);
        match ctr.on_vote_verified(mk_human_sbt(4), prop_id, vote.clone()) {
            Err(VoteError::DoubleVote(4)) => (),
            x => panic!("expected DoubleVote(4), got: {:?}", x),
        };
        assert_eq!(p.result, vec![1, 0, 0], "vote result should not change");

        // attempt to double vote with few tokens
        match ctr.on_vote_verified(mk_human_sbts(vec![2, 4]), prop_id, vote.clone()) {
            Err(VoteError::DoubleVote(4)) => (),
            x => panic!("expected DoubleVote(4), got: {:?}", x),
        };
        assert_eq!(p.result, vec![1, 0, 0], "vote result should not change");

        // wrong issuer
        match ctr.on_vote_verified(mk_nohuman_sbt(3), prop_id, vote.clone()) {
            Err(VoteError::WrongIssuer) => (),
            x => panic!("expected WrongIssuer, got: {:?}", x),
        };
        match ctr.on_vote_verified(vec![], prop_id, vote.clone()) {
            Err(VoteError::NoSBTs) => (),
            x => panic!("expected WrongIssuer, got: {:?}", x),
        };
        match ctr.on_vote_verified(vec![(human_issuer(), vec![])], prop_id, vote.clone()) {
            Err(VoteError::NoSBTs) => (),
            x => panic!("expected NoSBTs, got: {:?}", x),
        };
        assert_eq!(p.voters_num, 1, "voters num should not increment");
        assert_eq!(p.result, vec![1, 0, 0], "vote result should not change");

        //
        // Create more successful votes

        // alice, tokenID=20: successful vote with single selection
        ctx.predecessor_account_id = alice();
        testing_env!(ctx.clone());
        match ctr.on_vote_verified(mk_human_sbt(20), prop_id, vec![candidate(3)]) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        let p = ctr._proposal(prop_id);
        assert!(p.voters.contains(&20), "token id should be recorded");
        assert_eq!(p.voters_num, 2, "voters num should  increment");
        assert_eq!(p.result, vec![1, 0, 1], "vote should be counted");

        // bob, tokenID=22: vote with 2 selections
        ctx.predecessor_account_id = bob();
        testing_env!(ctx.clone());
        // candidates are put in non alphabetical order.
        match ctr.on_vote_verified(mk_human_sbt(22), prop_id, vec![candidate(3), candidate(2)]) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        let p = ctr._proposal(prop_id);
        assert!(p.voters.contains(&22), "token id should be recorded");
        assert_eq!(p.voters_num, 3, "voters num should  increment");
        assert_eq!(p.result, vec![1, 1, 2], "vote should be counted");
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
        // should not panic
        ctr.vote(prop_id, vec![]);
        // note: we can only check vote result and state change through an integration test.
    }
}
