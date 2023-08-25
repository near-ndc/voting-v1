use serde_json::json;
use workspaces::{Account, AccountId, Contract, DevNetwork, Worker};

pub async fn setup_registry(
    worker: &Worker<impl DevNetwork>,
    authority: Account,
    auth_flagger: Account,
    iah_issuer: Account,
    issuers: Vec<AccountId>,
) -> anyhow::Result<Contract> {
    let registry_contract = worker
        .dev_deploy(include_bytes!("../../res/registry.wasm"))
        .await?;

    let res = registry_contract
        .call("new")
        .args_json(json!({"authority": authority.id(),
          "iah_issuer": iah_issuer.id(), "iah_classes": [1],
          "authorized_flaggers": vec![auth_flagger.id()],
          "community_verified_set": vec![(iah_issuer.id(), vec![1])]
        }))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    // if any issuers passed add them to the registry
    for issuer in issuers {
        let res = authority
            .call(registry_contract.id(), "admin_add_sbt_issuer")
            .args_json(json!({ "issuer": issuer }))
            .max_gas()
            .transact()
            .await?;
        assert!(res.is_success());
    }

    Ok(registry_contract)
}
