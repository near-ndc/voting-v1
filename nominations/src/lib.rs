use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::UnorderedMap;
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault, Promise};

mod consent;
mod constants;
mod ext;
mod proposal;
mod storage;
mod view;

use crate::consent::*;
pub use crate::constants::*;
pub use crate::ext::*;
pub use crate::proposal::*;
use crate::storage::*;

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
    // TODO:
    // map of nominations
    // start + end time for nominations
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(
        gwg: AccountId,
        sbt_registry: AccountId,
        iah_issuer: AccountId,
        iah_class_id: u64,
    ) -> Self {
        Self {
            pause: false,
            gwg,
            sbt_registry,
            iah_issuer,
            iah_class_id,
        }
    }

    /// creates new empty proposal
    /// returns proposal ID
    pub fn nominate(&mut self, nominatee: AccountId) -> u32 {
        // 1. Storage fee
        // 2. check if in start / end time
        // 3. check if use has IAH SBT
        // 4. callback -> check if user already nominated and cast the nomination

        require!(
            env::prepaid_gas() >= GAS_VOTE,
            format!("not enough gas, min: {:?}", GAS_VOTE)
        );

        // call SBT registry to verify IAH SBT
        ext_sbtreg::ext(self.sbt_registry.clone())
            .sbt_tokens_by_owner(
                user.clone(),
                Some(self.iah_issuer.clone()),
                Some(self.iah_class_id.clone()),
                Some(1),
            )
            .then(
                ext_self::ext(env::current_account_id())
                    .with_static_gas(GAS_VOTE_CALLBACK)
                    .on_nominate_verified(prop_id, user, vote),
            )
    }

    /*****************
     * PRIVATE
     ****************/

    #[private]
    pub fn on_nominate_verified(&mut self, prop_id: u32, user: AccountId, vote: Vote) {
        let mut p = self._proposal(prop_id);
        p.vote_on_verified(&user, vote);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {}
