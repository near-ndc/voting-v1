use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LazyOption, LookupMap, UnorderedSet};
use near_sdk::{env, near_bindgen, require, AccountId, PanicOnDefault, Promise};

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
    /// map of nominations
    pub nominations: LookupMap<NominationKey, HouseType>,
    /// map of upvotes -> number of received upvotes per nomination
    pub upvotes: UnorderedSet<(NominationKey, AccountId)>,
    pub num_upvotes: LookupMap<AccountId, u64>,
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
            nominations: LookupMap::new(StorageKey::Nominations),
            upvotes: UnorderedSet::new(StorageKey::Upvotes),
            num_upvotes: LookupMap::new(StorageKey::NumUpvotes),
            campaigns: LookupMap::new(StorageKey::Campaigns),
            campaign_counter: 0,
            admins: LazyOption::new(StorageKey::Admins, Some(&admins)),
        }
    }

    /**********
     * QUERIES
     **********/

    /// returns the number of upvotes per nomination. If the nomination has not been upvoted returns 0
    pub fn upvotes_per_nomination(&self, nominee: AccountId) -> u64 {
        self.num_upvotes.get(&nominee).unwrap_or(0)
    }

    /**********
     * TRANSACTIONS
     **********/

    pub fn add_campaign(&mut self, name: String, link: String, start_time: u64, end_time: u64) {
        if let Some(admins) = self.admins.get() {
            let caller = env::predecessor_account_id();
            require!(admins.contains(&caller), "not authoirized");
        }
        let storage_start = env::storage_usage();
        require!(
            name.len() <= MAX_CAMPAIGN_LEN && link.len() <= MAX_CAMPAIGN_LEN,
            "max name and link length is 200 characters"
        );
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
    /// + Checks if the caller is a verified human
    /// + Check if the caller is a OG member
    /// + Checks if the nomination has been already submitted
    /// + Checks if the user has nominated themselves to a different house before
    /// + Checks if the nomination was submitted during the nomination period
    pub fn self_nominate(
        &mut self,
        campaign: u32,
        house_type: HouseType,
        #[allow(unused_variables)] comment: String,
        #[allow(unused_variables)] external_resource: Option<String>,
    ) -> Promise {
        let nominee = env::predecessor_account_id();
        let c = self
            .campaigns
            .get(&campaign)
            .expect("campaign ID not found");

        c.assert_active();

        let nomination_key = NominationKey {
            campaign,
            nominee: nominee.clone(),
        };
        require!(
            !self.nominations.contains_key(&nomination_key),
            "User has already nominated themselves to a different house",
        );

        require!(
            env::prepaid_gas() >= GAS_NOMINATE,
            format!("not enough gas, min: {:?}", GAS_NOMINATE)
        );

        // call SBT registry to verify IAH SBT and cast the nomination is callback based on the return from sbt_tokens_by_owner
        // TODO: add check for the sbt og token
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
                    .on_nominate_verified(nomination_key, house_type),
            )
    }

    pub fn upvote(
        &mut self,
        campaign: u32,
        candidate: AccountId,
        #[allow(unused_variables)] comment: String,
        #[allow(unused_variables)] external_resource: Option<String>,
    ) -> Promise {
        let upvoter = env::predecessor_account_id();
        let c = self
            .campaigns
            .get(&campaign)
            .expect("campaign ID not found");

        c.assert_active();

        let nomination_key = NominationKey {
            campaign,
            nominee: candidate.clone(),
        };
        require!(
            self.nominations.contains_key(&nomination_key),
            "Nomination not found",
        );

        require!(
            self.upvotes
                .contains(&(nomination_key.clone(), upvoter.clone())),
            "User has already upvoted given nomination"
        );

        // call SBT registry to verify IAH SBT and cast the nomination is callback based on the return from sbt_tokens_by_owner
        // TODO: add check for the sbt og token
        ext_sbtreg::ext(self.sbt_registry.clone())
            .sbt_tokens_by_owner(
                upvoter.clone(),
                Some(self.iah_issuer.clone()),
                Some(self.iah_class_id.clone()),
                Some(1),
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_VOTE_CALLBACK)
                    .on_upvote_verified(nomination_key, upvoter),
            )
    }

    /// Revokes callers nominatnion and all the upvotes of that specific nomination
    pub fn self_revoke(&mut self, campaign: u32) {
        let nominee = env::predecessor_account_id();
        let c = self
            .campaigns
            .get(&campaign)
            .expect("campaign ID not found");

        c.assert_active();

        let nomination_key = NominationKey {
            campaign,
            nominee: nominee.clone(),
        };
        require!(
            self.nominations.contains_key(&nomination_key),
            "User is not nominated, cannot revoke",
        );

        self.nominations.remove(&nomination_key);
        self.num_upvotes.remove(&nomination_key.nominee);

        let mut keys_to_remove: Vec<(NominationKey, AccountId)> = Vec::new();

        for upvote in self.upvotes.iter() {
            if upvote.0 == nomination_key.clone() {
                keys_to_remove.push(upvote);
            }
        }
        for key in keys_to_remove.iter() {
            self.upvotes.remove(key);
        }
    }

    pub fn revoke_upvote(&mut self, candidate: AccountId, campaign: u32) {
        let caller = env::predecessor_account_id();
        let nomination_key = NominationKey {
            campaign,
            nominee: candidate.clone(),
        };
        if self.upvotes.remove(&(nomination_key, caller)) {
            let num_of_upvotes = self.num_upvotes.get(&candidate).unwrap_or(1); //we do 1 so the results will be at least 0
            self.num_upvotes.insert(&candidate, &(num_of_upvotes - 1));
        } else {
            env::panic_str(
                "There are no upvotes registered for this candidate from the caller account",
            );
        }
    }

    /*****************
     * PRIVATE
     ****************/

    /// If the upvoter is a verified human registes the upvote otherwise panics
    #[private]
    pub fn on_upvote_verified(
        &mut self,
        #[callback_unwrap] val: Vec<(AccountId, Vec<OwnedToken>)>,
        nomination_key: NominationKey,
        upvoter: AccountId,
    ) {
        if val.is_empty() {
            env::panic_str("Not a verified human, or the token has expired");
        }
        let num_of_upvotes = self.num_upvotes.get(&nomination_key.nominee).unwrap_or(0);
        self.num_upvotes
            .insert(&nomination_key.nominee, &(num_of_upvotes + 1));
        self.upvotes.insert(&(nomination_key, upvoter));
    }

    #[private]
    pub fn on_nominate_verified(
        &mut self,
        #[callback_unwrap] val: Vec<(AccountId, Vec<OwnedToken>)>,
        nomination_key: NominationKey,
        house_type: HouseType,
    ) {
        if val.is_empty() {
            // TODO: add check for the OG token
            env::panic_str("Not a verified human, or the token has expired");
        }
        self.nominations.insert(&nomination_key, &house_type);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {}
