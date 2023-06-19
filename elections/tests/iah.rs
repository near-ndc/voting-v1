use near_units::parse_near;
use serde_json::json;
use workspaces::{Account, Contract, DevNetwork, Worker};

//extern crate elections;
use elections::{
    proposal::{HouseType, VOTE_COST},
    TokenMetadata, MILI_SECOND,
};

async fn init(
    worker: &Worker<impl DevNetwork>,
) -> anyhow::Result<(Contract, Account, Account, Account, u32)> {
    // deploy contracts
    let ndc_elections_contract = worker
        .dev_deploy(include_bytes!("../../res/elections.wasm"))
        .await?;

    let registry_contract = worker
        // registry is a contract form https://github.com/near-ndc/i-am-human
        .dev_deploy(include_bytes!("../../res/registry.wasm"))
        .await?;

    let authority_acc = worker.dev_create_account().await?;
    let iah_issuer = worker.dev_create_account().await?;
    let alice_acc = worker.dev_create_account().await?;
    let bob_acc = worker.dev_create_account().await?;
    let john_acc = worker.dev_create_account().await?;

    // initialize contracts
    let res = ndc_elections_contract
        .call("new")
        .args_json(json!({
            "authority": authority_acc.id(),
            "sbt_registry": registry_contract.id(),
            "iah_issuer": iah_issuer.id(),
            "iah_class_id": 1,
        }))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    let res = registry_contract
        .call("new")
        .args_json(json!({
            "authority": authority_acc.id(),
            "iah_issuer": iah_issuer.id(),
            "iah_classes": (1,),
        }))
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
    let current_timestamp = block_info.timestamp() / MILI_SECOND; // timestamp in milliseconds
    let start_time_ms = current_timestamp + 1_000 * 10; // 10 seconds in milliseconds
    let expires_at: u64 = current_timestamp + 1_000 * 1000; // 1000 seconds in milliseconds
    let start_time = start_time_ms / 1000; // 10 seconds

    // mint IAH sbt to alice and john
    let token_metadata = TokenMetadata {
        class: 1,
        issued_at: Some(0),
        expires_at: Some(expires_at),
        reference: None,
        reference_hash: None,
    };

    let token_metadata_short_expire_at = TokenMetadata {
        class: 1,
        issued_at: Some(0),
        expires_at: Some(current_timestamp),
        reference: None,
        reference_hash: None,
    };
    let token_spec = vec![
        (alice_acc.id(), vec![token_metadata]),
        (john_acc.id(), vec![token_metadata_short_expire_at]),
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
    let proposal_id: u32 = authority_acc
    .call(ndc_elections_contract.id(), "create_proposal")
    .args_json(json!({"typ": HouseType::HouseOfMerit, "start": start_time, "end": u64::MAX, "ref_link": "test.io", "quorum": 10, "credits": 5, "seats": 1, "candidates": [john_acc.id(), alice_acc.id()],}))
    .max_gas()
    .transact()
    .await?
    .json()?;

    return Ok((
        ndc_elections_contract.to_owned(),
        alice_acc,
        bob_acc,
        john_acc,
        proposal_id,
    ));
}

#[tokio::test]
async fn vote_by_human() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, alice_acc, _, john_acc, proposal_id) = init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(100).await?;
    // create a vote
    let res = alice_acc
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [john_acc.id()],}))
        .deposit(VOTE_COST)
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    Ok(())
}

#[tokio::test]
async fn vote_by_non_human() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, _, bob_acc, john_acc, proposal_id) = init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(100).await?;

    // create a vote
    let res = bob_acc
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [john_acc.id()],}))
        .deposit(VOTE_COST)
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_failure());

    Ok(())
}

#[tokio::test]
async fn vote_expired_iah_token() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, alice_acc, _, john_acc, proposal_id) = init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(100).await?;

    // create a vote
    let res = john_acc
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [alice_acc.id()],}))
        .deposit(VOTE_COST)
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_failure());

    Ok(())
}
