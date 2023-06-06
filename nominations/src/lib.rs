use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, LookupMap, LookupSet};
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault, PromiseResult};

mod constants;
mod storage;

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
    /// IAH class ID used for Facetech verification
    pub iah_class_id: u64,
    /// map of nominations (nominator -> nominee)
    pub nominations: LookupSet<NominationKey>,
    /// map of `(campaign, nominee)` => number of received nominations
    pub nominations_sum: LookupMap<(u32, AccountId), u64>,
    pub campaigns: LookupMap<u32, Campaign>,
    pub campaign_counter: u32,

    /// used for backend key rotation
    pub admins: LazyOption<Vec<AccountId>>,
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(
        sbt_registry: AccountId,
        iah_issuer: AccountId,
        iah_class_id: u64,
        admins: Vec<AccountId>,
    ) -> Self {
        Self {
            sbt_registry,
            iah_issuer,
            iah_class_id,
            nominations: LookupSet::new(StorageKey::Nominations),
            nominations_sum: LookupMap::new(StorageKey::NominationsPerUser),
            campaigns: LookupMap::new(StorageKey::Campaigns),
            campaign_counter: 0,
            admins: LazyOption::new(StorageKey::Admins, Some(&admins)),
        }
    }

    /**********
     * QUERIES
     **********/

    /// returns the number of nominations per user. If the user has not been nomianted once returns 0
    pub fn nominations_per_user(&self, campaign: u32, nominee: AccountId) -> u64 {
        self.nominations_sum.get(&(campaign, nominee)).unwrap_or(0)
    }

    /**********
     * FUNCTIONS
     **********/

    pub fn add_campaign(&mut self, name: String, link: String, start_time: u64, end_time: u64) {
        if let Some(admins) = self.admins.get() {
            let caller = env::predecessor_account_id();
            require!(admins.contains(&caller), "not authoirized");
        }
        let storage_start = env::storage_usage();
        let c = Campaign {
            name,
            link,
            start_time,
            end_time,
        };
        self.campaign_counter += 1;
        self.campaigns.insert(&self.campaign_counter, &c);

        let storage_usage = env::storage_usage();
        let required_deposit = (storage_usage - storage_start) as u128 * env::storage_byte_cost();
        require!(
            env::attached_deposit() >= required_deposit,
            format!(
                "not enough NEAR for storage depost, required: {}",
                required_deposit
            )
        );
    }

    /// nominate method allows to submit nominatios by verified humans
    /// + Checks if the nominator is a verified human
    /// + Checks if the pair (nominator, nominee) has been already submitted
    /// + Checks if the nomination was submitted during the nomination period
    pub fn nominate(
        &mut self,
        campaign: u32,
        nominee: AccountId,
        #[allow(unused_variables)] comment: String,
        #[allow(unused_variables)] external_resource: Option<String>,
    ) {
        let nominator = env::predecessor_account_id();
        let c = self
            .campaigns
            .get(&campaign)
            .expect("campaign ID not found");

        c.assert_active();
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
                    .on_nominate_verified(campaign, nominator, nominee),
            );
    }

    /*****************
     * PRIVATE
     ****************/

    #[private]
    pub fn on_nominate_verified(
        &mut self,
        campaign: u32,
        nominator: AccountId,
        nominee: AccountId,
    ) {
        match env::promise_result(0) {
            PromiseResult::NotReady => unreachable!(),
            PromiseResult::Successful(value) => {
                if let Ok(result) =
                    near_sdk::serde_json::from_slice::<Vec<(AccountId, Vec<OwnedToken>)>>(&value)
                {
                    // must have SBT tokens
                    require!(result.len() > 0, "not a human");
                    let nomination_key = NominationKey {
                        campaign,
                        nominator,
                        nominee: nominee.clone(),
                    };
                    if !self.nominations.contains(&nomination_key) {
                        let key = &(campaign, nominee);
                        let num_of_nominations = self.nominations_sum.get(&key).unwrap_or(0);
                        self.nominations_sum.insert(&key, &(num_of_nominations + 1));
                        self.nominations.insert(&nomination_key);
                    }
                }
            }
            PromiseResult::Failed => env::panic_str("sbt_tokens_by_owner call failed"),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {}
