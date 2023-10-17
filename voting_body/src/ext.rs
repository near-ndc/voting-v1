use near_sdk::ext_contract;

#[ext_contract(ext_self)]
pub trait ExtSelf {
    fn on_execute(&mut self, prop_id: u32);
    fn on_ban_dismiss(&mut self, prop_id: u32);
}
