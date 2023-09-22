use near_sdk::ext_contract;
use near_sdk::json_types::U128;

#[ext_contract(ext_self)]
pub trait ExtSelf {
    fn on_execute(&mut self, prop_id: u32, budget: U128);
}
