use near_units::parse_near;
use serde_json::json;
use workspaces::{Account, Contract, DevNetwork, Worker};

use crate::util::{HouseType, TokenMetadata, SECOND, VOTE_COST};

mod util;

async fn init(
    worker: &Worker<impl DevNetwork>,
) -> anyhow::Result<(Contract, Account, Account, Account, u32)> {
    // deploy contracts
    let ndc_elections_contract = worker
        .dev_deploy(include_bytes!("../../res/ndc_elections.wasm"))
        .await?;

    let registry_contract = worker
        .dev_deploy(include_bytes!("../../res/registry.wasm"))
        .await?;

    let sbt_gd_issuer_acc = worker.dev_create_account().await?;
    let authority_acc = worker.dev_create_account().await?;
    let iah_issuer = worker.dev_create_account().await?;
    let alice_acc = worker.dev_create_account().await?;
    let bob_acc = worker.dev_create_account().await?;
    let john_acc = worker.dev_create_account().await?;

    // initialize contracts
    let res  = ndc_elections_contract
        .call("new")
        .args_json(json!({"authority": authority_acc.id(),"sbt_registry": registry_contract.id(),"iah_issuer": iah_issuer.id(),"iah_class_id": 1,}))
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
        .args_json(json!({"issuer": sbt_gd_issuer_acc.id()}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    // mint IAH sbt to alice
    let token_metadata = TokenMetadata {
        class: 1,
        issued_at: Some(0),
        expires_at: Some(100000),
        reference: None,
        reference_hash: None,
    };
    let token_spec = vec![(alice_acc.id(), vec![token_metadata])];

    let res = sbt_gd_issuer_acc
        .call(registry_contract.id(), "sbt_mint")
        .args_json(json!({ "token_spec": token_spec }))
        .deposit(parse_near!("1 N"))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    // get current block time
    let block_info = worker.view_block().await?;
    let current_timestamp = (block_info.timestamp() / SECOND) as u32;
    let start_time = current_timestamp + 10;

    // create a proposal
    let proposal_id: u32 = authority_acc
    .call(ndc_elections_contract.id(), "creat_proposal")
    .args_json(json!({"typ": HouseType::HouseOfMerit, "start": start_time, "end": u64::MAX, "ref_link": "test.io", "quorum": 10, "credits": 5, "candidates": [john_acc.id()],}))
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
    let vote = (john_acc.id(), 2);

    let res = alice_acc
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [vote],}))
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
    let vote = (john_acc.id(), 2);

    let res = bob_acc
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [vote],}))
        .deposit(VOTE_COST)
        .max_gas()
        .transact()
        .await?;
    // TODO: this one should fails once the check for a human is implemented.
    assert!(res.is_failure());
    Ok(())
}
