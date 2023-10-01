use near_sdk::json_types::U128;
use near_sdk::{ext_contract, AccountId};

#[ext_contract(ext_self)]
pub trait ExtSelf {
    fn on_execute(&mut self, prop_id: u32, budget: U128);

    fn on_ban(&mut self, prop_id: u32, member: AccountId, receiver_id: AccountId);

    fn on_remove(&mut self, prop_id: u32);
}
