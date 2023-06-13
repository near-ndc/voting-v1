use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, LookupMap, UnorderedSet};
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
    /// IAH class ID used for Facetech verification
    pub iah_class_id: u64,
    /// OG token class ID
    pub og_class_id: u64,
    /// map of nominations
    pub nominations: LookupMap<AccountId, HouseType>,
    /// set of pairs (candidate, upvoter)
    pub upvotes: UnorderedSet<(AccountId, AccountId)>,
    /// number of upvotes per candidate
    pub upvotes_per_candidate: LookupMap<AccountId, u64>,
    /// used for backend key rotation
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
        iah_class_id: u64,
        og_class_id: u64,
        admins: Vec<AccountId>,
        start_time: u64,
        end_time: u64,
    ) -> Self {
        Self {
            sbt_registry,
            iah_issuer,
            iah_class_id,
            og_class_id,
            start_time,
            end_time,
            nominations: LookupMap::new(StorageKey::Nominations),
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
    fn nominations(&self, house: HouseType) -> Vec<(AccountId, u32)> {
        // TODO: add implementation for the query
        env::panic_str("not implemented");
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
            !self.nominations.contains_key(&nominee),
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

        // call SBT registry to verify IAH/ OG SBT and cast the nomination in callback based on the return from sbt_tokens_by_owner
        ext_sbtreg::ext(self.sbt_registry.clone())
            .sbt_tokens_by_owner(
                // TODO: Once the is_verified method in registry is implemented use it instead
                nominee.clone(),
                Some(self.iah_issuer.clone()),
                Some(self.iah_class_id.clone()),
                None,
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

        require!(
            self.nominations.contains_key(&candidate),
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
            .sbt_tokens_by_owner(
                // TODO: Once the is_verified method in registry is implemented use it instead
                upvoter.clone(),
                Some(self.iah_issuer.clone()),
                Some(self.iah_class_id.clone()),
                Some(1),
                Some(true),
            )
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
            self.nominations.contains_key(&candidate),
            "Nomination does not exist",
        );

        require!(
            env::prepaid_gas() >= GAS_COMMENT,
            format!("Not enough gas, min: {:?}", GAS_COMMENT)
        );

        // call SBT registry to verify IAH/ OG SBT and cast the nomination in callback based on the return from sbt_tokens_by_owner
        ext_sbtreg::ext(self.sbt_registry.clone())
            .sbt_tokens_by_owner(
                // TODO: Once the is_verified method in registry is implemented use it instead
                commenter.clone(),
                Some(self.iah_issuer.clone()),
                Some(self.iah_class_id.clone()),
                Some(1),
                Some(true),
            )
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
            self.nominations.contains_key(&nominee),
            "User is not nominated, cannot revoke",
        );

        self.nominations.remove(&nominee);
        self.upvotes_per_candidate.remove(&nominee);

        let mut keys_to_remove: Vec<(AccountId, AccountId)> = Vec::new();

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
        #[callback_unwrap] val: Vec<(AccountId, Vec<OwnedToken>)>,
        nominee: AccountId,
        upvoter: AccountId,
    ) {
        if val.is_empty() {
            env::panic_str("Not a verified human member, or the tokens are expired");
        }
        let num_of_upvotes = self.upvotes_per_candidate.get(&nominee).unwrap_or(0);
        self.upvotes_per_candidate
            .insert(&nominee, &(num_of_upvotes + 1));
        self.upvotes.insert(&(nominee, upvoter));
    }

    /// Checks If the commenter is a verified human otherwise panics
    #[private]
    pub fn on_comment_verified(
        &mut self,
        #[callback_unwrap] val: Vec<(AccountId, Vec<OwnedToken>)>,
    ) {
        if val.is_empty() {
            env::panic_str("Not a verified human member, or the tokens are expired");
        }
        // TODO: what should be done in the case we do not register the comments on-chain?
    }

    ///Checks If the caller is a verified human and OG token holder and registers the nomination otherwise panics
    #[private]
    pub fn on_nominate_verified(
        &mut self,
        #[callback_unwrap] val: Vec<(AccountId, Vec<OwnedToken>)>,
        nominee: AccountId,
        house_type: HouseType,
    ) {
        // TODO: Once the is_verified method in registry is implemented use it instead
        // verify human and og token holder
        let mut iah_verified = false;
        let mut og_verified = false;
        for token in val[0].1.iter() {
            if token.metadata.class == self.iah_class_id {
                iah_verified = true;
            }
            if token.metadata.class == self.og_class_id {
                og_verified = true;
            }
        }
        if !iah_verified && !og_verified {
            env::panic_str("Not a verified human/OG member, or the tokens are expired");
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
mod tests {}
