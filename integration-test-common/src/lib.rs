use workspaces::{Account, Contract, DevNetwork, Worker};
use serde_json::json;

pub async fn setup_registry(worker: &Worker<impl DevNetwork>, authority_acc: Account, iah_issuer: Account) 
-> anyhow::Result<Contract> {
    let registry_contract = worker
    .dev_deploy(include_bytes!("../../res/registry.wasm"))
    .await?;

    let res = registry_contract
    .call("new")
    .args_json(json!({"authority": authority_acc.id(),"iah_issuer": iah_issuer.id(), "iah_classes": [1],}))
    .max_gas()
    .transact()
    .await?;
    assert!(res.is_success());

    return Ok(registry_contract);
}