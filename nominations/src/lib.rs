use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, UnorderedMap};
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault, PromiseResult};

mod constants;
mod proposal;
mod storage;
mod view;

pub use crate::constants::*;
pub use crate::proposal::*;
use crate::storage::*;

pub mod ext;
pub use crate::ext::*;

#[derive(BorshDeserialize, BorshSerialize)]
pub struct Nomination {
    nominated_by: AccountId,
    timestamp: u64,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Contract {
    pub pause: bool,
    pub gwg: AccountId,
    pub sbt_registry: AccountId,
    /// IAH issuer account for proof of humanity
    pub iah_issuer: AccountId,
    /// IAH class ID used for Facetech verification
    pub iah_class_id: u64,
    /// map of nominations
    pub nominations: UnorderedMap<AccountId, Nomination>,
    /// start and end time for the nominations
    pub start_time: u64,
    pub end_time: u64,
    /// number of nominations per user
    pub nominations_per_user: LookupMap<AccountId, u64>,
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(
        gwg: AccountId,
        sbt_registry: AccountId,
        iah_issuer: AccountId,
        iah_class_id: u64,
        start_time: u64,
        end_time: u64,
    ) -> Self {
        Self {
            pause: false,
            gwg,
            sbt_registry,
            iah_issuer,
            iah_class_id,
            start_time,
            end_time,
            nominations: UnorderedMap::new(StorageKey::Nominations),
            nominations_per_user: LookupMap::new(StorageKey::NominationsPerUser),
        }
    }

    /// creates a new nomination
    /// returns proposal ID
    pub fn nominate(
        &mut self,
        user: AccountId,
        comment: Option<String>,
        external_resource: Option<String>,
    ) {
        let nominated_by = env::predecessor_account_id();
        let storage_start = env::storage_usage();

        // check the nomination period is active
        let current_timestamp = env::block_timestamp();
        require!(
            current_timestamp <= self.end_time && current_timestamp >= self.start_time,
            format!(
                "It is not nomination period now. start_time: {:?}, end_time: {:?}, got: {:?}",
                self.start_time, self.end_time, current_timestamp
            )
        );

        require!(
            env::prepaid_gas() >= GAS_NOMINATE,
            format!("not enough gas, min: {:?}", GAS_NOMINATE)
        );

        // 3. check if user has IAH SBT
        // 4. callback -> check if user already nominated and cast the nomination
        // call SBT registry to verify IAH SBT
        ext_sbtreg::ext(self.sbt_registry.clone())
            .sbt_tokens_by_owner(
                user.clone(),
                Some(self.iah_issuer.clone()),
                Some(self.iah_class_id.clone()),
                Some(1),
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_VOTE_CALLBACK)
                    .on_nominate_verified(user, nominated_by, current_timestamp),
            );
    }

    /*****************
     * PRIVATE
     ****************/

    #[private]
    pub fn on_nominate_verified(
        &mut self,
        user: AccountId,
        nominated_by: AccountId,
        timestamp: u64,
    ) {
        match env::promise_result(0) {
            PromiseResult::NotReady => unreachable!(),
            PromiseResult::Successful(value) => {
                if let Ok(result) =
                    near_sdk::serde_json::from_slice::<Vec<(AccountId, Vec<OwnedToken>)>>(&value)
                {
                    if result.len() > 0 {
                        let nomination = Nomination {
                            nominated_by: nominated_by,
                            timestamp: timestamp,
                        };
                        // TODO: should we take any action if user is nominating the same user again? Currently basically nothing chanes
                        self.nominations.insert(&user, &nomination);
                        let num_of_nominations = self
                            .nominations_per_user
                            .get(&nomination.nominated_by)
                            .unwrap_or(0);
                        self.nominations_per_user
                            .insert(&nomination.nominated_by, &(num_of_nominations + 1));
                    }
                }
            }
            PromiseResult::Failed => env::panic_str("ERR_CALL_FAILED"),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {}
