use serde_json::json;
use workspaces::{Account, AccountId, Contract, DevNetwork, Worker};

pub async fn setup_registry(
    worker: &Worker<impl DevNetwork>,
    authority_acc: Account,
    iah_issuer: Account,
    issuers: Option<Vec<AccountId>>,
) -> anyhow::Result<Contract> {
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

    // if any issuers passed add them to the registry
    if issuers.is_some() {
        for issuer in issuers.unwrap() {
            let res = authority_acc
                .call(registry_contract.id(), "admin_add_sbt_issuer")
                .args_json(json!({ "issuer": issuer }))
                .max_gas()
                .transact()
                .await?;
            assert!(res.is_success());
        }
    }

    return Ok(registry_contract);
}
