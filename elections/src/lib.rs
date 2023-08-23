use std::cmp::max;
use std::collections::HashSet;

use events::{emit_revoke_vote, emit_vote};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LookupMap;
use near_sdk::json_types::U128;
use near_sdk::{
    env, near_bindgen, require, AccountId, PanicOnDefault, Promise, PromiseError, PromiseOrValue,
};

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
    pub total_slashed: u128,
    /// Finish time is end + cooldown. This used in the `unbond` function: user can unbond only after this time.
    /// Unix timestamp (in milliseconds)
    pub finish_time: u64,

    /// address which can pause the contract and make a new proposal. Should be a multisig / DAO;
    pub authority: AccountId,
    pub sbt_registry: AccountId,

    pub bond_amount_verified: u128,
    pub bond_amount_gray: u128,
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
            policy: policy,
            finish_time: finish_time,
            bond_amount_gray: GRAY_BOND_AMOUNT,
            bond_amount_verified: BOND_AMOUNT,
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
    /// User is required to pay bond amount in this step
    #[payable]
    pub fn accept_fair_voting_policy(&mut self, policy: String) -> Promise {
        require!(
            env::attached_deposit() >= ACCEPT_POLICY_COST,
            format!(
                "requires {} yocto deposit for storage fees",
                ACCEPT_POLICY_COST
            )
        );
        require!(
            env::prepaid_gas() >= ACCEPT_POLICY_GAS,
            format!("not enough gas, min: {:?}", ACCEPT_POLICY_GAS)
        );

        let sender = env::predecessor_account_id();
        ext_sbtreg::ext(self.sbt_registry.clone())
            .is_human(sender.clone())
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(VOTE_GAS_CALLBACK)
                    .on_accept_policy_callback(sender, policy, U128(env::attached_deposit())),
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
            self.policy == self.accepted_policy.get(&user).unwrap_or_default(),
            "user didn't accept the voting policy, or the accepted voting policy doesn't match the required one"
        );

        validate_vote(&vote, p.seats, &p.candidates);
        // call SBT registry to verify SBT
        let sbt_promise = ext_sbtreg::ext(self.sbt_registry.clone()).is_human(user.clone());
        let check_flag = ext_sbtreg::ext(self.sbt_registry.clone()).account_flagged(user.clone());

        sbt_promise.and(check_flag).then(
            ext_self::ext(env::current_account_id())
                .with_static_gas(VOTE_GAS_CALLBACK)
                .on_vote_verified(prop_id, user, vote),
        )
    }

    #[payable]
    pub fn bond(&mut self, caller: AccountId, iah_proof: HumanSBTs, _payload: String) -> PromiseOrValue<U128> {
        let attached_deposit = env::attached_deposit();
        if env::predecessor_account_id() != self.sbt_registry {
            return PromiseOrValue::Promise(Promise::new(caller)
            .transfer(attached_deposit)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(FAILURE_CALLBACK_GAS)
                    .on_failure(format!(
                        "Can only be called by registry"
                    )),
            ));
            // option 1. return deposit and return from function ... note it will show in explorer that the function succeeded
            // option 2. return deposit and schedule "self callback" to panic. - here the explorer will show that function fails.
        }

        if iah_proof.is_empty() || !(iah_proof.len() == 1 && iah_proof[0].1.len() == 1) {
            return PromiseOrValue::Promise(Promise::new(caller)
            .transfer(attached_deposit)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(FAILURE_CALLBACK_GAS)
                    .on_failure(format!(
                        "Not a human"
                    )),
            ));
        }
        let token_id = iah_proof[0].1.get(0).unwrap();
        self.bonded_amounts.insert(token_id, &attached_deposit);
        PromiseOrValue::Value(U128(attached_deposit))
    }

    #[payable]
    pub fn unbond(&mut self, caller: AccountId, iah_proof: HumanSBTs, _payload: String) -> Promise {
        let attached_deposit = env::attached_deposit();
        if env::predecessor_account_id() != self.sbt_registry {
            return Promise::new(caller)
            .transfer(attached_deposit)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(FAILURE_CALLBACK_GAS)
                    .on_failure(format!(
                        "Can only be called by registry"
                    )),
            );
        }

        if iah_proof.is_empty() || !(iah_proof.len() == 1 && iah_proof[0].1.len() == 1) {
            return Promise::new(caller)
            .transfer(attached_deposit)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(FAILURE_CALLBACK_GAS)
                    .on_failure(format!(
                        "Not a human"
                    )),
            );
        }

        let token_id = iah_proof[0].1.get(0).unwrap();
        let bonded_amount = self
                    .bonded_amounts
                    .get(token_id)
                    .expect("bond doesn't exist");
        let unbond_amount = bonded_amount - ACCEPT_POLICY_COST - VOTE_COST;
        self.bonded_amounts.remove(token_id);

        Promise::new(caller).transfer(unbond_amount)
    }

    /// Method for the authority to revoke votes from blacklisted accounts.
    /// Panics if the proposal doesn't exists or the it's called before the proposal starts or after proposal `end+cooldown`.
    #[handle_result]
    pub fn revoke_vote(&mut self, prop_id: u32, token_id: TokenId) -> Result<(), RevokeVoteError> {
        // check if the caller is the authority allowed to revoke votes
        self.assert_admin();
        self.slash_bond(token_id);
        let mut p = self._proposal(prop_id);
        p.revoke_votes(token_id)?;
        self.proposals.insert(&prop_id, &p);
        emit_revoke_vote(prop_id);
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
        #[callback_unwrap] account_flag: Option<AccountFlag>,
        prop_id: u32,
        voter: AccountId,
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

        let required_bond;
        if account_flag == Some(AccountFlag::Blacklisted) {
            return Err(VoteError::Blacklisted);
        }
        if account_flag == Some(AccountFlag::Verified) {
            required_bond = self.bond_amount_verified;
        } else {
            required_bond = self.bond_amount_gray;
        }

        let bond_deposited = self
            .bonded_amounts
            .get(&tokens[0].1.get(0).unwrap())
            .expect("Bond doesn't exist");
        if bond_deposited < required_bond {
            return Err(VoteError::MinBond(required_bond, bond_deposited));
        }

        let mut p = self._proposal(prop_id);
        p.vote_on_verified(&tokens[0].1, voter, vote)?;
        self.proposals.insert(&prop_id, &p);
        emit_vote(prop_id);
        Ok(())
    }

    #[private]
    pub fn on_accept_policy_callback(
        &mut self,
        #[callback_result] callback_result: Result<HumanSBTs, PromiseError>,
        sender: AccountId,
        policy: String,
        deposit_amount: U128,
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
                self.accepted_policy.insert(&sender, &policy);

                self.bonded_amounts.insert(token_id, &deposit_amount.0);

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
                            .on_failure(format!(
                                "IAHRegistry::is_human(), Accept policy failure: {e:?}"
                            )),
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
        let bond_amount = self.bonded_amounts.get(&token_id);
        if let Some(value) = bond_amount {
            self.total_slashed += value - ACCEPT_POLICY_COST - VOTE_COST;
            self.bonded_amounts.remove(&token_id);
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod unit_tests {
    use near_sdk::{
        test_utils::{self, VMContextBuilder},
        testing_env, Gas, VMContext,
    };

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

            ctr.on_accept_policy_callback(
                Ok(mk_human_sbt(i as u64)),
                candidate(i),
                policy1(),
                U128(BOND_AMOUNT),
            );
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
            0,
            vec![],
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
        ctr.on_accept_policy_callback(Ok(mk_human_sbt(1)), alice(), policy1(), U128(BOND_AMOUNT));

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
            2,
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
            2,
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
        testing_env!(ctx.clone());
        ctr.on_accept_policy_callback(Ok(mk_human_sbt(1)), admin(), policy1(), U128(BOND_AMOUNT));

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
        ctr.on_accept_policy_callback(Ok(mk_human_sbt(4)), admin(), policy1(), U128(BOND_AMOUNT));
        ctr._proposal(prop_id).voters.insert(&4, &vec![1]);
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

        // wrong issuer
        ctr.on_accept_policy_callback(Ok(mk_human_sbt(3)), admin(), policy1(), U128(BOND_AMOUNT));
        match ctr.on_vote_verified(
            mk_nohuman_sbt(3),
            Some(AccountFlag::Verified),
            prop_id,
            alice(),
            vote.clone(),
        ) {
            Err(VoteError::WrongIssuer) => (),
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
        ctx.predecessor_account_id = alice();
        testing_env!(ctx.clone());
        ctr.on_accept_policy_callback(Ok(mk_human_sbt(20)), alice(), policy1(), U128(BOND_AMOUNT));

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
        ctr.on_accept_policy_callback(Ok(mk_human_sbt(22)), bob(), policy1(), U128(BOND_AMOUNT));

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
            vec![],
        ) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        };
        let p = ctr._proposal(prop_sp);
        assert!(p.voters.contains_key(&22), "token id should be recorded");
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
        ctr.on_accept_policy_callback(Ok(mk_human_sbt(1)), admin(), policy1(), U128(BOND_AMOUNT));

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
        ctx.attached_deposit = VOTE_COST;
        ctx.block_timestamp = (START + 2) * MSECOND;
        ctx.prepaid_gas = Gas(10 * Gas::ONE_TERA.0);
        testing_env!(ctx);

        ctr.vote(prop_id, vec![candidate(1)]);
    }

    #[test]
    #[should_panic(
        expected = "requires 1000000000000000000000 yocto deposit for storage fees for every new vote"
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
        ctr.on_accept_policy_callback(Ok(mk_human_sbt(1)), admin(), policy1(), U128(BOND_AMOUNT));

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
            Err(RevokeVoteError::NotVoted) => (),
            x => panic!("expected NotVoted, got: {:?}", x),
        }
        assert!(test_utils::get_logs().len() == 0);
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

    #[test]
    fn user_votes() {
        let (mut ctx, mut ctr) = setup(&admin());
        let prop_id_1 = mk_proposal(&mut ctr);
        mk_proposal(&mut ctr);
        let prop_id_3 = mk_proposal(&mut ctr);
        ctx.block_timestamp = (START + 2) * MSECOND;
        testing_env!(ctx.clone());
        ctr.on_accept_policy_callback(Ok(mk_human_sbt(1)), admin(), policy1(), U128(BOND_AMOUNT));

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
        testing_env!(ctx.clone());
        ctr.on_accept_policy_callback(Ok(mk_human_sbt(1)), admin(), policy1(), U128(BOND_AMOUNT));

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
    fn revoke_vote() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_id = mk_proposal(&mut ctr);
        let vote = vec![candidate(1)];
        ctx.block_timestamp = (START + 2) * MSECOND;
        testing_env!(ctx.clone());
        ctr.on_accept_policy_callback(Ok(mk_human_sbt(1)), alice(), policy1(), U128(BOND_AMOUNT));

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
        let p = ctr._proposal(1);
        assert_eq!(p.voters_num, 1);
        assert_eq!(p.result, vec![1, 0, 0]);

        // Before revoke bond should be present
        assert_eq!(ctr.bonded_amounts.get(&1), Some(BOND_AMOUNT));

        // revoke vote
        match ctr.revoke_vote(prop_id, 1) {
            Ok(_) => (),
            x => panic!("expected OK, got: {:?}", x),
        }

        // Bond amount should be slashed
        assert_eq!(ctr.bonded_amounts.get(&1), None);
        assert_eq!(
            ctr.total_slashed,
            BOND_AMOUNT - ACCEPT_POLICY_COST - VOTE_COST
        );

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
        testing_env!(ctx.clone());
        ctr.on_accept_policy_callback(Ok(mk_human_sbt(1)), admin(), policy1(), U128(BOND_AMOUNT));

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

        ctr.on_accept_policy_callback(Ok(mk_human_sbt(1)), alice(), policy1(), U128(BOND_AMOUNT));
        assert_eq!(ctr.bonded_amounts.get(&1), Some(BOND_AMOUNT));

        ctx.predecessor_account_id = sbt_registry();
        ctx.attached_deposit = BOND_AMOUNT;
        testing_env!(ctx);

        ctr.bond(alice(), mk_human_sbt(2), "".to_string());
        assert_eq!(ctr.bonded_amounts.get(&2), Some(BOND_AMOUNT));
    }

    #[test]
    #[should_panic(expected = "Err(MinBond(3000000000000000000000000, 2999000000000000000000000))")]
    fn vote_without_bond_amount() {
        let (mut ctx, mut ctr) = setup(&admin());
        let prop_id_1 = mk_proposal(&mut ctr);
        ctx.block_timestamp = (START + 2) * MSECOND;
        testing_env!(ctx.clone());
        ctr.on_accept_policy_callback(
            Ok(mk_human_sbt(1)),
            admin(),
            policy1(),
            U128(BOND_AMOUNT - MILI_NEAR),
        );

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
    fn unbond_amount() {
        let (mut ctx, mut ctr) = setup(&admin());

        let prop_sp = mk_proposal_setup_package(&mut ctr);
        alice_voting_context(&mut ctx, &mut ctr);

        assert_eq!(ctr.bonded_amounts.get(&1), Some(BOND_AMOUNT));
        ctr.vote(prop_sp, vec![]);

        ctx.block_timestamp = ctr.finish_time + 1;
        ctx.predecessor_account_id = sbt_registry();
        ctx.attached_deposit = BOND_AMOUNT;
        testing_env!(ctx);

        ctr.unbond(alice(), mk_human_sbt(1), "".to_string());
        assert_eq!(ctr.bonded_amounts.get(&1), None);
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
