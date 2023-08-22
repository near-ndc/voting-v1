use std::collections::HashSet;

use events::emit_vote;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, LookupSet};
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault, Promise, PromiseError, PromiseOrValue};
use near_sdk::json_types::U128;

mod constants;
mod errors;
mod events;
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
    pub prop_counter: u32,
    pub proposals: LookupMap<u32, Proposal>,
    pub accepted_policy: LookupMap<AccountId, [u8; 32]>,
    pub bonded: LookupMap<TokenId, u128>,
    pub total_slashed: u128,

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
            accepted_policy: LookupMap::new(StorageKey::AcceptedPolicy),
            bonded: LookupMap::new(StorageKey::Bonded),
            total_slashed: 0,
            prop_counter: 0,
        }
    }

    /*
     * Queries are in view.rs
     */

    /**********
     * TRANSACTIONS
     **********/

    /// Creates a new empty proposal. `start` and `end`are timestamps in milliseconds.
    /// * `policy` is a blake2s-256 hex-encoded hash of the Fair Voting Policy text.
    /// Returns the new proposal ID.
    /// NOTE: storage is paid from the account state
    pub fn create_proposal(
        &mut self,
        typ: ProposalType,
        start: u64,
        end: u64,
        cooldown: u64,
        ref_link: String,
        quorum: u32,
        seats: u16,
        #[allow(unused_mut)] mut candidates: Vec<AccountId>,
        policy: String,
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
        if typ == ProposalType::SetupPackage {
            require!(
                candidates.is_empty(),
                "setup_package candidates must be an empty list"
            );
            require!(seats == 0, "setup_package seats must be 0");
        }

        let policy = assert_hash_hex_string(&policy);

        let cs: HashSet<&AccountId> = HashSet::from_iter(candidates.iter());
        require!(cs.len() == candidates.len(), "duplicated candidates");
        candidates.sort();

        self.prop_counter += 1;
        let l = candidates.len();
        let p = Proposal {
            typ,
            start,
            end,
            cooldown,
            quorum,
            ref_link,
            seats,
            candidates,
            result: vec![0; l],
            voters: LookupSet::new(StorageKey::ProposalVoters(self.prop_counter)),
            voters_num: 0,
            voters_candidates: LookupMap::new(StorageKey::VotersCandidates(self.prop_counter)),
            policy,
        };

        self.proposals.insert(&self.prop_counter, &p);
        self.prop_counter
    }

    /// Transaction to record the predecessor account accepting the Fair Voting Policy.
    /// * `policy` is a blake2s-256 hex-encoded hash (must be 64 bytes) of the Fair Voting Policy text.
    /// User is required to pay bond amount in this step
    #[payable]
    pub fn accept_fair_voting_policy(&mut self, policy: String) -> Promise {
        require!(
            env::prepaid_gas() >= ACCEPT_POLICY_GAS,
            format!("not enough gas, min: {:?}", ACCEPT_POLICY_GAS)
        );
        // call SBT registry to check for graylist
        ext_sbtreg::ext(self.sbt_registry.clone())
            .is_gray(env::predecessor_account_id())
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(IS_GRAY_RESULT_CALLBACK)
                    .on_gray_list_result(env::predecessor_account_id(), policy, U128(env::attached_deposit())),
            )
    }

    /// Election vote using a seat-selection mechanism.
    /// For the `SetupPackage` proposal, vote must be an empty list.
    #[payable]
    pub fn vote(&mut self, prop_id: u32, vote: Vote) -> Promise {
        let user = env::predecessor_account_id();
        let p = self._proposal(prop_id);
        p.assert_active();
        if p.typ == ProposalType::SetupPackage {
            require!(vote.is_empty(), "setup_package vote must be an empty list");
        }
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
        require!(
            p.policy == self.accepted_policy.get(&user).unwrap_or_default(),
            "user didn't accept the voting policy, or the accepted voting policy doesn't match the required one"
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

    /// Method for the authority to revoke votes from blacklisted accounts.
    /// Panics if the proposal doesn't exists or the it's called before the proposal starts or after proposal `end+cooldown`.
    #[handle_result]
    pub fn revoke_vote(&mut self, prop_id: u32, token_id: TokenId) -> Result<(), VoteError> {
        // check if the caller is the authority allowed to revoke votes
        self.assert_admin();
        self.slash_bond(token_id);
        let mut p = self._proposal(prop_id);
        p.revoke_votes(token_id)?;
        self.proposals.insert(&prop_id, &p);
        Ok(())
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
        if !(tokens.len() == 1 && tokens[0].1.len() == 1) {
            // in current version we support only one proof of personhood issuer: Fractal, so here
            // we simplify by requiring that the result contains tokens only from one issuer.
            return Err(VoteError::WrongIssuer);
        }
        if !self.bonded.contains_key(&tokens[0].1.get(0).unwrap()) {
            return Err(VoteError::NoBond);
        }
        let mut p = self._proposal(prop_id);
        p.vote_on_verified(&tokens[0].1, vote)?;
        self.proposals.insert(&prop_id, &p);
        emit_vote(prop_id);
        Ok(())
    }

    #[private]
    pub fn on_gray_list_result(
        &mut self,
        #[callback_result] callback_result: Result<bool, PromiseError>,
        sender: AccountId,
        policy: String,
        deposit_amount: U128
    ) -> Promise {

        let result = callback_result
            .map_err(|e| format!("IAHRegistry::is_gray() call failure: {e:?}"))
            .and_then(|is_gray| {
                let bond_amount;
                if is_gray {
                    bond_amount = GRAY_BOND_AMOUNT;
                } else {
                    bond_amount = BOND_AMOUNT;
                }

                if deposit_amount < U128(ACCEPT_POLICY_COST + bond_amount) {
                    return Err(
                    format!(
                        "requires {} yocto deposit for bond amount and storage fees",
                        ACCEPT_POLICY_COST + bond_amount
                    ));
                }

                Ok( ext_sbtreg::ext(self.sbt_registry.clone())
                    .is_human(sender.clone())
                    .then(
                        ext_self::ext(env::current_account_id())
                            .with_static_gas(ACCEPT_POLICY_GAS_CALLBACK)
                            .on_gray_verified(sender.clone(), policy, deposit_amount, U128(bond_amount)),
                ))
            });

        result.unwrap_or_else(|e| {
            Promise::new(sender)
                .transfer(deposit_amount.0)
                .then(
                    Self::ext(env::current_account_id())
                        .with_static_gas(FAILURE_CALLBACK_GAS)
                        .on_failure(e),
                )
        })
    }

    #[private]
    pub fn on_gray_verified(
        &mut self,
        #[callback_result] callback_result: Result<Vec<(AccountId, Vec<TokenId>)>, PromiseError>,
        sender: AccountId,
        policy: String,
        deposit_amount: U128,
        bond_amount: U128,
    ) -> PromiseOrValue<TokenId> {
        let attached_deposit = deposit_amount.0;

        let result = callback_result
            .map_err(|e| format!("IAHRegistry::is_human() call failure: {e:?}"))
            .and_then(|tokens| {
                if tokens.is_empty() || !(tokens.len() == 1 && tokens[0].1.len() == 1) {
                    return Err("IAHRegistry::is_human() returns result: Not a human".to_owned());
                }

                let token_id = tokens[0].1.get(0).unwrap();
                let policy = assert_hash_hex_string(&policy);
                self.accepted_policy
                    .insert(&sender, &policy);

                self.bonded.insert(token_id, &bond_amount.0);

                Ok(token_id.clone())
            });

        match result {
        Ok(token) => PromiseOrValue::Value(token),
        Err(e) => {
            // Return deposit back to sender if accept policy failure
            Promise::new(sender)
                .transfer(attached_deposit)
                .then(
                    Self::ext(env::current_account_id())
                        .with_static_gas(FAILURE_CALLBACK_GAS)
                        .on_failure(format!("IAHRegistry::is_human(), Accept policy failure: {e:?}")),
                )
                .into()
        }
    }
    }

    #[private]
    pub fn on_failure(&mut self, error: String) {
        env::panic_str(&error)
    }

    /*****************
     * INTERNAL
     ****************/

    pub fn slash_bond(&mut self, token_id: TokenId) {
        let bond_amount = self.bonded.get(&token_id).expect("Bond doesn't exist");
        self.total_slashed += bond_amount;
        self.bonded.remove(&token_id);
    }

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

    fn policy1() -> String {
        "f1c09f8686fe7d0d798517111a66675da0012d8ad1693a47e0e2a7d3ae1c69d4".to_owned()
    }

    fn policy2() -> String {
        "21c09f8686fe7d0d798517111a66675da0012d8ad1693a47e0e2a7d3ae1c69d4".to_owned()
    }

    fn mk_proposal(ctr: &mut Contract) -> u32 {
        ctr.create_proposal(
            crate::ProposalType::HouseOfMerit,
            START + 1,
            START + 10,
            100,
            String::from("ref_link.io"),
            2,
            2,
            vec![candidate(1), candidate(2), candidate(3)],
            policy1(),
        )
    }

    fn mk_proposal_setup_package(ctr: &mut Contract) -> u32 {
        ctr.create_proposal(
            crate::ProposalType::SetupPackage,
            START + 1,
            START + 10,
            100,
            String::from("ref_link.io"),
            2,
            0,
            vec![],
            policy1(),
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

    fn alice_voting_context(ctx: &mut VMContext, ctr: &mut Contract) {
        ctx.predecessor_account_id = alice();
        ctx.attached_deposit = ACCEPT_POLICY_COST;
        testing_env!(ctx.clone());
        ctr.accept_fair_voting_policy(policy1());

        ctx.attached_deposit = VOTE_COST;
        ctx.block_timestamp = (START + 2) * MSECOND;
        ctx.prepaid_gas = VOTE_GAS;
        testing_env!(ctx.clone());
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
        (ctx, ctr)
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
            crate::ProposalType::HouseOfMerit,
            START - 1,
            START + 100,
            100,
            String::from("ref_link.io"),
            2,
            2,
            vec![candidate(1)],
            policy1(),
        );
    }

    #[test]
    #[should_panic(expected = "proposal start must be before end")]
    fn create_proposal_end_before_start() {
        let (_, mut ctr) = setup(&admin());

        ctr.create_proposal(
            crate::ProposalType::HouseOfMerit,
            START + 10,
            START,
            100,
            String::from("ref_link.io"),
            2,
            2,
            vec![candidate(1)],
            policy1(),
        );
    }

    #[test]
    #[should_panic(expected = "ref_link length must be between 6 and 120 bytes")]
    fn create_proposal_wrong_ref_link_length() {
        let (_, mut ctr) = setup(&admin());

        ctr.create_proposal(
            crate::ProposalType::HouseOfMerit,
            START + 1,
            START + 10,
            100,
            String::from("short"),
            2,
            2,
            vec![candidate(1)],
            policy1(),
        );
    }

    #[test]
    #[should_panic(expected = "duplicated candidates")]
    fn create_proposal_duplicated_candidates() {
        let (_, mut ctr) = setup(&admin());

        ctr.create_proposal(
            crate::ProposalType::HouseOfMerit,
            START + 1,
            START + 10,
            100,
            String::from("ref_link.io"),
            2,
            2,
            vec![candidate(1), candidate(1)],
            policy1(),
        );
    }

    #[test]
    #[should_panic(expected = "setup_package candidates must be an empty list")]
    fn create_proposal_setup_package() {
        let (_, mut ctr) = setup(&admin());

        let n = ctr.create_proposal(
            crate::ProposalType::SetupPackage,
            START + 1,
            START + 10,
            100,
            String::from("ref_link.io"),
            2,
            0,
            vec![],
            policy1(),
        );
        assert_eq!(n, 1);

        // this should fail because setup package requires candidates=[]
        ctr.create_proposal(
            crate::ProposalType::SetupPackage,
            START + 1,
            START + 10,
            100,
            String::from("ref_link.io"),
            2,
            2,
            vec![candidate(1)],
            policy1(),
        );
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

        let prop_id = mk_proposal_setup_package(&mut ctr);
        assert_eq!(prop_id, 3);
        assert_eq!(ctr.prop_counter, 3);
        assert!(ctr.proposals.contains_key(&prop_id));

        let proposals = ctr.proposals();
        assert_eq!(proposals.len(), 3);
        assert_eq!(proposals[0].id, 1);
        assert_eq!(proposals[1].id, 2);
        assert_eq!(proposals[2].id, 3);
    }

    #[test]
    fn vote_on_verified() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        let prop_sp = mk_proposal_setup_package(&mut ctr);
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
        match ctr.on_vote_verified(mk_human_sbts(vec![4]), prop_id, vote.clone()) {
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
        match ctr.on_vote_verified(vec![(human_issuer(), vec![])], prop_id, vote) {
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
        testing_env!(ctx);
        // candidates are put in non alphabetical order.
        match ctr.on_vote_verified(mk_human_sbt(22), prop_id, vec![candidate(3), candidate(2)]) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        let p = ctr._proposal(prop_id);
        assert!(p.voters.contains(&22), "token id should be recorded");
        assert_eq!(p.voters_num, 3, "voters num should  increment");
        assert_eq!(p.result, vec![1, 1, 2], "vote should be counted");

        // SetupPackage vote, again with bob
        match ctr.on_vote_verified(mk_human_sbt(22), prop_sp, vec![]) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        let p = ctr._proposal(prop_sp);
        assert!(p.voters.contains(&22), "token id should be recorded");
        assert_eq!(p.voters_num, 1, "voters num should  increment");
        assert!(p.result.is_empty(), "vote should be counted");
    }

    #[test]
    #[should_panic(expected = "requires 1000000000000000000000 yocto deposit for storage fees")]
    fn accepted_policy_deposit() {
        let (mut ctx, mut ctr) = setup(&admin());

        ctx.attached_deposit = ACCEPT_POLICY_COST / 2;
        testing_env!(ctx);

        ctr.accept_fair_voting_policy(policy1());
    }

    #[test]
    fn accepted_policy_deposit_ok() {
        let (mut ctx, mut ctr) = setup(&admin());

        ctx.attached_deposit = ACCEPT_POLICY_COST;
        testing_env!(ctx);

        ctr.accept_fair_voting_policy(policy1());
        // should be able to accept more then once
        ctr.accept_fair_voting_policy(policy1());
    }

    #[test]
    fn accepted_policy_query() {
        let (mut ctx, mut ctr) = setup(&admin());

        let mut res = ctr.accepted_policy(admin());
        assert!(res.is_none());
        ctx.attached_deposit = ACCEPT_POLICY_COST;
        testing_env!(ctx.clone());
        ctr.accept_fair_voting_policy(policy1());
        res = ctr.accepted_policy(admin());
        assert!(res.is_some());
        assert_eq!(res.unwrap(), policy1());
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
    #[should_panic(expected = "not enough gas, min: Gas(70000000000000)")]
    fn vote_wrong_gas() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        ctx.attached_deposit = VOTE_COST;
        ctx.block_timestamp = (START + 2) * MSECOND;
        ctx.prepaid_gas = Gas(10 * Gas::ONE_TERA.0);
        testing_env!(ctx);

        ctr.vote(prop_id, vec![candidate(1)]);
    }

    #[test]
    #[should_panic(
        expected = "requires 500000000000000000000 yocto deposit for storage fees for every new vote"
    )]
    fn vote_wrong_deposit() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        ctx.attached_deposit = VOTE_COST - 1;
        ctx.block_timestamp = (START + 2) * MSECOND;
        testing_env!(ctx);

        ctr.vote(prop_id, vec![candidate(1)]);
    }

    #[test]
    #[should_panic(
        expected = "user didn't accept the voting policy, or the accepted voting policy doesn't match the required one"
    )]
    fn vote_not_accepted_policy() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        ctx.attached_deposit = VOTE_COST;
        ctx.block_timestamp = (START + 2) * MSECOND;
        ctx.prepaid_gas = VOTE_GAS;
        testing_env!(ctx);

        ctr.vote(prop_id, vec![candidate(1)]);
    }

    #[test]
    #[should_panic(
        expected = "user didn't accept the voting policy, or the accepted voting policy doesn't match the required one"
    )]
    fn vote_wrong_accepted_policy() {
        let (mut ctx, mut ctr) = setup(&admin());

        ctx.attached_deposit = ACCEPT_POLICY_COST;
        testing_env!(ctx.clone());
        ctr.accept_fair_voting_policy(policy2());

        let prop_id = mk_proposal(&mut ctr);
        ctx.attached_deposit = VOTE_COST;
        ctx.block_timestamp = (START + 2) * MSECOND;
        ctx.prepaid_gas = VOTE_GAS;
        testing_env!(ctx);

        ctr.vote(prop_id, vec![candidate(1)]);
    }

    #[test]
    fn proposal_status_query() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        let mut res = ctr.proposal_status(prop_id);
        assert_eq!(res, Some(ProposalStatus::NOT_STARTED));

        ctx.block_timestamp = (START + 2) * MSECOND;
        testing_env!(ctx.clone());

        res = ctr.proposal_status(prop_id);
        assert_eq!(res, Some(ProposalStatus::ONGOING));

        ctx.block_timestamp = (START + 11) * MSECOND;
        testing_env!(ctx.clone());

        res = ctr.proposal_status(prop_id);
        assert_eq!(res, Some(ProposalStatus::COOLDOWN));

        ctx.block_timestamp = (START + 111) * MSECOND;
        testing_env!(ctx.clone());

        res = ctr.proposal_status(prop_id);
        assert_eq!(res, Some(ProposalStatus::ENDED));
    }

    #[test]
    #[should_panic(expected = "double vote for the same candidate")]
    fn vote_double_vote_same_candidate() {
        let (mut ctx, mut ctr) = setup(&admin());

        ctx.attached_deposit = ACCEPT_POLICY_COST;
        testing_env!(ctx.clone());
        ctr.accept_fair_voting_policy(policy1());

        let prop_id = mk_proposal(&mut ctr);
        ctx.attached_deposit = VOTE_COST;
        ctx.block_timestamp = (START + 2) * MSECOND;
        ctx.prepaid_gas = VOTE_GAS;
        testing_env!(ctx);

        ctr.vote(prop_id, vec![candidate(1), candidate(1)]);
    }

    #[test]
    #[should_panic(expected = "vote for unknown candidate")]
    fn vote_unknown_candidate() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        alice_voting_context(&mut ctx, &mut ctr);
        ctr.vote(prop_id, vec![bob()]);
    }

    #[test]
    #[should_panic(expected = "max vote is 2 seats")]
    fn vote_too_many_selections() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        alice_voting_context(&mut ctx, &mut ctr);
        ctr.vote(prop_id, vec![candidate(1), candidate(2), candidate(3)]);
    }

    #[test]
    fn vote_empty_vote() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        alice_voting_context(&mut ctx, &mut ctr);
        // should not panic
        ctr.vote(prop_id, vec![]);
        // note: we can only check vote result and state change through an integration test.
    }

    #[test]
    #[should_panic(expected = "setup_package vote must be an empty list")]
    fn vote_wrong_setup_package_vote() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal_setup_package(&mut ctr);
        alice_voting_context(&mut ctx, &mut ctr);
        ctr.vote(prop_id, vec![candidate(1)]);
    }

    #[test]
    fn vote_valid() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_sp = mk_proposal_setup_package(&mut ctr);
        let prop_hom1 = mk_proposal(&mut ctr);
        let prop_hom2 = mk_proposal(&mut ctr);
        alice_voting_context(&mut ctx, &mut ctr);

        ctr.vote(prop_sp, vec![]);
        ctr.vote(prop_hom1, vec![]);
        // need to setup new context, otherwise we have a gas error
        alice_voting_context(&mut ctx, &mut ctr);
        ctr.vote(prop_hom2, vec![candidate(2), candidate(1)]);
    }

    #[test]
    #[should_panic(expected = "not an admin")]
    fn revoke_vote_not_admin() {
        let (_, mut ctr) = setup(&alice());
        let prop_id = mk_proposal(&mut ctr);
        let res = ctr.revoke_vote(prop_id, 1);
        // this will never be checked since the method is panicing not returning an error
        assert!(res.is_err());
    }

    #[test]
    fn revoke_vote_no_votes() {
        let (mut ctx, mut ctr) = setup(&admin());
        let prop_id = mk_proposal(&mut ctr);
        ctx.block_timestamp = (START + 100) * MSECOND;
        testing_env!(ctx);
        match ctr.revoke_vote(prop_id, 1) {
            Err(VoteError::NotVoted) => (),
            x => panic!("expected NotVoted, got: {:?}", x),
        }
    }

    #[test]
    #[should_panic(expected = "proposal not found")]
    fn revoke_vote_no_proposal() {
        let (_, mut ctr) = setup(&admin());
        let prop_id = 2;
        match ctr.revoke_vote(prop_id, 1) {
            x => panic!("{:?}", x),
        }
    }
}
