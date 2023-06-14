use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, LookupMap, UnorderedMap, UnorderedSet};
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault, Promise};

mod constants;
mod storage;

pub use crate::constants::*;
use crate::storage::*;

pub mod ext;
pub use crate::ext::*;

#[cfg(test)]
mod integration_tests;

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Contract {
    pub sbt_registry: AccountId,
    /// IAH issuer account for proof of humanity
    pub iah_issuer: AccountId,
    /// OG token (issuer, class_id)
    pub og_class: (AccountId, u64),
    /// map of nominations
    pub nominations: UnorderedMap<AccountId, HouseType>,
    /// set of pairs (candidate, upvoter)
    /// TODO: this needs to be updated to a different structure to improve performance
    pub upvotes: UnorderedSet<(AccountId, AccountId)>,
    /// number of upvotes per candidate
    pub upvotes_per_candidate: LookupMap<AccountId, u32>,
    /// list of admins
    pub admins: LazyOption<Vec<AccountId>>,
    /// nomination period start time
    pub start_time: u64,
    /// nomination period end time
    pub end_time: u64,
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(
        sbt_registry: AccountId,
        iah_issuer: AccountId,
        og_class: (AccountId, u64),
        admins: Vec<AccountId>,
        start_time: u64,
        end_time: u64,
    ) -> Self {
        Self {
            sbt_registry,
            iah_issuer,
            og_class,
            start_time,
            end_time,
            nominations: UnorderedMap::new(StorageKey::Nominations),
            upvotes: UnorderedSet::new(StorageKey::Upvotes),
            upvotes_per_candidate: LookupMap::new(StorageKey::UpvotesPerCandidate),
            admins: LazyOption::new(StorageKey::Admins, Some(&admins)),
        }
    }

    /**********
     * QUERIES
     **********/

    /// returns list of pairs:
    ///   (self-nominated account, sum of upvotes) of given house.
    pub fn nominations(&self, house: HouseType) -> Vec<(AccountId, u32)> {
        let mut results: Vec<(AccountId, u32)> = Vec::new();
        for nomination in self.nominations.iter() {
            if nomination.1 == house {
                let num_of_upvotes = self.upvotes_per_candidate.get(&nomination.0).unwrap_or(0);
                results.push((nomination.0, num_of_upvotes));
            }
        }
        results
    }

    /**********
     * TRANSACTIONS
     **********/

    /// nominate method allows to submit nominatios by verified humans
    /// + Checks if the caller is a verified human
    /// + Check if the caller is a OG member
    /// + Checks if the nomination has been already submitted
    /// + Checks if the user has nominated themselves to a different house before
    /// + Checks if the nomination was submitted during the nomination period
    pub fn self_nominate(
        &mut self,
        house: HouseType,
        #[allow(unused_variables)] comment: String,
        #[allow(unused_variables)] link: Option<String>,
    ) -> Promise {
        self.assert_active();
        let nominee = env::predecessor_account_id();

        require!(
            self.nominations.get(&nominee).is_none(),
            "User has already nominated themselves to a different house",
        );

        require!(
            env::prepaid_gas() >= GAS_NOMINATE,
            format!("Not enough gas, min: {:?}", GAS_NOMINATE)
        );

        require!(
            env::attached_deposit() >= NOMINATE_COST,
            format!("Not enough deposit, min: {:?}", NOMINATE_COST)
        );

        // call SBT registry to verif OG SBT and cast the nomination in callback based on the return from sbt_tokens_by_owner
        ext_sbtreg::ext(self.sbt_registry.clone())
            .sbt_tokens_by_owner(
                nominee.clone(),
                Some(self.og_class.0.clone()),
                Some(self.og_class.1.clone()),
                Some(1),
                Some(true),
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_NOMINATE)
                    .on_nominate_verified(nominee, house),
            )
    }

    /// upvtoe allows users to upvote a specific candidante
    /// + Checks if the caller is a verified human
    /// + Checks if there is a nomination for the given candidate
    /// + Checks if the caller has already upvoted the candidate
    /// + Checks if the nomination period is active
    pub fn upvote(&mut self, candidate: AccountId) -> Promise {
        self.assert_active();
        let upvoter = env::predecessor_account_id();

        require!(upvoter != candidate, "Cannot upvote your own nomination");
        require!(
            self.nominations.get(&candidate).is_some(),
            "Nomination not found",
        );

        require!(
            !self.upvotes.contains(&(candidate.clone(), upvoter.clone())),
            "User has already upvoted given nomination"
        );

        require!(
            env::prepaid_gas() >= GAS_UPVOTE,
            format!("Not enough gas, min: {:?}", GAS_UPVOTE)
        );

        require!(
            env::attached_deposit() >= UPVOTE_COST,
            format!("Not enough deposit, min: {:?}", UPVOTE_COST)
        );

        // call SBT registry to verify IAH/ OG SBT and cast the upvote in callback based on the return from sbt_tokens_by_owner
        ext_sbtreg::ext(self.sbt_registry.clone())
            .is_human(upvoter.clone())
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_UPVOTE)
                    .on_upvote_verified(candidate, upvoter),
            )
    }

    // comment allows users to comment on a existing nomination
    /// + Checks if the caller is a verified human
    /// + Checks if there is a nomination for the given candidate
    /// + Checks if the nomination period is active
    pub fn comment(&mut self, candidate: AccountId, comment: String) -> Promise {
        self.assert_active();
        let commenter = env::predecessor_account_id();
        require!(
            self.nominations.get(&candidate).is_some(),
            "Nomination not found",
        );

        require!(
            env::prepaid_gas() >= GAS_COMMENT,
            format!("Not enough gas, min: {:?}", GAS_COMMENT)
        );

        // call SBT registry to verify IAH/ OG SBT and cast the nomination in callback based on the return from sbt_tokens_by_owner
        ext_sbtreg::ext(self.sbt_registry.clone())
            .is_human(commenter.clone())
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_NOMINATE)
                    .on_comment_verified(),
            )
    }

    /// Revokes callers nominatnion and all the upvotes of that specific nomination
    /// + Checks if the nomination period is active
    /// + Checks if the user has a nomination to revoke
    pub fn self_revoke(&mut self) {
        self.assert_active();
        let nominee = env::predecessor_account_id();

        require!(
            self.nominations.get(&nominee).is_some(),
            "User is not nominated, cannot revoke",
        );

        self.nominations.remove(&nominee);
        self.upvotes_per_candidate.remove(&nominee);

        let mut keys_to_remove: Vec<(AccountId, AccountId)> = Vec::new();
        //TODO: once the upvotes data strucred is updaed we need to change it as well
        for upvote in self.upvotes.iter() {
            if upvote.0 == nominee {
                keys_to_remove.push(upvote);
            }
        }
        for key in keys_to_remove.iter() {
            self.upvotes.remove(key);
        }
    }

    /// Revokes the upvote
    /// + Checks if the nomination period is active
    /// + Checks if the caller upvoted the `candidate` before
    pub fn revoke_upvote(&mut self, candidate: AccountId) {
        self.assert_active();
        let caller = env::predecessor_account_id();

        if self.upvotes.remove(&(candidate.clone(), caller)) {
            let num_of_upvotes = self.upvotes_per_candidate.get(&candidate).unwrap_or(1); //we do 1 so the results will be at least 0
            self.upvotes_per_candidate
                .insert(&candidate, &(num_of_upvotes - 1));
        } else {
            env::panic_str(
                "There are no upvotes registered for this candidate from the caller account",
            );
        }
    }

    /*****************
     * PRIVATE
     ****************/

    /// Checks If the upvoter is a verified human registers the upvote otherwise panics
    #[private]
    pub fn on_upvote_verified(
        &mut self,
        #[callback_unwrap] is_human: bool,
        nominee: AccountId,
        upvoter: AccountId,
    ) {
        if !is_human {
            env::panic_str("Not a verified human member, or the tokens are expired");
        }
        let num_of_upvotes = self.upvotes_per_candidate.get(&nominee).unwrap_or(0);
        self.upvotes_per_candidate
            .insert(&nominee, &(num_of_upvotes + 1));
        self.upvotes.insert(&(nominee, upvoter));
    }

    /// Checks If the commenter is a verified human otherwise panics
    #[private]
    pub fn on_comment_verified(&mut self, #[callback_unwrap] is_human: bool) {
        if !is_human {
            env::panic_str("Not a verified human member, or the tokens are expired");
        }
        // TODO: what should be done in the case we do not register the comments on-chain?
    }

    ///Checks If the caller is a OG token holder and registers the nomination otherwise panics
    #[private]
    pub fn on_nominate_verified(
        &mut self,
        #[callback_unwrap] sbts: Vec<(AccountId, Vec<OwnedToken>)>,
        nominee: AccountId,
        house_type: HouseType,
    ) {
        if sbts.is_empty() && !(sbts[0].1[0].metadata.class == self.og_class.1) {
            env::panic_str("Not a verified OG member, or the token is expired");
        }
        self.nominations.insert(&nominee, &house_type);
    }

    fn assert_active(&self) {
        let current_timestamp = env::block_timestamp();
        require!(
            current_timestamp >= self.start_time && current_timestamp <= self.end_time,
            "Nominations time is not active"
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::ops::Sub;

    use near_sdk::{test_utils::VMContextBuilder, testing_env, AccountId, Gas, VMContext};

    use crate::{
        storage::HouseType, Contract, GAS_COMMENT, GAS_NOMINATE, GAS_UPVOTE, NOMINATE_COST, SECOND,
        UPVOTE_COST,
    };
    const START: u64 = 10;
    const END: u64 = 100000;
    const OG_CLASS_ID: u64 = 2;

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

    fn iah_issuer() -> AccountId {
        AccountId::new_unchecked("iah_issuer.near".to_string())
    }

    fn og_token_issuer() -> AccountId {
        AccountId::new_unchecked("og_token.near".to_string())
    }

    fn setup(predecessor: &AccountId) -> (VMContext, Contract) {
        let mut ctx = VMContextBuilder::new()
            .predecessor_account_id(admin())
            // .attached_deposit(deposit_dec.into())
            .block_timestamp(START * SECOND)
            .is_view(false)
            .build();
        testing_env!(ctx.clone());
        let ctr = Contract::new(
            sbt_registry(),
            iah_issuer(),
            (og_token_issuer(), OG_CLASS_ID),
            vec![admin()],
            START * SECOND,
            END * SECOND,
        );
        ctx.predecessor_account_id = predecessor.clone();
        testing_env!(ctx.clone());
        return (ctx, ctr);
    }

    #[test]
    fn assert_active() {
        let (_, ctr) = setup(&admin());
        ctr.assert_active();
    }

    #[test]
    #[should_panic(expected = "Nominations time is not active")]
    fn assert_active_too_early() {
        let (mut ctx, ctr) = setup(&alice());
        ctx.block_timestamp = (START - 5) * SECOND;
        testing_env!(ctx.clone());
        ctr.assert_active();
    }

    #[test]
    #[should_panic(expected = "Nominations time is not active")]
    fn assert_active_too_late() {
        let (mut ctx, ctr) = setup(&alice());
        ctx.block_timestamp = (END + 5) * SECOND;
        testing_env!(ctx.clone());
        ctr.assert_active();
    }

    #[test]
    #[should_panic(expected = "User has already nominated themselves to a different house")]
    fn self_nominate_already_nominated() {
        let (_, mut ctr) = setup(&alice());
        ctr.nominations
            .insert(&alice(), &HouseType::CouncilOfAdvisors);
        ctr.self_nominate(HouseType::HouseOfMerit, String::from("test"), None);
    }

    #[test]
    #[should_panic(expected = "Not enough gas, min: Gas(20000000000000)")]
    fn self_nominate_wrong_gas() {
        let (mut ctx, mut ctr) = setup(&alice());
        ctx.prepaid_gas = GAS_NOMINATE.sub(Gas(10));
        testing_env!(ctx.clone());
        ctr.self_nominate(HouseType::HouseOfMerit, String::from("test"), None);
    }

    #[test]
    #[should_panic(expected = "Not enough deposit, min: 1000000000000000000000")]
    fn self_nominate_wrong_deposit() {
        let (_, mut ctr) = setup(&alice());
        ctr.self_nominate(HouseType::HouseOfMerit, String::from("test"), None);
    }

    #[test]
    #[should_panic(expected = "Nominations time is not active")]
    fn self_nominate_not_active() {
        let (mut ctx, mut ctr) = setup(&alice());
        ctx.block_timestamp = (START - 5) * SECOND;
        testing_env!(ctx.clone());
        ctr.self_nominate(HouseType::HouseOfMerit, String::from("test"), None);
    }

    #[test]
    fn self_nominate() {
        let (mut ctx, mut ctr) = setup(&alice());
        ctx.attached_deposit = NOMINATE_COST;
        testing_env!(ctx.clone());
        ctr.self_nominate(HouseType::HouseOfMerit, String::from("test"), None);
    }

    #[test]
    #[should_panic(expected = "Cannot upvote your own nomination")]
    fn upvote_self_upvote() {
        let (_, mut ctr) = setup(&alice());
        ctr.nominations
            .insert(&alice(), &HouseType::CouncilOfAdvisors);
        ctr.upvote(alice());
    }

    #[test]
    #[should_panic(expected = "Nomination not found")]
    fn upvote_nomination_not_found() {
        let (_, mut ctr) = setup(&bob());
        ctr.upvote(alice());
    }

    #[test]
    #[should_panic(expected = "User has already upvoted given nomination")]
    fn upvote_nomination_already_upvoted() {
        let (_, mut ctr) = setup(&bob());
        ctr.nominations
            .insert(&alice(), &HouseType::CouncilOfAdvisors);
        ctr.upvotes.insert(&(alice(), bob()));
        ctr.upvote(alice());
    }

    #[test]
    #[should_panic(expected = "Not enough gas, min: Gas(20000000000000)")]
    fn upvote_wrong_gas() {
        let (mut ctx, mut ctr) = setup(&bob());
        ctr.nominations
            .insert(&alice(), &HouseType::CouncilOfAdvisors);
        ctx.prepaid_gas = GAS_UPVOTE.sub(Gas(10));
        testing_env!(ctx.clone());
        ctr.upvote(alice());
    }

    #[test]
    #[should_panic(expected = "Not enough deposit, min: 1000000000000000000000")]
    fn upvote_wrong_deposit() {
        let (_, mut ctr) = setup(&bob());
        ctr.nominations
            .insert(&alice(), &HouseType::CouncilOfAdvisors);
        ctr.upvote(alice());
    }

    #[test]
    fn upvote() {
        let (mut ctx, mut ctr) = setup(&bob());
        ctr.nominations
            .insert(&alice(), &HouseType::CouncilOfAdvisors);
        ctx.attached_deposit = UPVOTE_COST;
        testing_env!(ctx.clone());
        ctr.upvote(alice());
    }

    #[test]
    #[should_panic(expected = "Nomination not found")]
    fn comment_nomination_not_found() {
        let (_, mut ctr) = setup(&bob());
        ctr.comment(alice(), String::from("test"));
    }

    #[test]
    #[should_panic(expected = "Not enough gas, min: Gas(20000000000000)")]
    fn comment_wrong_gas() {
        let (mut ctx, mut ctr) = setup(&bob());
        ctr.nominations
            .insert(&alice(), &HouseType::CouncilOfAdvisors);
        ctx.prepaid_gas = GAS_COMMENT.sub(Gas(10));
        testing_env!(ctx.clone());
        ctr.comment(alice(), String::from("test"));
    }

    #[test]
    fn comment() {
        let (_, mut ctr) = setup(&bob());
        ctr.nominations
            .insert(&alice(), &HouseType::CouncilOfAdvisors);
        ctr.comment(alice(), String::from("test"));
    }

    #[test]
    #[should_panic(expected = "User is not nominated, cannot revoke")]
    fn self_revoke_nomination_not_found() {
        let (_, mut ctr) = setup(&alice());
        ctr.self_revoke();
    }

    #[test]
    fn self_revoke_basic() {
        let (mut ctx, mut ctr) = setup(&bob());
        ctr.nominations
            .insert(&alice(), &HouseType::CouncilOfAdvisors);
        assert!(ctr.nominations.len() == 1);
        ctx.predecessor_account_id = alice();
        testing_env!(ctx.clone());
        ctr.self_revoke();
        assert!(ctr.nominations.is_empty());
    }

    #[test]
    fn self_revoke_flow1() {
        let (mut ctx, mut ctr) = setup(&bob());

        // add two nominations
        ctr.nominations
            .insert(&candidate(1), &HouseType::CouncilOfAdvisors);
        ctr.nominations
            .insert(&candidate(2), &HouseType::CouncilOfAdvisors);
        assert!(ctr.nominations.len() == 2);

        // upvote candidate 1 two times
        ctr.upvotes.insert(&(candidate(1), candidate(2)));
        ctr.upvotes.insert(&(candidate(1), candidate(3)));
        assert!(ctr.upvotes.len() == 2);

        // update the num of upvotes for the candidate
        ctr.upvotes_per_candidate.insert(&candidate(1), &2);
        assert_eq!(ctr.upvotes_per_candidate.get(&candidate(1)).unwrap(), 2);

        ctx.predecessor_account_id = candidate(1);
        testing_env!(ctx.clone());

        // revoke
        ctr.self_revoke();

        // make sure all the values were deleted
        assert!(ctr.nominations.len() == 1);
        assert!(ctr.upvotes.is_empty());
        assert_eq!(ctr.upvotes_per_candidate.get(&candidate(1)), None);
    }

    #[test]
    #[should_panic(
        expected = "There are no upvotes registered for this candidate from the caller account"
    )]
    fn revoke_upvote_no_upvote() {
        let (_, mut ctr) = setup(&bob());
        ctr.nominations
            .insert(&candidate(1), &HouseType::CouncilOfAdvisors);
        assert!(ctr.nominations.len() == 1);
        ctr.revoke_upvote(candidate(1));
    }

    #[test]
    fn revoke_upvote_basics() {
        let (_, mut ctr) = setup(&bob());

        // add a nomination and upvote it
        ctr.nominations
            .insert(&candidate(1), &HouseType::CouncilOfAdvisors);
        ctr.upvotes.insert(&(candidate(1), bob()));
        ctr.upvotes_per_candidate.insert(&candidate(1), &1);
        assert!(ctr.nominations.len() == 1);
        assert!(ctr.upvotes.len() == 1);
        assert!(ctr.upvotes_per_candidate.get(&candidate(1)) == Some(1));

        // revoke the upvote
        ctr.revoke_upvote(candidate(1));

        // check all the values are updated correctly
        assert!(ctr.nominations.len() == 1);
        assert!(ctr.upvotes.len() == 0);
        assert!(ctr.upvotes_per_candidate.get(&candidate(1)) == Some(0));
    }

    #[test]
    fn revoke_upvote_flow1() {
        let (mut ctx, mut ctr) = setup(&bob());

        // add two nominations and upvote them
        ctr.nominations
            .insert(&candidate(1), &HouseType::CouncilOfAdvisors);
        ctr.nominations
            .insert(&candidate(2), &HouseType::CouncilOfAdvisors);
        ctr.upvotes.insert(&(candidate(1), bob()));
        ctr.upvotes.insert(&(candidate(1), candidate(2)));
        ctr.upvotes.insert(&(candidate(2), candidate(1)));
        ctr.upvotes_per_candidate.insert(&candidate(1), &2);
        ctr.upvotes_per_candidate.insert(&candidate(2), &1);
        assert!(ctr.nominations.len() == 2);
        assert!(ctr.upvotes.len() == 3);
        assert!(ctr.upvotes_per_candidate.get(&candidate(1)) == Some(2));
        assert!(ctr.upvotes_per_candidate.get(&candidate(2)) == Some(1));

        // revoke the (candidate(1) <- bob) upvote
        ctr.revoke_upvote(candidate(1));

        // check all the values are updated correctly
        assert!(ctr.nominations.len() == 2);
        assert!(ctr.upvotes.len() == 2);
        assert!(ctr.upvotes_per_candidate.get(&candidate(1)) == Some(1));
        assert!(ctr.upvotes_per_candidate.get(&candidate(2)) == Some(1));

        // revoke the (candidate(1) <- candidate(2)) upvote
        ctx.predecessor_account_id = candidate(2);
        testing_env!(ctx.clone());
        ctr.revoke_upvote(candidate(1));

        // check all the values are updated correctly
        assert!(ctr.nominations.len() == 2);
        assert!(ctr.upvotes.len() == 1);
        assert!(ctr.upvotes_per_candidate.get(&candidate(1)) == Some(0));
        assert!(ctr.upvotes_per_candidate.get(&candidate(2)) == Some(1));
    }

    #[test]
    fn nominations() {
        let (_, mut ctr) = setup(&bob());

        let upvotes_candidate_1 = 5;
        let upvotes_candidate_2 = 3;
        let upvotes_candidate_3 = 1;

        // add 3 nominations
        ctr.nominations
            .insert(&candidate(1), &HouseType::CouncilOfAdvisors);
        ctr.nominations
            .insert(&candidate(2), &HouseType::CouncilOfAdvisors);
        ctr.nominations
            .insert(&candidate(3), &HouseType::HouseOfMerit);
        ctr.upvotes_per_candidate
            .insert(&candidate(1), &upvotes_candidate_1);
        ctr.upvotes_per_candidate
            .insert(&candidate(2), &upvotes_candidate_2);
        ctr.upvotes_per_candidate
            .insert(&candidate(3), &upvotes_candidate_3);

        // querry nominations for CouncilOfAdvisord
        let counsil_of_advisors = ctr.nominations(HouseType::CouncilOfAdvisors);
        assert!(counsil_of_advisors.len() == 2);
        assert!(counsil_of_advisors[0].0 == candidate(1));
        assert!(counsil_of_advisors[0].1 == upvotes_candidate_1);
        assert!(counsil_of_advisors[1].0 == candidate(2));
        assert!(counsil_of_advisors[1].1 == upvotes_candidate_2);

        // querry nominations for HouseOfMerit
        let counsil_of_advisors = ctr.nominations(HouseType::HouseOfMerit);
        assert!(counsil_of_advisors.len() == 1);
        assert!(counsil_of_advisors[0].0 == candidate(3));
        assert!(counsil_of_advisors[0].1 == upvotes_candidate_3);
    }
}
