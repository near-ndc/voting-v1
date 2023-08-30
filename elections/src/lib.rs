use std::cmp::max;

use events::{emit_bond, emit_revoke_vote, emit_vote};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault, Promise, PromiseOrValue};

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

    /// blake2s-256 hash of the Fair Voting Policy text.
    pub policy: [u8; 32],
    pub accepted_policy: LookupMap<AccountId, [u8; 32]>,
    /// we assume that each account has at most one IAH token.
    pub bonded_amounts: LookupMap<TokenId, u128>,
    /// total amount of near slashed due to violating the fair voting policy
    pub total_slashed: u128,
    /// Finish time is end + cooldown. This used in the `unbond` function: user can unbond only after this time.
    /// Unix timestamp (in milliseconds)
    pub finish_time: u64,

    /// address which can pause the contract and make a new proposal. Should be a multisig / DAO;
    pub authority: AccountId,
    pub sbt_registry: AccountId,
}

#[near_bindgen]
impl Contract {
    #[init]
    /// * `policy` is a blake2s-256 hex-encoded hash of the Fair Voting Policy text.
    pub fn new(
        authority: AccountId,
        sbt_registry: AccountId,
        policy: String,
        finish_time: u64,
    ) -> Self {
        let policy = assert_hash_hex_string(&policy);

        Self {
            pause: false,
            authority,
            sbt_registry,
            proposals: LookupMap::new(StorageKey::Proposals),
            accepted_policy: LookupMap::new(StorageKey::AcceptedPolicy),
            bonded_amounts: LookupMap::new(StorageKey::BondedAmount),
            total_slashed: 0,
            prop_counter: 0,
            policy,
            finish_time,
        }
    }

    /*
     * Queries are in view.rs
     */

    /**********
     * TRANSACTIONS
     **********/

