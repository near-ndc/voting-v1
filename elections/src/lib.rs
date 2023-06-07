use std::collections::HashSet;

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, LookupSet};
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault, Promise};

mod constants;
mod ext;
mod proposal;
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

    /// address which can pause the contract and make proposal.
    /// Should be a multisig / DAO;
    pub authority: AccountId,
    pub sbt_registry: AccountId,
    /// issuer account for proof of humanity
    pub iah_issuer: AccountId,
    /// SBT class ID used for Facetech verification
    pub iah_class_id: u64,
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(
        authority: AccountId,
        sbt_registry: AccountId,
        iah_issuer: AccountId,
        iah_class_id: u64,
    ) -> Self {
        Self {
            pause: false,
            authority,
            sbt_registry,
            iah_issuer,
            iah_class_id,
            proposals: LookupMap::new(StorageKey::Proposals),
            prop_counter: 0,
        }
    }

    /// creates new empty proposal
    /// returns the new proposal ID
    pub fn creat_proposal(
        &mut self,
        typ: HouseType,
        start: u64,
        end: u64,
        ref_link: String,
        quorum: u32,
        credits: u16,
        #[allow(unused_mut)] mut candidates: Vec<AccountId>,
    ) -> u32 {
        self.assert_admin();
        let min_start = env::block_timestamp() / SECOND;
        require!(min_start < start, "proposal start must be in the future");
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
            credits,
            candidates,
            result: vec![0; l],
            voters: LookupSet::new(StorageKey::ProposalVoters(self.prop_counter)),
        };

        self.proposals.insert(&self.prop_counter, &p);
        self.prop_counter
    }

    /// election vote using quadratic mechanism
    #[payable]
    pub fn vote(&mut self, prop_id: u32, vote: Vote) -> Promise {
        let p = self._proposal(prop_id);
        p.assert_active();
        let user = env::predecessor_account_id();
        require!(!p.voters.contains(&user), "caller already voted",);
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
        validate_vote(&vote, p.credits, &p.candidates);

        // TODO
        // call SBT registry to verify  SBT
        // ext_sbtreg::ext(self.sbt_registry.clone())
        //     .sbt_tokens_by_owner(
        //         user.clone(),
        //         Some(self.iah_issuer.clone()),
        //         Some(self.iah_class_id.clone()),
        //         Some(1),
        //     )
        //     .then(
        ext_self::ext(env::current_account_id())
            .with_static_gas(VOTE_GAS_CALLBACK)
            .on_vote_verified(prop_id, user, vote)
    }

    /*****************
     * PRIVATE
     ****************/

    #[private]
    pub fn on_vote_verified(&mut self, prop_id: u32, user: AccountId, vote: Vote) {
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
mod tests {
    use near_sdk::{test_utils::VMContextBuilder, testing_env, AccountId, Gas, VMContext};

    use crate::{Contract, Vote, SECOND, VOTE_COST, VOTE_GAS};
    const START: u64 = 10;
    const CLASS_ID: u64 = 1;

    fn alice() -> AccountId {
        AccountId::new_unchecked("alice.near".to_string())
    }

    fn bob() -> AccountId {
        AccountId::new_unchecked("bob.near".to_string())
    }

    fn admin() -> AccountId {
        AccountId::new_unchecked("admin.near".to_string())
    }

    fn authority() -> AccountId {
        AccountId::new_unchecked("authority.near".to_string())
    }

    fn sbt_registry() -> AccountId {
        AccountId::new_unchecked("sbt_registry.near".to_string())
    }

    fn iah_issuer() -> AccountId {
        AccountId::new_unchecked("iah_issuer.near".to_string())
    }

    fn setup(predecessor: &AccountId) -> (VMContext, Contract) {
        let mut ctx = VMContextBuilder::new()
            .predecessor_account_id(admin())
            // .attached_deposit(deposit_dec.into())
            .block_timestamp(START * SECOND)
            .is_view(false)
            .build();
        testing_env!(ctx.clone());
        let ctr = Contract::new(authority(), sbt_registry(), iah_issuer(), CLASS_ID);
        ctx.predecessor_account_id = predecessor.clone();
        testing_env!(ctx.clone());
        return (ctx, ctr);
    }

    #[test]
    fn assert_admin() {
        let (_, ctr) = setup(&authority());
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
        let (_, mut ctr) = setup(&authority());

        ctr.creat_proposal(
            crate::HouseType::HouseOfMerit,
            START - 1,
            START + 100,
            String::from("ref_link.io"),
            2,
            2,
            vec![alice()],
        );
    }

    #[test]
    #[should_panic(expected = "proposal start must be before end")]
    fn create_proposal_end_before_start() {
        let (_, mut ctr) = setup(&authority());

        ctr.creat_proposal(
            crate::HouseType::HouseOfMerit,
            START + 10,
            START,
            String::from("ref_link.io"),
            2,
            2,
            vec![alice()],
        );
    }

    #[test]
    #[should_panic(expected = "ref_link length must be between 6 and 120 bytes")]
    fn create_proposal_wrong_ref_link_length() {
        let (_, mut ctr) = setup(&authority());

        ctr.creat_proposal(
            crate::HouseType::HouseOfMerit,
            START + 1,
            START + 10,
            String::from("short"),
            2,
            2,
            vec![alice()],
        );
    }

    #[test]
    #[should_panic(expected = "duplicated candidates")]
    fn create_proposal_duplicated_candidates() {
        let (_, mut ctr) = setup(&authority());

        ctr.creat_proposal(
            crate::HouseType::HouseOfMerit,
            START + 1,
            START + 10,
            String::from("ref_link.io"),
            2,
            2,
            vec![alice(), alice()],
        );
    }

    #[test]
    fn create_proposal() {
        let (_, mut ctr) = setup(&authority());

        assert!(ctr.prop_counter == 0);
        let prop_id = ctr.creat_proposal(
            crate::HouseType::HouseOfMerit,
            START + 1,
            START + 10,
            String::from("ref_link.io"),
            2,
            2,
            vec![alice()],
        );
        assert!(ctr.prop_counter == 1);
        assert!(ctr.proposals.contains_key(&prop_id));
    }

    #[test]
    #[should_panic(expected = "can only vote between proposal start and end time")]
    fn vote_wrong_time() {
        let (_, mut ctr) = setup(&authority());

        assert!(ctr.prop_counter == 0);
        let prop_id = ctr.creat_proposal(
            crate::HouseType::HouseOfMerit,
            START + 1,
            START + 10,
            String::from("ref_link.io"),
            2,
            2,
            vec![alice()],
        );
        let vote: Vote = vec![(alice(), 1)];
        ctr.vote(prop_id, vote);
    }

    #[test]
    #[should_panic(
        expected = "requires 150000000000000000000 yocto deposit for storage fees for every new vote"
    )]
    fn vote_wrong_deposit() {
        let (mut ctx, mut ctr) = setup(&authority());

        assert!(ctr.prop_counter == 0);
        let prop_id = ctr.creat_proposal(
            crate::HouseType::HouseOfMerit,
            START + 1,
            START + 10,
            String::from("ref_link.io"),
            2,
            2,
            vec![alice()],
        );
        ctx.attached_deposit = VOTE_COST - 1;
        ctx.block_timestamp = (START + 2) * SECOND;
        testing_env!(ctx.clone());
        let vote: Vote = vec![(alice(), 1)];
        ctr.vote(prop_id, vote);
    }

    #[test]
    #[should_panic(expected = "caller already voted")]
    fn vote_caller_already_voted() {
        let (mut ctx, mut ctr) = setup(&authority());

        assert!(ctr.prop_counter == 0);
        let prop_id = ctr.creat_proposal(
            crate::HouseType::HouseOfMerit,
            START + 1,
            START + 10,
            String::from("ref_link.io"),
            2,
            2,
            vec![alice()],
        );
        //set bob as voter and attempt double vote
        ctr._proposal(prop_id).voters.insert(&bob());
        ctx.predecessor_account_id = bob();
        ctx.attached_deposit = VOTE_COST;
        ctx.block_timestamp = (START + 2) * SECOND;
        testing_env!(ctx.clone());
        let vote: Vote = vec![(alice(), 1)];
        ctr.vote(prop_id, vote.clone());
        ctr._proposal(prop_id).voters.insert(&bob());
    }

    #[test]
    #[should_panic(expected = "not enough gas, min: Gas(70000000000000)")]
    fn vote_wrong_gas() {
        let (mut ctx, mut ctr) = setup(&authority());

        assert!(ctr.prop_counter == 0);
        let prop_id = ctr.creat_proposal(
            crate::HouseType::HouseOfMerit,
            START + 1,
            START + 10,
            String::from("ref_link.io"),
            2,
            2,
            vec![alice()],
        );
        ctx.attached_deposit = VOTE_COST;
        ctx.block_timestamp = (START + 2) * SECOND;
        ctx.prepaid_gas = Gas(10 * Gas::ONE_TERA.0);
        testing_env!(ctx.clone());
        let vote: Vote = vec![(alice(), 1)];
        ctr.vote(prop_id, vote);
    }

    #[test]
    #[should_panic(expected = "double vote for the same candidate")]
    fn vote_double_vote_same_candidate() {
        let (mut ctx, mut ctr) = setup(&authority());

        assert!(ctr.prop_counter == 0);
        let prop_id = ctr.creat_proposal(
            crate::HouseType::HouseOfMerit,
            START + 1,
            START + 10,
            String::from("ref_link.io"),
            2,
            2,
            vec![alice()],
        );
        ctx.attached_deposit = VOTE_COST;
        ctx.block_timestamp = (START + 2) * SECOND;
        ctx.prepaid_gas = VOTE_GAS;
        testing_env!(ctx.clone());
        let vote: Vote = vec![(alice(), 1), (alice(), 1)];
        ctr.vote(prop_id, vote);
    }

    #[test]
    #[should_panic(expected = "vote for unknown candidate")]
    fn vote_unknown_candidate() {
        let (mut ctx, mut ctr) = setup(&authority());

        assert!(ctr.prop_counter == 0);
        let prop_id = ctr.creat_proposal(
            crate::HouseType::HouseOfMerit,
            START + 1,
            START + 10,
            String::from("ref_link.io"),
            2,
            2,
            vec![alice()],
        );
        ctx.attached_deposit = VOTE_COST;
        ctx.block_timestamp = (START + 2) * SECOND;
        ctx.prepaid_gas = VOTE_GAS;
        testing_env!(ctx.clone());
        let vote: Vote = vec![(bob(), 1)];
        ctr.vote(prop_id, vote);
    }

    #[test]
    #[should_panic(expected = "vote with too many credits")]
    fn vote_too_many_credits() {
        let (mut ctx, mut ctr) = setup(&authority());

        assert!(ctr.prop_counter == 0);
        let prop_id = ctr.creat_proposal(
            crate::HouseType::HouseOfMerit,
            START + 1,
            START + 10,
            String::from("ref_link.io"),
            2,
            2,
            vec![alice()],
        );
        ctx.attached_deposit = VOTE_COST;
        ctx.block_timestamp = (START + 2) * SECOND;
        ctx.prepaid_gas = VOTE_GAS;
        testing_env!(ctx.clone());
        let vote: Vote = vec![(alice(), 5)];
        ctr.vote(prop_id, vote);
    }
}
