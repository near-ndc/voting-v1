use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, LookupMap, UnorderedMap};
use near_sdk::env::panic_str;
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
            self.upvotes
                .get(&(candidate.clone(), upvoter.clone()))
                .is_some(),
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

        // call SBT registry to verify IAH/ OG SBT and cast the upvote in callback based on the
        // return from sbt_tokens_by_owner
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
        n.upvotes -= 1;
        self.nominations.insert(&candidate, &n);

        match self.upvotes.remove(&(candidate, caller)) {
            None => panic_str("upvote doesn't exist"),
            Some(t) => require!(n.timestamp <= t, "upvote not valid, candidate revoked"),
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
            .expect("not a valid candidate");
        n.upvotes -= 1;
        self.nominations.insert(&candidate, &n);
        if let Some(t) = self
            .upvotes
            .insert(&(candidate, upvoter), &env::block_timestamp_ms())
        {
            require!(t < n.timestamp, "nomination already upvoted");
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
            timestamp: timestamp * MSECOND,
            upvotes: 0,
        }
    }

    /// creates and inserts default nomination
    fn insert_def_nomination(ctr: &mut Contract) {
        ctr.nominations.insert(
            &alice(),
            &mk_nomination(HouseType::CouncilOfAdvisors, START),
        );
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
        insert_def_nomination(&mut ctr);
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
        insert_def_nomination(&mut ctr);
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
        insert_def_nomination(&mut ctr);
        ctr.upvotes.insert(&(alice(), bob()), &START);
        ctr.upvote(alice());
    }

    #[test]
    fn upvote_after_revoke() {
        let (_, mut ctr) = setup(&bob());
        insert_def_nomination(&mut ctr);
        ctr.upvotes.insert(&(alice(), bob()), &START);
        ctr.upvote(alice());
        let n = ctr.nominations.get(&alice()).unwrap();
        assert_eq!(n.upvotes, 1);
    }

    #[test]
    #[should_panic(expected = "Not enough gas, min: Gas(20000000000000)")]
    fn upvote_wrong_gas() {
        let (mut ctx, mut ctr) = setup(&bob());
        insert_def_nomination(&mut ctr);
        ctx.prepaid_gas = GAS_UPVOTE.sub(Gas(10));
        testing_env!(ctx.clone());
        ctr.upvote(alice());
    }

    #[test]
    #[should_panic(expected = "Not enough deposit, min: 1000000000000000000000")]
    fn upvote_wrong_deposit() {
        let (_, mut ctr) = setup(&bob());
        insert_def_nomination(&mut ctr);
        ctr.upvote(alice());
    }

    #[test]
    fn upvote() {
        let (mut ctx, mut ctr) = setup(&bob());
        ctx.attached_deposit = UPVOTE_COST;
        testing_env!(ctx.clone());

        insert_def_nomination(&mut ctr);
        ctr.upvote(alice());
        let n = ctr.nominations.get(&alice()).unwrap();
        assert_eq!(n.upvotes, 1);

        // make another upvote
        ctx.predecessor_account_id = candidate(1);
        testing_env!(ctx.clone());
        ctr.upvote(alice());
        let n = ctr.nominations.get(&alice()).unwrap();
        assert_eq!(n.upvotes, 2);
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
        insert_def_nomination(&mut ctr);
        ctr.comment(alice(), String::from("test"));
    }

    #[test]
    fn comment() {
        let (_, mut ctr) = setup(&bob());
        insert_def_nomination(&mut ctr);
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
        insert_def_nomination(&mut ctr);
        assert!(ctr.nominations.len() == 1);
        ctx.predecessor_account_id = alice();
        testing_env!(ctx.clone());
        ctr.self_revoke();
        assert!(ctr.nominations.is_empty());
    }

    #[test]
    fn self_revoke_flow1() {
        let (mut ctx, mut ctr) = setup(&bob());

        // TODO: For the proper flow test we need to use the "private" functions, rather
        // than hacking the state.
    }

    #[test]
    #[should_panic(expected = "caller didn't upvote the candidate")]
    fn remove_upvote_no_upvote() {
        let (_, mut ctr) = setup(&bob());
        insert_def_nomination(&mut ctr);
        ctr.remove_upvote(alice());
    }

    #[test]
    fn remove_upvote_basics() {
        let (_, mut ctr) = setup(&bob());

        // TODO use contract functions to manipulate the state
        /*
        // add a nomination and upvote it
        ctr.nominations
            .insert(&candidate(1), &HouseType::CouncilOfAdvisors);
        ctr.upvotes.insert(&(candidate(1), bob()));
        ctr.upvotes_per_candidate.insert(&candidate(1), &1);
        assert!(ctr.nominations.len() == 1);
        assert!(ctr.upvotes.len() == 1);
        assert!(ctr.upvotes_per_candidate.get(&candidate(1)) == Some(1));

        // remove the upvote
        ctr.remove_upvote(candidate(1));

        // check all the values are updated correctly
        assert!(ctr.nominations.len() == 1);
        assert!(ctr.upvotes.len() == 0);
        assert!(ctr.upvotes_per_candidate.get(&candidate(1)) == Some(0));
         */
    }

    #[test]
    fn remove_upvote_flow1() {
        let (mut ctx, mut ctr) = setup(&bob());

        // This flow shoud also use the function rather than hacking the state
        /*
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

            // remove the (candidate(1) <- bob) upvote
            ctr.remove_upvote(candidate(1));

            // check all the values are updated correctly
            assert!(ctr.nominations.len() == 2);
            assert!(ctr.upvotes.len() == 2);
            assert!(ctr.upvotes_per_candidate.get(&candidate(1)) == Some(1));
            assert!(ctr.upvotes_per_candidate.get(&candidate(2)) == Some(1));

            // remove the (candidate(1) <- candidate(2)) upvote
            ctx.predecessor_account_id = candidate(2);
            testing_env!(ctx.clone());
            ctr.remove_upvote(candidate(1));

            // check all the values are updated correctly
            assert!(ctr.nominations.len() == 2);
            assert!(ctr.upvotes.len() == 1);
            assert!(ctr.upvotes_per_candidate.get(&candidate(1)) == Some(0));
            assert!(ctr.upvotes_per_candidate.get(&candidate(2)) == Some(1));
        */
    }

    #[test]
    fn nominations() {
        // let (_, mut ctr) = setup(&bob());

        // TODO: this flow should use functions to create nominations.
    }
}