    /// Creates a new empty proposal. `start` and `end`are timestamps in milliseconds.
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
        min_candidate_support: u64,
    ) -> u32 {
        self.assert_admin();
        let candidates_len = candidates.len();
        require!(
            env::block_timestamp_ms() < start,
            "proposal start must be in the future"
        );
        require!(start < end, "proposal start must be before end");
        require!(
            0 < seats && seats <= candidates_len as u16,
            "require 0 < seats <= candidates.length"
        );
        require!(
            MIN_REF_LINK_LEN <= ref_link.len() && ref_link.len() <= MAX_REF_LINK_LEN,
            format!(
                "ref_link length must be between {} and {} bytes",
                MIN_REF_LINK_LEN, MAX_REF_LINK_LEN
            )
        );

        if typ == ProposalType::SetupPackage {
            validate_setup_package(seats, &candidates);
        }

        candidates.sort();
        let mut c1 = &candidates[0];
        for c in candidates.iter().skip(1) {
            require!(c1 != c, "duplicated candidates");
            c1 = c;
        }

        self.prop_counter += 1;
        let p = Proposal {
            typ,
            start,
            end,
            cooldown,
            quorum,
            ref_link,
            seats,
            candidates,
            result: vec![0; candidates_len],
            voters: LookupMap::new(StorageKey::ProposalVoters(self.prop_counter)),
            voters_num: 0,
            min_candidate_support,
            user_sbt: LookupMap::new(StorageKey::UserSBT(self.prop_counter)),
        };

        self.finish_time = max(self.finish_time, end + cooldown);
        self.proposals.insert(&self.prop_counter, &p);
        self.prop_counter
    }

    /// Transaction to record the predecessor account accepting the Fair Voting Policy.
    /// * `policy` is a blake2s-256 hex-encoded hash (must be 64 bytes) of the Fair Voting Policy text.
    #[payable]
    pub fn accept_fair_voting_policy(&mut self, policy: String) {
        require!(
            env::attached_deposit() >= ACCEPT_POLICY_COST,
            format!(
                "requires {} yocto deposit for storage fees",
                ACCEPT_POLICY_COST
            )
        );
        let policy = assert_hash_hex_string(&policy);
        self.accepted_policy
            .insert(&env::predecessor_account_id(), &policy);
    }

    /// Election vote using a seat-selection mechanism.
    /// For the `SetupPackage` proposal, vote must be an empty list.
    /// NOTE: we don't need to take storage deposit because user is required to bond at least
    /// 3N, that will way more than what's needed to vote for few proposals.
    pub fn vote(&mut self, prop_id: u32, vote: Vote) -> Promise {
        let user = env::predecessor_account_id();
        let p = self._proposal(prop_id);
        p.assert_active();
        require!(
            env::prepaid_gas() >= VOTE_GAS,
            format!("not enough gas, min: {:?}", VOTE_GAS)
        );
        require!(
            self.policy == self.accepted_policy.get(&user).unwrap_or_default(),
            "user didn't accept the voting policy, or the accepted voting policy doesn't match the required one"
        );

        validate_vote(p.typ, &vote, p.seats, &p.candidates);
        // call SBT registry to verify SBT
        let sbt_promise = ext_sbtreg::ext(self.sbt_registry.clone()).is_human(user.clone());
        let acc_flag = ext_sbtreg::ext(self.sbt_registry.clone()).account_flagged(user.clone());

        sbt_promise.and(acc_flag).then(
            ext_self::ext(env::current_account_id())
                .with_static_gas(VOTE_GAS_CALLBACK)
                .on_vote_verified(prop_id, user, vote),
        )
    }

    /// Allows user to bond before voting. The method needs to be called through registry.is_human_call
    /// Panics if the caller is not registry
    /// Emits bond event
    #[payable]
    pub fn bond(
        &mut self,
        caller: AccountId,
        iah_proof: HumanSBTs,
        #[allow(unused_variables)] payload: serde_json::Value, // required by is_human_call
    ) -> PromiseOrValue<U128> {
        let deposit = env::attached_deposit();
        if env::predecessor_account_id() != self.sbt_registry {
            return PromiseOrValue::Promise(
                Promise::new(caller)
                    .transfer(deposit)
                    .then(Self::fail("can only be called by registry")),
            );
        }

        let (ok, token_id) = Self::is_human_issuer(&iah_proof);
        if !ok {
            return PromiseOrValue::Promise(
                Promise::new(caller)
                    .transfer(deposit)
                    .then(Self::fail("not a human")),
            );
        }

        emit_bond(deposit);
        self.bonded_amounts.insert(&token_id, &deposit);
        PromiseOrValue::Value(U128(deposit))
    }

    /// Allows user to unbond after the elections is over.
    /// Can only be called using registry.is_human_call
    /// Panics if the caller is not registry
    /// Panics if called before the elections is over
    /// Panics if user didn't bond
    #[payable]
    #[allow(unused_variables)] // `payload` is not used but it needs to be payload so that is_human_call works
    pub fn unbond(
        &mut self,
        caller: AccountId,
        iah_proof: HumanSBTs,
        payload: serde_json::Value,
    ) -> Promise {
        if env::predecessor_account_id() != self.sbt_registry {
            return Self::fail("can only be called by registry");
        }

        let (ok, token_id) = Self::is_human_issuer(&iah_proof);
        if !ok {
            return Self::fail("not a human");
        }
        if env::block_timestamp_ms() <= self.finish_time {
            return Self::fail("cannot unbond: election is still in progress");
        }

        let mut voted_for_all = true;

        // cleanup votes, policy data from caller
        for i in 1..=self.prop_counter {
            let proposal = self.proposals.get(&i);
            if let Some(mut prop) = proposal {
                prop.user_sbt.remove(&caller);
                if prop.voters.remove(&token_id).is_none() {
                    voted_for_all = false;
                }
            }
        }
        self.accepted_policy.remove(&caller);

        let mut unbond_amount = self
            .bonded_amounts
            .remove(&token_id)
            .expect("voter didn't bond");

        // call to registry to mint `I Voted` SBT
        if voted_for_all {
            unbond_amount -= MINT_COST;
            Promise::new(caller.clone()).transfer(unbond_amount);
            ext_sbtreg::ext(self.sbt_registry.clone())
                .with_static_gas(MINT_GAS)
                .with_attached_deposit(MINT_COST)
                .sbt_mint(vec![(
                    caller,
                    vec![TokenMetadata {
                        class: I_VOTED_SBT_CLASS,
                        issued_at: None,
                        expires_at: None,
                        reference: None,
                        reference_hash: None,
                    }],
                )])
        } else {
            Promise::new(caller.clone()).transfer(unbond_amount)
        }
    }

    /// Method for the authority to revoke any votes
    /// Panics if the proposal doesn't exists or the it's called before the proposal starts or after proposal `end+cooldown`.
    #[handle_result]
    pub fn admin_revoke_vote(
        &mut self,
        prop_id: u32,
        token_id: TokenId,
    ) -> Result<(), RevokeVoteError> {
        // check if the caller is the authority allowed to revoke votes
        self.assert_admin();
        self.slash_bond(token_id);
        let mut p = self._proposal(prop_id);
        p.revoke_votes(token_id)?;
        self.proposals.insert(&prop_id, &p);
        emit_revoke_vote(prop_id);
        Ok(())
    }

    /// Method to revoke votes from blacklisted accounts.
    /// The method makes a call to the registry to verify the user is blacklisted.
    /// Panics if:
    /// - the proposal doesn't exists
    /// - it's called before the proposal starts or after proposal `end+cooldown`
    /// - the user is not blacklisted
    pub fn revoke_vote(&mut self, prop_id: u32, user: AccountId) -> Promise {
        // call SBT registry to verify user is blacklisted
        ext_sbtreg::ext(self.sbt_registry.clone())
            .account_flagged(user.clone())
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(REVOKE_VOTE_GAS_CALLBACK)
                    .on_revoke_verified(prop_id, user),
            )
    }

    /*****************
     * PRIVATE
     ****************/

    #[private]
    #[handle_result]
    pub fn on_vote_verified(
        &mut self,
        #[callback_unwrap] iah_proof: HumanSBTs,
        #[callback_unwrap] account_flag: Option<AccountFlag>,
        prop_id: u32,
        voter: AccountId,
        vote: Vote,
    ) -> Result<(), VoteError> {
        let (ok, token_id) = Self::is_human_issuer(&iah_proof);
        if !ok {
            return Err(VoteError::NoSBTs);
        }

        let required_bond = match account_flag {
            Some(AccountFlag::Blacklisted) => return Err(VoteError::Blacklisted),
            Some(AccountFlag::Verified) => BOND_AMOUNT,
            None => GRAY_BOND_AMOUNT,
        };

        if let Some(bond) = self.bonded_amounts.get(&token_id) {
            if bond < required_bond {
                return Err(VoteError::MinBond(required_bond, bond));
            }
        } else {
            return Err(VoteError::NoBond);
        }

        let mut p = self._proposal(prop_id);
        p.vote_on_verified(&iah_proof[0].1, voter, vote)?;
        self.proposals.insert(&prop_id, &p);
        emit_vote(prop_id);
        Ok(())
    }

    #[private]
    pub fn on_failure(&mut self, error: String) {
        env::panic_str(&error)
    }

    #[handle_result]
    pub fn on_revoke_verified(
        &mut self,
        #[callback_unwrap] flag: AccountFlag,
        prop_id: u32,
        user: AccountId,
    ) -> Result<(), RevokeVoteError> {
        if flag != AccountFlag::Blacklisted {
            return Err(RevokeVoteError::NotBlacklisted);
        }
        let mut p = self._proposal(prop_id);
        let token_id = p.user_sbt.get(&user).ok_or(RevokeVoteError::NotVoted)?;

        p.revoke_votes(token_id)?;
        self.proposals.insert(&prop_id, &p);
        emit_revoke_vote(prop_id);
        Ok(())
    }

    /*****************
     * INTERNAL
     ****************/

    #[private]
    pub fn slash_bond(&mut self, token_id: TokenId) {
        let bond_amount = self.bonded_amounts.remove(&token_id);
        if let Some(value) = bond_amount {
            self.total_slashed += value;
        }
    }

    fn fail(reason: &str) -> Promise {
        Self::ext(env::current_account_id())
            .with_static_gas(FAILURE_CALLBACK_GAS)
            .on_failure(reason.to_string())
    }

    #[inline]
    fn is_human_issuer(iah_proof: &HumanSBTs) -> (bool, TokenId) {
        // in current version we support only one proof of personhood issuer: Fractal, so here
        // we simplify by requiring that the result contains tokens only from one issuer.
        if iah_proof.is_empty() || !(iah_proof.len() == 1 && iah_proof[0].1.len() == 1) {
            (false, 0)
        } else {
            (true, *iah_proof[0].1.first().unwrap())
        }
    }

    #[inline]
    fn assert_admin(&self) {
        require!(
            self.authority == env::predecessor_account_id(),
            "not an admin"
        );
    }
}

