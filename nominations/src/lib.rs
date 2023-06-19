use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, LookupMap, UnorderedMap};
use near_sdk::env::panic_str;
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault, Promise};

mod constants;
pub mod storage;

pub use crate::constants::*;
use crate::storage::*;

pub mod ext;
pub use crate::ext::*;

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Contract {
    pub sbt_registry: AccountId,
    /// IAH issuer account for proof of humanity
    pub iah_issuer: AccountId,
    /// OG token (issuer, class_id)
    pub og_class: (AccountId, u64),
    /// map of nominations
    pub nominations: UnorderedMap<AccountId, Nomination>,
    /// map (candidate, upvoter) -> timestamp
    pub upvotes: LookupMap<(AccountId, AccountId), u64>,
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
            upvotes: LookupMap::new(StorageKey::Upvotes),
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
        for n in self.nominations.iter() {
            if n.1.house == house {
                results.push((n.0, n.1.upvotes));
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
    #[payable]
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
            "User has already an active self-nomination",
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
                Some(false),
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
    #[payable]
    pub fn upvote(&mut self, candidate: AccountId) -> Promise {
        self.assert_active();
        let upvoter = env::predecessor_account_id();

        require!(upvoter != candidate, "Cannot upvote your own nomination");
        require!(
            self.nominations.get(&candidate).is_some(),
            "Nomination not found",
        );
        require!(
            env::prepaid_gas() >= GAS_UPVOTE,
            format!("Not enough gas, min: {:?}", GAS_UPVOTE)
        );
        require!(
            env::attached_deposit() >= UPVOTE_COST,
            format!("Not enough deposit, min: {:?}", UPVOTE_COST)
        );

        // call SBT registry to verify IAH and cast the upvote in callback
        ext_sbtreg::ext(self.sbt_registry.clone())
            .is_human(upvoter.clone())
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_UPVOTE)
                    .on_upvote_verified(candidate, upvoter),
            )
    }

    /// comment allows users to comment on a existing nomination
    /// + Checks if the caller is a verified human
    /// + Checks if there is a nomination for the given candidate
    /// + Checks if the nomination period is active
    pub fn comment(
        &mut self,
        candidate: AccountId,
        #[allow(unused_variables)] comment: String,
    ) -> Promise {
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

        // call SBT registry to verify IAH and validate comment in the callback
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
    }

    /// Remove the upvote
    /// + Checks if the nomination period is active
    /// + Checks if the caller upvoted the `candidate` before
    pub fn remove_upvote(&mut self, candidate: AccountId) {
        self.assert_active();
        let caller = env::predecessor_account_id();
        let mut n = self
            .nominations
            .get(&candidate)
            .expect("not a valid candidate");

        match self.upvotes.remove(&(candidate.clone(), caller)) {
            None => panic_str("upvote doesn't exist"),
            Some(t) => require!(n.timestamp <= t, "upvote not valid, candidate revoked"),
        }
        n.upvotes -= 1;
        self.nominations.insert(&candidate, &n);
    }

    /*****************
     * PRIVATE
     ****************/

    /// Checks If the upvoter is a verified human registers the upvote otherwise panics
    #[private]
    pub fn on_upvote_verified(
        &mut self,
        #[callback_unwrap] is_human: bool,
        candidate: AccountId,
        upvoter: AccountId,
    ) {
        require!(
            is_human,
            "Not a verified human member, or the tokens are expired"
        );
        let mut n = self
            .nominations
            .get(&candidate)
            .expect("Not a valid candidate");
        n.upvotes += 1;
        self.nominations.insert(&candidate, &n);
        if let Some(t) = self
            .upvotes
            .insert(&(candidate, upvoter), &env::block_timestamp_ms())
        {
            require!(t < n.timestamp, "Nomination already upvoted");
        }
    }

    /// Checks If the commenter is a verified human otherwise panics
    #[private]
    pub fn on_comment_verified(&mut self, #[callback_unwrap] is_human: bool) {
        require!(
            is_human,
            "Not a verified human member, or the tokens are expired"
        );
        // we don't record anything. Comments are handled by indexer.
    }

    ///Checks If the caller is a OG token holder and registers the nomination otherwise panics
    #[private]
    pub fn on_nominate_verified(
        &mut self,
        #[callback_unwrap] sbts: Vec<(AccountId, Vec<OwnedToken>)>,
        nominee: AccountId,
        house_type: HouseType,
    ) {
        require!(
            !sbts.is_empty() && sbts[0].1[0].metadata.class == self.og_class.1,
            "Not a verified OG member, or the token is expired",
        );

        let n = Nomination {
            house: house_type,
            timestamp: env::block_timestamp_ms(),
            upvotes: 0,
        };
        require!(
            self.nominations.insert(&nominee, &n).is_none(),
            "User has already nominated themselves",
        );
    }

    fn assert_active(&self) {
        let current_timestamp = env::block_timestamp();
        require!(
            self.start_time < current_timestamp && current_timestamp <= self.end_time,
            "Nominations time is not active"
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::ops::Sub;

    use near_sdk::{test_utils::VMContextBuilder, testing_env, AccountId, Gas, VMContext};

    use super::*;

    const START: u64 = 10;
    const SECOND: u64 = 1_000_000;
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
        AccountId::new_unchecked("og.near".to_string())
    }

    fn mk_nomination(house: HouseType, timestamp: u64) -> Nomination {
        Nomination {
            house,
            timestamp: timestamp * SECOND,
            upvotes: 0,
        }
    }

    /// creates and inserts default nomination
    fn insert_nomination(ctr: &mut Contract, candidate: AccountId, house: Option<HouseType>) {
        let house = house.unwrap_or(HouseType::CouncilOfAdvisors);
        ctr.nominations
            .insert(&candidate, &mk_nomination(house, START));
    }

    /// inserts a upvote for a specified candidate
    fn insert_upvote(ctr: &mut Contract, upvoter: AccountId, candidate: AccountId) {
        ctr.upvotes
            .insert(&(candidate.clone(), upvoter), &((START + 10) * SECOND));
        let mut nomination = ctr
            .nominations
            .get(&candidate)
            .expect("Nomination not found");
        nomination.upvotes += 1;
        ctr.nominations.insert(&candidate, &nomination);
    }

    fn setup(predecessor: &AccountId) -> (VMContext, Contract) {
        let mut ctx = VMContextBuilder::new()
            .predecessor_account_id(admin())
            // .attached_deposit(deposit_dec.into())
            .block_timestamp((START + 1) * SECOND)
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
    #[should_panic(expected = "User has already an active self-nomination")]
    fn self_nominate_already_nominated() {
        let (_, mut ctr) = setup(&alice());
        insert_nomination(&mut ctr, alice(), None);
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
        insert_nomination(&mut ctr, alice(), None);
        ctr.upvote(alice());
    }

    #[test]
    #[should_panic(expected = "Nomination not found")]
    fn upvote_nomination_not_found() {
        let (_, mut ctr) = setup(&bob());
        ctr.upvote(alice());
    }

    #[test]
    #[should_panic(expected = "Nomination not found")]
    fn upvote_after_revoke() {
        let (mut ctx, mut ctr) = setup(&alice());
        insert_nomination(&mut ctr, alice(), None);
        ctr.self_revoke();

        ctx.predecessor_account_id = bob();
        testing_env!(ctx.clone());
        ctr.upvote(alice());
    }

    #[test]
    #[should_panic(expected = "Not enough gas, min: Gas(20000000000000)")]
    fn upvote_wrong_gas() {
        let (mut ctx, mut ctr) = setup(&bob());
        insert_nomination(&mut ctr, alice(), None);
        ctx.prepaid_gas = GAS_UPVOTE.sub(Gas(10));
        testing_env!(ctx.clone());
        ctr.upvote(alice());
    }

    #[test]
    #[should_panic(expected = "Not enough deposit, min: 1000000000000000000000")]
    fn upvote_wrong_deposit() {
        let (_, mut ctr) = setup(&bob());
        insert_nomination(&mut ctr, alice(), None);
        ctr.upvote(alice());
    }

    #[test]
    fn upvote() {
        let (mut ctx, mut ctr) = setup(&bob());
        ctx.attached_deposit = UPVOTE_COST;
        testing_env!(ctx.clone());

        insert_nomination(&mut ctr, alice(), None);
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
        ctx.prepaid_gas = GAS_COMMENT.sub(Gas(10));
        testing_env!(ctx.clone());
        insert_nomination(&mut ctr, alice(), None);
        ctr.comment(alice(), String::from("test"));
    }

    #[test]
    fn comment() {
        let (_, mut ctr) = setup(&bob());
        insert_nomination(&mut ctr, alice(), None);
        ctr.comment(alice(), String::from("test"));
    }

    #[test]
    #[should_panic(expected = "User is not nominated, cannot revoke")]
    fn self_revoke_nomination_not_found() {
        let (_, mut ctr) = setup(&alice());
        ctr.self_revoke();
    }

    #[test]
    fn self_revoke() {
        let (mut ctx, mut ctr) = setup(&bob());
        insert_nomination(&mut ctr, alice(), None);
        assert!(ctr.nominations.len() == 1);
        ctx.predecessor_account_id = alice();
        testing_env!(ctx.clone());
        ctr.self_revoke();
        assert!(ctr.nominations.is_empty());
    }

    #[test]
    #[should_panic(expected = "upvote doesn't exist")]
    fn remove_upvote_no_upvote() {
        let (_, mut ctr) = setup(&bob());
        insert_nomination(&mut ctr, alice(), None);
        ctr.remove_upvote(alice());
    }

    #[test]
    fn remove_upvote() {
        let (_, mut ctr) = setup(&bob());

        // add a nomination and upvote it
        insert_nomination(&mut ctr, candidate(1), None);
        insert_upvote(&mut ctr, bob(), candidate(1));
        assert!(ctr.nominations.len() == 1);
        assert!(ctr.nominations.get(&candidate(1)).unwrap().upvotes == 1);
        assert!(ctr.upvotes.contains_key(&(candidate(1), bob())));

        // remove the upvote
        ctr.remove_upvote(candidate(1));

        // check all the values are updated correctly
        assert!(ctr.nominations.len() == 1);
        assert!(ctr.nominations.get(&candidate(1)).unwrap().upvotes == 0);
        assert!(!ctr.upvotes.contains_key(&(candidate(1), bob())));
    }

    #[test]
    fn nominations() {
        let (_, mut ctr) = setup(&bob());
        let upvotes_candidate_1 = 3;
        let upvotes_candidate_2 = 1;
        let upvotes_candidate_3 = 0;

        // add 3 nominations
        insert_nomination(&mut ctr, candidate(1), Some(HouseType::CouncilOfAdvisors));
        insert_nomination(&mut ctr, candidate(2), Some(HouseType::CouncilOfAdvisors));
        insert_nomination(&mut ctr, candidate(3), Some(HouseType::HouseOfMerit));
        // upvote candidate
        insert_upvote(&mut ctr, candidate(2), candidate(1));
        insert_upvote(&mut ctr, candidate(3), candidate(1));
        insert_upvote(&mut ctr, candidate(4), candidate(1));
        insert_upvote(&mut ctr, candidate(4), candidate(2));

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
