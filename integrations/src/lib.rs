use std::collections::HashMap;

use congress::{HookPerm, PropPerm};
use near_units::parse_near;
use near_workspaces::{Account, AccountId, Contract, DevNetwork, Worker};
use serde_json::json;

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

pub async fn instantiate_congress(
    congress_contract: Contract,
    now: u64,
    members: Vec<&AccountId>,
    member_perms: Vec<PropPerm>,
    hook_auth: HashMap<AccountId, Vec<HookPerm>>,
    community_fund: Account,
    registry: &AccountId,
    cooldown: u64,
) -> anyhow::Result<Contract> {
    let start_time = now + 20 * 1000;
    let end_time: u64 = now + 100 * 1000;
    let vote_duration = 20 * 1000;
    let min_vote_duration = 0;
    // initialize contract
    let res = congress_contract
        .call("new")
        .args_json(json!({
            "community_fund": community_fund.id(),
            "start_time": start_time,
            "end_time": end_time,
            "cooldown": cooldown,
            "vote_duration": vote_duration,
            "min_vote_duration": min_vote_duration,
            "members": members,
            "member_perms": member_perms,
            "hook_auth": hook_auth,
            "budget_cap": parse_near!("1 N").to_string(),
            "big_funding_threshold": parse_near!("0.3 N").to_string(),
            "registry": registry
        }))
        .max_gas()
        .transact()
        .await?;

    assert!(res.is_success(), "{:?}", res);

    Ok(congress_contract)
}