fn validate_setup_package(seats: u16, cs: &Vec<AccountId>) {
    // Users can vote to at most one option
    require!(seats == 1, "SetupPackage seats must equal 1");
    require!(
        cs.len() == 3
            && cs[0].as_str() == "yes"
            && cs[1].as_str() == "no"
            && cs[2].as_str() == "abstain",
        "SetupPackage candidates must be ['yes', 'no', 'abstain']"
    );
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod unit_tests {
    use near_sdk::{
        test_utils::{self, VMContextBuilder},
        testing_env, Gas, VMContext,
    };
    use serde_json::Value;

    use crate::*;

    /// 1ms in nano seconds
    const MSECOND: u64 = 1_000_000;
    const START: u64 = 10;

    fn setup_package_candidates() -> Vec<AccountId> {
        vec![
            AccountId::try_from("yes".to_string()).unwrap(),
            AccountId::try_from("no".to_string()).unwrap(),
            AccountId::try_from("abstain".to_string()).unwrap(),
        ]
    }

    const ALICE_SBT: u64 = 1;

    fn alice() -> AccountId {
        AccountId::new_unchecked("alice.near".to_string())
    }

    fn bob() -> AccountId {
        AccountId::new_unchecked("bob.near".to_string())
    }

    fn charlie() -> AccountId {
        AccountId::new_unchecked("elon.near".to_string())
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

    fn bond_amount_call(ctx: &mut VMContext, ctr: &mut Contract, user: AccountId, token_id: u64) {
        let temp_attached = ctx.attached_deposit;
        let temp_caller = ctx.predecessor_account_id.clone();
        ctx.attached_deposit = BOND_AMOUNT;
        ctx.predecessor_account_id = sbt_registry();
        testing_env!(ctx.clone());

        ctr.bond(user, mk_human_sbt(token_id), Value::String("".to_string()));

        ctx.predecessor_account_id = temp_caller;
        ctx.attached_deposit = temp_attached;
        testing_env!(ctx.clone());
    }

    fn mock_proposal_and_votes(ctx: &mut VMContext, ctr: &mut Contract) -> u32 {
        let mut candidates = Vec::new();
        for idx in 0..100 {
            candidates.push(candidate(idx));
        }

        let prop_id = ctr.create_proposal(
            crate::ProposalType::HouseOfMerit,
            START + 1,
            START + 10,
            100,
            String::from("ref_link.io"),
            10,
            5,
            candidates,
            6,
        );
        ctx.block_timestamp = (START + 2) * MSECOND;
        testing_env!(ctx.clone());

        let vote1 = vec![candidate(1), candidate(2), candidate(3)];
        let vote2 = vec![candidate(2), candidate(3), candidate(4)];
        let vote3 = vec![candidate(3), candidate(4), candidate(5)];
        let mut current_vote = &vote1;

        for i in 1..=15u32 {
            if i == 6 {
                current_vote = &vote2;
            } else if i == 11 {
                current_vote = &vote3;
            }

            bond_amount_call(ctx, ctr, candidate(i), i as u64);

            match ctr.on_vote_verified(
                mk_human_sbt(i as u64),
                Some(AccountFlag::Verified),
                prop_id,
                candidate(i),
                current_vote.to_vec(),
            ) {
                Ok(_) => (),
                x => panic!("expected OK, got: {:?}", x),
            };
        }
        prop_id
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
            2,
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
            1,
            setup_package_candidates(),
            2,
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

        bond_amount_call(ctx, ctr, alice(), ALICE_SBT);

        ctx.attached_deposit = 0;
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
        let ctr = Contract::new(admin(), sbt_registry(), policy1(), 1);
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
            2,
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
            2,
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
            1,
            vec![candidate(1)],
            2,
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
            2,
        );
    }

    #[test]
    #[should_panic(expected = "require 0 < seats <= candidates.length")]
    fn create_proposal_zero_seats() {
        let (_, mut ctr) = setup(&admin());
        ctr.create_proposal(
            crate::ProposalType::HouseOfMerit,
            START + 1,
            START + 10,
            100,
            String::from("ref_link.io"),
            2,
            0,
            vec![candidate(1), candidate(1)],
            1,
        );
    }

    #[test]
    #[should_panic(expected = "require 0 < seats <= candidates.length")]
    fn create_proposal_not_enough_candidates() {
        let (_, mut ctr) = setup(&admin());
        ctr.create_proposal(
            crate::ProposalType::HouseOfMerit,
            START + 1,
            START + 10,
            100,
            String::from("ref_link.io"),
            2,
            3,
            vec![candidate(1), candidate(1)],
            1,
        );
    }

    #[test]
    #[should_panic(expected = "SetupPackage seats must equal 1")]
    fn create_proposal_setup_package_wrong_seats() {
        let (_, mut ctr) = setup(&admin());
        ctr.create_proposal(
            crate::ProposalType::SetupPackage,
            START + 1,
            START + 10,
            100,
            String::from("ref_link.io"),
            2,
            2,
            setup_package_candidates(),
            2,
        );
    }

    #[test]
    #[should_panic(expected = "SetupPackage candidates must be ['yes', 'no', 'abstain']")]
    fn create_proposal_setup_package_wrong_candidates() {
        let (_, mut ctr) = setup(&admin());
        ctr.create_proposal(
            crate::ProposalType::SetupPackage,
            START + 1,
            START + 10,
            100,
            String::from("ref_link.io"),
            2,
            1,
            setup_package_candidates()[..=1].to_vec(),
            2,
        );
    }

    #[test]
    #[should_panic(expected = "SetupPackage candidates must be ['yes', 'no', 'abstain']")]
    fn create_proposal_setup_package_wrong_candidates2() {
        let (_, mut ctr) = setup(&admin());
        ctr.create_proposal(
            crate::ProposalType::SetupPackage,
            START + 1,
            START + 10,
            100,
            String::from("ref_link.io"),
            2,
            1,
            vec![candidate(1), candidate(2), candidate(3)],
            2,
        );
    }

    #[test]
    #[should_panic(expected = "SetupPackage candidates must be ['yes', 'no', 'abstain']")]
    fn create_proposal_setup_package_wrong_candidates3() {
        let (_, mut ctr) = setup(&admin());
        let mut cs = setup_package_candidates();
        cs.push("no2".to_string().try_into().unwrap());
        ctr.create_proposal(
            crate::ProposalType::SetupPackage,
            START + 1,
            START + 10,
            100,
            String::from("ref_link.io"),
            2,
            1,
            cs,
            2,
        );
    }

    #[test]
    #[should_panic(expected = "SetupPackage candidates must be ['yes', 'no', 'abstain']")]
    fn create_proposal_setup_package_wrong_candidates_order() {
        let (_, mut ctr) = setup(&admin());
        let mut cs = setup_package_candidates();
        let c = cs[0].clone();
        cs[0] = cs[1].clone();
        cs[1] = c;
        ctr.create_proposal(
            crate::ProposalType::SetupPackage,
            START + 1,
            START + 10,
            100,
            String::from("ref_link.io"),
            2,
            1,
            cs,
            2,
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
        ctx.attached_deposit = BOND_AMOUNT;
        ctx.predecessor_account_id = sbt_registry();
        testing_env!(ctx.clone());
        ctr.bond(alice(), mk_human_sbt(1), Value::String("".to_string()));
        assert_eq!(
            test_utils::get_logs(),
            vec![
                r#"EVENT_JSON:{"standard":"ndc-elections","version":"1.0.0","event":"bond","data":{"amount":"3000000000000000000000000"}}"#
            ]
        );

        // check initial state
        let p = ctr._proposal(prop_id);
        assert_eq!(p.voters_num, 0, "voters num should be zero");
        assert_eq!(p.result, vec![0, 0, 0],);

        // successful vote
        match ctr.on_vote_verified(
            mk_human_sbt(1),
            Some(AccountFlag::Verified),
            prop_id,
            alice(),
            vote.clone(),
        ) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        let p = ctr._proposal(prop_id);
        assert!(p.voters.contains_key(&1));
        assert_eq!(p.voters_num, 1, "voters num should increment");
        assert_eq!(p.result, vec![1, 0, 0], "vote should be counted");
        assert!(p.user_sbt.contains_key(&alice()));

        // attempt double vote
        match ctr.on_vote_verified(
            mk_human_sbt(1),
            Some(AccountFlag::Verified),
            prop_id,
            alice(),
            vote.clone(),
        ) {
            Err(VoteError::DoubleVote(1)) => (),
            x => panic!("expected DoubleVote(1), got: {:?}", x),
        };
        assert_eq!(p.voters_num, 1, "voters num should not increment");
        assert_eq!(p.result, vec![1, 0, 0], "vote result should not change");

        //set sbt=4 and attempt double vote
        ctr._proposal(prop_id).voters.insert(&4, &vec![1]);
        ctr.bond(alice(), mk_human_sbt(4), Value::String("".to_string()));
        match ctr.on_vote_verified(
            mk_human_sbt(4),
            Some(AccountFlag::Verified),
            prop_id,
            alice(),
            vote.clone(),
        ) {
            Err(VoteError::DoubleVote(4)) => (),
            x => panic!("expected DoubleVote(4), got: {:?}", x),
        };
        assert_eq!(p.result, vec![1, 0, 0], "vote result should not change");

        // attempt to double vote with few tokens
        match ctr.on_vote_verified(
            mk_human_sbts(vec![4]),
            Some(AccountFlag::Verified),
            prop_id,
            alice(),
            vote.clone(),
        ) {
            Err(VoteError::DoubleVote(4)) => (),
            x => panic!("expected DoubleVote(4), got: {:?}", x),
        };
        assert_eq!(p.result, vec![1, 0, 0], "vote result should not change");

        // not a human
        match ctr.on_vote_verified(
            mk_nohuman_sbt(3),
            Some(AccountFlag::Verified),
            prop_id,
            alice(),
            vote.clone(),
        ) {
            Err(VoteError::NoSBTs) => (),
            x => panic!("expected WrongIssuer, got: {:?}", x),
        };
        match ctr.on_vote_verified(
            vec![],
            Some(AccountFlag::Verified),
            prop_id,
            alice(),
            vote.clone(),
        ) {
            Err(VoteError::NoSBTs) => (),
            x => panic!("expected WrongIssuer, got: {:?}", x),
        };
        match ctr.on_vote_verified(
            vec![(human_issuer(), vec![])],
            Some(AccountFlag::Verified),
            prop_id,
            alice(),
            vote,
        ) {
            Err(VoteError::NoSBTs) => (),
            x => panic!("expected NoSBTs, got: {:?}", x),
        };
        assert_eq!(p.voters_num, 1, "voters num should not increment");
        assert_eq!(p.result, vec![1, 0, 0], "vote result should not change");

        //
        // Create more successful votes

        // bob, tokenID=20: successful vote with single selection
        ctr.bond(bob(), mk_human_sbt(20), Value::String("".to_string()));
        ctr.bond(charlie(), mk_human_sbt(22), Value::String("".to_string()));
        ctx.predecessor_account_id = alice();
        testing_env!(ctx.clone());

        match ctr.on_vote_verified(
            mk_human_sbt(20),
            Some(AccountFlag::Verified),
            prop_id,
            bob(),
            vec![candidate(3)],
        ) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        let p = ctr._proposal(prop_id);
        assert!(p.voters.contains_key(&20), "token id should be recorded");
        assert_eq!(p.voters_num, 2, "voters num should  increment");
        assert_eq!(p.result, vec![1, 0, 1], "vote should be counted");
        assert!(
            p.user_sbt.contains_key(&bob()),
            "user and its sbt should be recorded"
        );

        // charlie, tokenID=22: vote with 2 selections
        ctx.predecessor_account_id = bob();
        testing_env!(ctx);

        // candidates are put in non alphabetical order.
        match ctr.on_vote_verified(
            mk_human_sbt(22),
            Some(AccountFlag::Verified),
            prop_id,
            charlie(),
            vec![candidate(3), candidate(2)],
        ) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        let p = ctr._proposal(prop_id);
        assert!(p.voters.contains_key(&22), "token id should be recorded");
        assert_eq!(p.voters_num, 3, "voters num should  increment");
        assert_eq!(p.result, vec![1, 1, 2], "vote should be counted");
        assert!(
            p.user_sbt.contains_key(&charlie()),
            "user and its sbt should be recorded"
        );

        // SetupPackage vote, again with charlie
        match ctr.on_vote_verified(
            mk_human_sbt(22),
            Some(AccountFlag::Verified),
            prop_sp,
            charlie(),
            setup_package_candidates()[1..=1].to_vec(),
        ) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        let p = ctr._proposal(prop_sp);
        assert!(p.voters.contains_key(&22), "token id should be recorded");
        assert_eq!(p.voters_num, 1, "voters num should  increment");
        assert_eq!(p.result, vec![0, 1, 0], "vote should be counted");
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

        let mut res = ctr.accepted_policy(admin());
        assert!(res.is_none());

        ctx.attached_deposit = ACCEPT_POLICY_COST;
        testing_env!(ctx);
        ctr.accept_fair_voting_policy(policy1());
        // should be able to accept more then once
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
    #[should_panic(expected = "not enough gas, min: Gas(110000000000000)")]
    fn vote_wrong_gas() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        ctx.block_timestamp = (START + 2) * MSECOND;
        ctx.prepaid_gas = Gas(10 * Gas::ONE_TERA.0);
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
        ctx.attached_deposit = 0;
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
        testing_env!(ctx);

        res = ctr.proposal_status(prop_id);
        assert_eq!(res, Some(ProposalStatus::ENDED));
    }

    #[test]
    #[should_panic(expected = "double vote for the same option")]
    fn vote_double_vote_same_candidate() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        alice_voting_context(&mut ctx, &mut ctr);
        ctr.vote(prop_id, vec![candidate(1), candidate(1)]);
    }

    #[test]
    #[should_panic(expected = "vote for unknown option")]
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
    #[should_panic(expected = "setup package vote must be non empty")]
    fn vote_empty_vote_setup_package() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal_setup_package(&mut ctr);
        alice_voting_context(&mut ctx, &mut ctr);
        ctr.vote(prop_id, vec![]);
    }

    #[test]
    #[should_panic(expected = "vote for unknown option")]
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

        ctr.vote(prop_sp, setup_package_candidates()[0..=0].to_vec());
        ctr.vote(prop_hom1, vec![]);
        // need to setup new context, otherwise we have a gas error
        alice_voting_context(&mut ctx, &mut ctr);
        ctr.vote(prop_hom2, vec![candidate(2), candidate(1)]);
    }

    #[test]
    #[should_panic(expected = "not an admin")]
    fn admin_revoke_vote_not_admin() {
        let (_, mut ctr) = setup(&alice());
        let prop_id = mk_proposal(&mut ctr);
        let res = ctr.admin_revoke_vote(prop_id, 1);
        // this will never be checked since the method is panicing not returning an error
        assert!(res.is_err());
    }

    #[test]
    fn admin_revoke_vote_no_votes() {
        let (mut ctx, mut ctr) = setup(&admin());
        let prop_id = mk_proposal(&mut ctr);
        ctx.block_timestamp = (START + 100) * MSECOND;
        testing_env!(ctx);
        match ctr.admin_revoke_vote(prop_id, 1) {
            Err(RevokeVoteError::NotVoted) => (),
            x => panic!("expected NotVoted, got: {:?}", x),
        }
        assert!(test_utils::get_logs().is_empty());
    }

    #[test]
    #[should_panic(expected = "proposal not found")]
    fn admin_revoke_vote_no_proposal() {
        let (_, mut ctr) = setup(&admin());
        let prop_id = 2;
        match ctr.admin_revoke_vote(prop_id, 1) {
            x => panic!("{:?}", x),
        }
    }

    #[test]
    fn user_votes() {
        let (mut ctx, mut ctr) = setup(&admin());
        let prop_id_1 = mk_proposal(&mut ctr);
        mk_proposal(&mut ctr);
        let prop_id_3 = mk_proposal(&mut ctr);
        ctx.block_timestamp = (START + 2) * MSECOND;
        ctx.attached_deposit = BOND_AMOUNT;
        ctx.predecessor_account_id = sbt_registry();
        testing_env!(ctx);
        ctr.bond(admin(), mk_human_sbt(1), Value::String("".to_string()));

        // vote on proposal 1
        match ctr.on_vote_verified(
            mk_human_sbt(1),
            Some(AccountFlag::Verified),
            prop_id_1,
            alice(),
            vec![candidate(3), candidate(2)],
        ) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        let res = ctr.user_votes(alice());
        assert_eq!(res, vec![Some(vec![2, 1]), None, None]);

        // vote on proposal 3
        match ctr.on_vote_verified(
            mk_human_sbt(1),
            Some(AccountFlag::Verified),
            prop_id_3,
            alice(),
            vec![candidate(2)],
        ) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        let res = ctr.user_votes(alice());
        assert_eq!(res, vec![Some(vec![2, 1]), None, Some(vec![1])]);

        assert_eq!(ctr.user_votes(bob()), vec![None, None, None]); // bob did not vote yet
    }

    #[test]
    fn user_sbt_map_prefix() {
        let (mut ctx, mut ctr) = setup(&admin());
        let prop_id_1 = mk_proposal(&mut ctr);
        let prop_id_2 = mk_proposal(&mut ctr);

        ctx.block_timestamp = (START + 2) * MSECOND;
        ctx.attached_deposit = BOND_AMOUNT;
        ctx.predecessor_account_id = sbt_registry();
        testing_env!(ctx);
        ctr.bond(alice(), mk_human_sbt(1), Value::String("".to_string()));

        match ctr.on_vote_verified(
            mk_human_sbt(1),
            Some(AccountFlag::Verified),
            prop_id_1,
            alice(),
            vec![candidate(3), candidate(2)],
        ) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        let p1 = ctr._proposal(prop_id_1);
        assert!(p1.user_sbt.get(&alice()).is_some());

        let p2 = ctr._proposal(prop_id_2);
        assert!(p2.user_sbt.get(&alice()).is_none());
    }

    #[test]
    fn admin_revoke_vote() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        let vote = vec![candidate(1)];
        ctx.block_timestamp = (START + 2) * MSECOND;
        bond_amount_call(&mut ctx, &mut ctr, alice(), 1);

        // successful vote
        match ctr.on_vote_verified(
            mk_human_sbt(1),
            Some(AccountFlag::Verified),
            prop_id,
            alice(),
            vote,
        ) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        let p = ctr._proposal(1);
        assert_eq!(p.voters_num, 1);
        assert_eq!(p.result, vec![1, 0, 0]);

        // Before revoke bond should be present
        assert_eq!(ctr.bonded_amounts.get(&1), Some(BOND_AMOUNT));

        // revoke vote
        match ctr.admin_revoke_vote(prop_id, 1) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        }

        // Bond amount should be slashed
        assert_eq!(ctr.bonded_amounts.get(&1), None);
        assert_eq!(ctr.total_slashed, BOND_AMOUNT);

        let p = ctr._proposal(1);
        assert_eq!(p.voters_num, 0, "vote should be revoked");
        assert_eq!(p.result, vec![0, 0, 0], "vote should be revoked");

        let expected_event = r#"EVENT_JSON:{"standard":"ndc-elections","version":"1.0.0","event":"revoke_vote","data":{"prop_id":1}}"#;
        assert!(test_utils::get_logs().len() == 2);
        assert_eq!(test_utils::get_logs()[1], expected_event);
    }

    #[test]
    fn has_voted_on_all_proposals() {
        let (mut ctx, mut ctr) = setup(&admin());
        let prop1 = mk_proposal(&mut ctr);
        let prop2 = mk_proposal(&mut ctr);
        let prop3 = mk_proposal(&mut ctr);
        let prop4 = mk_proposal(&mut ctr);
        ctx.block_timestamp = (START + 2) * MSECOND;
        ctx.attached_deposit = BOND_AMOUNT;
        ctx.predecessor_account_id = sbt_registry();
        testing_env!(ctx);
        ctr.bond(admin(), mk_human_sbt(1), Value::String("".to_string()));

        // first vote (voting not yet completed)
        let mut prop_id = prop1;
        match ctr.on_vote_verified(
            mk_human_sbt(1),
            Some(AccountFlag::Verified),
            prop_id,
            alice(),
            vec![candidate(3), candidate(2)],
        ) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        assert!(!ctr.has_voted_on_all_proposals(alice()));

        // second vote (voting not yet completed)
        prop_id = prop2;
        match ctr.on_vote_verified(
            mk_human_sbt(1),
            Some(AccountFlag::Verified),
            prop_id,
            alice(),
            vec![candidate(3), candidate(2)],
        ) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        assert!(!ctr.has_voted_on_all_proposals(alice()));

        // third vote (voting not yet completed)
        prop_id = prop3;
        match ctr.on_vote_verified(
            mk_human_sbt(1),
            Some(AccountFlag::Verified),
            prop_id,
            alice(),
            vec![candidate(3), candidate(2)],
        ) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        assert!(!ctr.has_voted_on_all_proposals(alice()));

        // fourth vote (voting completed)
        prop_id = prop4;
        match ctr.on_vote_verified(
            mk_human_sbt(1),
            Some(AccountFlag::Verified),
            prop_id,
            alice(),
            vec![candidate(3), candidate(2)],
        ) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        assert!(ctr.has_voted_on_all_proposals(alice()));
    }

    #[test]
    fn bond_amount() {
        let (mut ctx, mut ctr) = setup(&alice());

        ctx.predecessor_account_id = sbt_registry();
        ctx.attached_deposit = BOND_AMOUNT;
        testing_env!(ctx);

        ctr.bond(alice(), mk_human_sbt(2), Value::String("".to_string()));
        assert_eq!(ctr.bonded_amounts.get(&2), Some(BOND_AMOUNT));
    }

    #[test]
    #[should_panic(expected = "Err(NoBond)")]
    fn vote_without_bond_amount() {
        let (mut ctx, mut ctr) = setup(&admin());
        let prop_id_1 = mk_proposal(&mut ctr);
        ctx.block_timestamp = (START + 2) * MSECOND;
        testing_env!(ctx);

        match ctr.on_vote_verified(
            mk_human_sbt(1),
            Some(AccountFlag::Verified),
            prop_id_1,
            alice(),
            vec![candidate(3), candidate(2)],
        ) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
    }

    #[test]
    fn vote_unbond_full_flow() -> Result<(), VoteError> {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop1 = mk_proposal(&mut ctr);
        let prop_sp = mk_proposal_setup_package(&mut ctr);
        let vote_sp = setup_package_candidates()[0..=0].to_vec(); // abstain
        let vote1 = vec![candidate(3), candidate(1)];

        alice_voting_context(&mut ctx, &mut ctr);
        assert_eq!(ctr.bonded_amounts.get(&1), Some(BOND_AMOUNT));

        // cast a vote and call on_vote_verified callbacks.
        ctr.vote(prop_sp, vote_sp.clone());
        ctr.vote(prop1, vote1.clone());
        let iah_proof = vec![(alice(), vec![ALICE_SBT])];
        let flag = Some(AccountFlag::Verified);
        ctr.on_vote_verified(iah_proof.clone(), flag.clone(), prop1, alice(), vote1)?;
        ctr.on_vote_verified(iah_proof, flag, prop_sp, alice(), vote_sp)?;

        ctx.block_timestamp = ctr.finish_time * 1000000000; // in nano
        ctx.predecessor_account_id = sbt_registry();
        testing_env!(ctx.clone());

        assert_eq!(
            ctr.user_votes(alice()),
            vec![Some(vec![2, 0]), Some(vec![2])] // votes are alphabetically, yes==2
        );

        ctr.unbond(alice(), mk_human_sbt(1), Value::String("".to_string()));
        // Verify cleanup
        assert_eq!(ctr.bonded_amounts.get(&1), None);
        assert_eq!(ctr.user_votes(alice()), vec![None, None]);

        Ok(())
    }

    #[test]
    fn revoke_vote() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        let vote = vec![candidate(1)];
        ctx.block_timestamp = (START + 2) * MSECOND;
        testing_env!(ctx.clone());

        bond_amount_call(&mut ctx, &mut ctr, admin(), 1);

        // successful vote
        match ctr.on_vote_verified(
            mk_human_sbt(1),
            Some(AccountFlag::Verified),
            prop_id,
            alice(),
            vote,
        ) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        let p = ctr._proposal(1);
        assert_eq!(p.voters_num, 1);
        assert_eq!(p.result, vec![1, 0, 0]);

        // change predecessor non-admin account
        ctx.predecessor_account_id = bob();
        testing_env!(ctx.clone());

        // revoke vote (not blacklisted)
        match ctr.on_revoke_verified(AccountFlag::Verified, prop_id, alice()) {
            Err(RevokeVoteError::NotBlacklisted) => (),
            x => panic!("expected NotBlacklisted, got: {:?}", x),
        }

        // revoke vote
        match ctr.on_revoke_verified(AccountFlag::Blacklisted, prop_id, alice()) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        }
        let p = ctr._proposal(1);
        assert_eq!(p.voters_num, 0, "vote should be revoked");
        assert_eq!(p.result, vec![0, 0, 0], "vote should be revoked");

        let expected_event = r#"EVENT_JSON:{"standard":"ndc-elections","version":"1.0.0","event":"revoke_vote","data":{"prop_id":1}}"#;
        assert!(test_utils::get_logs().len() == 1);
        assert_eq!(test_utils::get_logs()[0], expected_event);
    }

    #[test]
    fn revoke_vote_no_votes() {
        let (mut ctx, mut ctr) = setup(&admin());
        let prop_id = mk_proposal(&mut ctr);
        ctx.block_timestamp = (START + 100) * MSECOND;
        ctx.predecessor_account_id = bob();
        testing_env!(ctx);
        match ctr.on_revoke_verified(AccountFlag::Blacklisted, prop_id, alice()) {
            Err(RevokeVoteError::NotVoted) => (),
            x => panic!("expected NotVoted, got: {:?}", x),
        }
        assert!(test_utils::get_logs().is_empty());
    }

    #[test]
    #[should_panic(expected = "proposal not found")]
    fn revoke_vote_no_proposal() {
        let (_, mut ctr) = setup(&bob());
        let prop_id = 2;
        match ctr.on_revoke_verified(AccountFlag::Blacklisted, prop_id, alice()) {
            x => panic!("{:?}", x),
        }
    }

    #[test]
    fn winners_by_house() {
        let (mut ctx, mut ctr) = setup(&admin());
        let prop_id = mock_proposal_and_votes(&mut ctx, &mut ctr);

        // elections not over yet
        let res = ctr.winners_by_house(prop_id);
        assert_eq!(res, vec![]);

        // voting over but cooldown not yet
        ctx.block_timestamp = (START + 11) * MSECOND;
        testing_env!(ctx.clone());
        let res = ctr.winners_by_house(prop_id);
        assert_eq!(res, vec![]);

        // candiate    | votes
        // -------------------
        // candiate(1) | 5
        // candiate(2) | 10
        // candiate(3) | 15
        // candiate(4) | 10
        // candiate(5) | 5

        // min_candidate_support = 6
        // seats = 5
        // the method should return only the candiadtes that reach min_candidate support
        // thats why we have only 3 winners rather than 5
        ctx.block_timestamp = (START + 111) * MSECOND; // past cooldown
        testing_env!(ctx.clone());
        let res = ctr.winners_by_house(prop_id);
        assert_eq!(res, vec![candidate(3), candidate(2), candidate(4)]);
    }
}
