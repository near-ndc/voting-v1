use integration_test_common::setup_registry;
use near_units::parse_near;
use serde_json::json;
use workspaces::{Account, Contract, DevNetwork, Worker};

/// 1ms in nano seconds
//extern crate elections;
use elections::{
    proposal::{HouseType, VOTE_COST},
    ProposalView, TokenMetadata,
};

/// 1ms in seconds
const MSECOND: u64 = 1_000_000;

async fn init(
    worker: &Worker<impl DevNetwork>,
) -> anyhow::Result<(Contract, Account, Account, Account, u32)> {
    // deploy contracts
    let ndc_elections_contract = worker.dev_deploy(include_bytes!("../../res/elections.wasm"));
    let ndc_elections_contract = ndc_elections_contract.await?;

    let authority_acc = worker.dev_create_account().await?;
    let iah_issuer = worker.dev_create_account().await?;
    let alice_acc = worker.dev_create_account().await?;
    let bob_acc = worker.dev_create_account().await?;
    let john_acc = worker.dev_create_account().await?;

    let registry_contract =
        setup_registry(worker, authority_acc.clone(), iah_issuer.clone()).await?;

    // initialize contracts
    let res1 = ndc_elections_contract
        .call("new")
        .args_json(json!({
            "authority": authority_acc.id(),
            "sbt_registry": registry_contract.id(),
        }))
        .max_gas()
        .transact();

    assert!(res1.await?.is_success());

    // get current block time
    let block = worker.view_block().await?;
    let now = block.timestamp() / MSECOND; // timestamp in seconds
    let start_time = now + 10 * 1000; // below we are executing 2 transactions, first has 3 receipts, so the proposal is roughtly now + 10seconds
    let expires_at: u64 = now + 100 * 1_000;

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
        expires_at: Some(now),
        reference: None,
        reference_hash: None,
    };
    let token_spec = vec![
        (alice_acc.id(), vec![token_metadata]),
        (john_acc.id(), vec![token_metadata_short_expire_at]),
    ];

    let res1 = iah_issuer
        .call(registry_contract.id(), "sbt_mint")
        .args_json(json!({ "token_spec": token_spec }))
        .deposit(parse_near!("1 N"))
        .max_gas()
        .transact()
        .await?;

    // create a proposal
    let res2 = authority_acc
    .call(ndc_elections_contract.id(), "create_proposal")
    .args_json(json!({"typ": HouseType::HouseOfMerit, "start": start_time, "end": u64::MAX, "ref_link": "test.io", "quorum": 10, "credits": 5, "seats": 1, "candidates": [john_acc.id(), alice_acc.id()],}))
    .max_gas()
    .transact();

    assert!(res1.is_success(), "{:?}", res1);
    let proposal_id: u32 = res2.await?.json()?;

    Ok((
        ndc_elections_contract.to_owned(),
        alice_acc,
        bob_acc,
        john_acc,
        proposal_id,
    ))
}

#[tokio::test]
async fn vote_by_human() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, alice_acc, _, john_acc, proposal_id) = init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(10).await?;

    let res = alice_acc
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [john_acc.id()],}))
        .deposit(VOTE_COST)
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res);

    Ok(())
}

#[tokio::test]
async fn vote_by_non_human() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, _, bob_acc, john_acc, proposal_id) = init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(12).await?;

    let res = bob_acc
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [john_acc.id()],}))
        .deposit(VOTE_COST)
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_failure(), "resp should be a failure {:?}", res);
    let res_str = format!("{:?}", res);
    assert!(
        res_str.contains("voter is not a verified human"),
        "{}",
        res_str
    );

    Ok(())
}

#[tokio::test]
async fn vote_expired_iah_token() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, alice_acc, _, john_acc, proposal_id) = init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(100).await?;

    let res = john_acc
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [alice_acc.id()],}))
        .deposit(VOTE_COST)
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_failure(), "resp should be a failure {:?}", res);
    let res_str = format!("{:?}", res);
    assert!(
        res_str.contains("voter is not a verified human"),
        "{}",
        res_str
    );

    Ok(())
}

#[tokio::test]
async fn state_change() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, alice_acc, _, john_acc, proposal_id) = init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(10).await?;

    let proposal = alice_acc
        .call(ndc_elections_contract.id(), "proposal")
        .args_json(json!({ "prop_id": proposal_id }))
        .view()
        .await?
        .json::<ProposalView>()?;
    assert_eq!(proposal.voters_num, 0);

    let res = alice_acc
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [john_acc.id()],}))
        .deposit(VOTE_COST)
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res);

    let proposal = alice_acc
        .call(ndc_elections_contract.id(), "proposal")
        .args_json(json!({ "prop_id": proposal_id }))
        .view()
        .await?
        .json::<ProposalView>()?;
    assert_eq!(proposal.voters_num, 1);
    assert_eq!(proposal.result[0].1, 0); // votes for alice
    assert_eq!(proposal.result[1].1, 1); // votes for john

    Ok(())
}
