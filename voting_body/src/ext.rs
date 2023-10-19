use congress::HookError;
use near_sdk::ext_contract;
use near_sdk::AccountId;

#[ext_contract(ext_self)]
pub trait ExtSelf {
    fn on_execute(&mut self, prop_id: u32);
    fn on_ban_dismiss(&mut self, prop_id: u32);
}

#[ext_contract(ext_congress)]
pub trait ExtCongress {
    fn veto_hook(&mut self, id: u32) -> Result<(), HookError>;
    fn dissolve_hook(&mut self) -> Result<(), HookError>;
    fn dismiss_hook(&mut self, member: AccountId) -> Result<(), HookError>;
}
