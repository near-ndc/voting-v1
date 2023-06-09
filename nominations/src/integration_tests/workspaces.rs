use near_units::parse_near;
use serde_json::json;
use workspaces::{Account, Contract, DevNetwork, Worker};

use crate::{TokenMetadata, SECOND};

async fn init(
    worker: &Worker<impl DevNetwork>,
) -> anyhow::Result<(Contract, Account, Account, Account, u32)> {
    // deploy contracts
    let ndc_nominations_contract = worker
        .dev_deploy(include_bytes!("../../../res/ndc_nominations.wasm"))
        .await?;

    let registry_contract = worker
        .dev_deploy(include_bytes!("../../../res/registry.wasm"))
        .await?;

    let authority_acc = worker.dev_create_account().await?;
    let iah_issuer = worker.dev_create_account().await?;
    let alice_acc = worker.dev_create_account().await?;
    let bob_acc = worker.dev_create_account().await?;
    let john_acc = worker.dev_create_account().await?;

    // initialize contracts
    let res  = ndc_nominations_contract
        .call("new")
        .args_json(json!({"sbt_registry": registry_contract.id(),"iah_issuer": iah_issuer.id(),"iah_class_id": 1, "admins": [authority_acc.id()]}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    let res = registry_contract
        .call("new")
        .args_json(json!({"authority": authority_acc.id(),}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    // add sbt_gd_as_an_issuer
    let res = authority_acc
        .call(registry_contract.id(), "admin_add_sbt_issuer")
        .args_json(json!({"issuer": iah_issuer.id()}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    // get current block time
    let block_info = worker.view_block().await?;
    let current_timestamp = block_info.timestamp() / SECOND;
    let expires_at = current_timestamp + 100000000000;

    // mint IAH sbt to alice
    let token_metadata = TokenMetadata {
        class: 1,
        issued_at: Some(0),
        expires_at: Some(expires_at),
        reference: None,
        reference_hash: None,
    };

    let token_metadata2 = TokenMetadata {
        class: 1,
        issued_at: Some(0),
        expires_at: Some(current_timestamp),
        reference: None,
        reference_hash: None,
    };

    let token_spec = vec![
        (alice_acc.id(), vec![token_metadata]),
        (bob_acc.id(), vec![token_metadata2]),
    ];

    let res = iah_issuer
        .call(registry_contract.id(), "sbt_mint")
        .args_json(json!({ "token_spec": token_spec }))
        .deposit(parse_near!("1 N"))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    // create a proposal
    let campaign_id: u32 = authority_acc
        .call(ndc_nominations_contract.id(), "add_campaign")
        .args_json(
            json!({"name": "test_campaign", "link": "test.io", "start_time": 0, "end_time": u64::MAX,}),
        )
        .max_gas()
        .deposit(parse_near!("1 N"))
        .transact()
        .await?
        .json()?;

    return Ok((
        ndc_nominations_contract.to_owned(),
        alice_acc,
        bob_acc,
        john_acc,
        campaign_id,
    ));
}

#[tokio::test]
async fn human_nominates_human() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, alice_acc, bob_acc, _, campaign_id) = init(&worker).await?;

    // assert the nominations per bob == 0
    let res: u64 = alice_acc
        .call(ndc_elections_contract.id(), "nominations_per_user")
        .args_json(json!({"campaign": campaign_id, "nominee": bob_acc.id()}))
        .max_gas()
        .transact()
        .await?
        .json()?;
    assert_eq!(res, 0);

    // fast forward to the campaign period
    worker.fast_forward(100).await?;
    // nominate
    let res:String  = alice_acc
        .call(ndc_elections_contract.id(), "nominate")
        .args_json(json!({"campaign": campaign_id, "nominee": bob_acc.id(),"comment": "test", "external_resource": "test"}))
        .max_gas()
        .transact()
        .await?
        .json()?;
    print!("{}", res);
    // assert!(res.is_success());

    // make sure the nomination for bob has been registered
    let res: u64 = alice_acc
        .call(ndc_elections_contract.id(), "nominations_per_user")
        .args_json(json!({"campaign": campaign_id, "nominee": bob_acc.id()}))
        .max_gas()
        .transact()
        .await?
        .json()?;
    assert_eq!(res, 1);

    println!("Passed ✅ human_nominates_human");
    Ok(())
}

#[tokio::test]
async fn non_human_nominates_human() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, _, bob_acc, john_acc, campaign_id) = init(&worker).await?;

    // fast forward to the campaign period
    worker.fast_forward(100).await?;
    // nominate
    let res = john_acc
        .call(ndc_elections_contract.id(), "nominate")
        .args_json(json!({"campaign": campaign_id, "nominee": bob_acc.id(),"comment": "test", "external_resource": "test"}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_failure());
    Ok(())
}

#[tokio::test]
async fn non_human_nominates_human_expired_token() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, _, bob_acc, _, campaign_id) = init(&worker).await?;

    // fast forward to the campaign period
    worker.fast_forward(100).await?;
    // nominate
    let res = bob_acc
        .call(ndc_elections_contract.id(), "nominate")
        .args_json(json!({"campaign": campaign_id, "nominee": bob_acc.id(),"comment": "test", "external_resource": "test"}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_failure());
    Ok(())
}
