use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, UnorderedSet};
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault, PromiseResult};

mod constants;
mod storage;

pub use crate::constants::*;
use crate::storage::*;

pub mod ext;
pub use crate::ext::*;

#[derive(BorshDeserialize, BorshSerialize)]
pub struct NominationKey {
    nominator: AccountId,
    nominee: AccountId,
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Contract {
    pub sbt_registry: AccountId,
    /// IAH issuer account for proof of humanity
    pub iah_issuer: AccountId,
    /// IAH class ID used for Facetech verification
    pub iah_class_id: u64,
    /// map of nominations (nominator -> nominee)
    pub nominations: UnorderedSet<NominationKey>,
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
        sbt_registry: AccountId,
        iah_issuer: AccountId,
        iah_class_id: u64,
        start_time: u64,
        end_time: u64,
    ) -> Self {
        Self {
            sbt_registry,
            iah_issuer,
            iah_class_id,
            start_time,
            end_time,
            nominations: UnorderedSet::new(StorageKey::Nominations),
            nominations_per_user: LookupMap::new(StorageKey::NominationsPerUser),
        }
    }
    // returns the number of nominations per user. If the user has not been nomianted once returns 0
    pub fn nominations_per_user(&self, user: AccountId) -> u64 {
        self.nominations_per_user.get(&user).unwrap_or(0)
    }

    /// nominate method allows to submit nominatios by verified humans
    /// + Checks if the nominator is a verified human
    /// + Checks if the pair (nominator, nominee) has been already submitted
    /// + Checks if the nomination was submitted during the nomination period
    pub fn nominate(
        &mut self,
        nominee: AccountId,
        comment: Option<String>,
        external_resource: Option<String>,
    ) {
        let nominator = env::predecessor_account_id();

        // check the nomination period is active
        let current_timestamp = env::block_timestamp();
        require!(
            current_timestamp <= self.end_time && current_timestamp >= self.start_time,
            format!(
                "it is not nomination period now. start_time: {:?}, end_time: {:?}, got: {:?}",
                self.start_time, self.end_time, current_timestamp
            )
        );

        require!(
            env::prepaid_gas() >= GAS_NOMINATE,
            format!("not enough gas, min: {:?}", GAS_NOMINATE)
        );

        // call SBT registry to verify IAH SBT and cast the nomination is callback based on the return from sbt_tokens_by_owner
        ext_sbtreg::ext(self.sbt_registry.clone())
            .sbt_tokens_by_owner(
                nominee.clone(),
                Some(self.iah_issuer.clone()),
                Some(self.iah_class_id.clone()),
                Some(1),
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_VOTE_CALLBACK)
                    .on_nominate_verified(nominator, nominee),
            );
    }

    /*****************
     * PRIVATE
     ****************/

    #[private]
    pub fn on_nominate_verified(&mut self, nominator: AccountId, nominee: AccountId) {
        match env::promise_result(0) {
            PromiseResult::NotReady => unreachable!(),
            PromiseResult::Successful(value) => {
                if let Ok(result) =
                    near_sdk::serde_json::from_slice::<Vec<(AccountId, Vec<OwnedToken>)>>(&value)
                {
                    // if len > 0 then its human
                    if result.len() > 0 {
                        let nomination = NominationKey { nominator, nominee };
                        if !self.nominations.contains(&nomination) {
                            let num_of_nominations = self
                                .nominations_per_user
                                .get(&nomination.nominee)
                                .unwrap_or(0);
                            self.nominations_per_user
                                .insert(&nomination.nominee, &(num_of_nominations + 1));
                            self.nominations.insert(&nomination);
                        } else {
                            env::panic_str("this nomination has been already submitted");
                        }
                    }
                }
            }
            PromiseResult::Failed => env::panic_str("sbt_tokens_by_owner call failed"),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {}
