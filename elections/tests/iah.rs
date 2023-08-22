use integrations::setup_registry;
use near_units::parse_near;
use serde_json::json;
use workspaces::{Account, Contract, DevNetwork, Worker};

/// 1ms in nano seconds
//extern crate elections;
use elections::{
    proposal::{ProposalType, VOTE_COST},
    ProposalView, TokenMetadata, BOND_AMOUNT,
};

/// 1ms in seconds
const MSECOND: u64 = 1_000_000;

async fn init(
    worker: &Worker<impl DevNetwork>,
) -> anyhow::Result<(Contract, Account, Account, Account, u32)> {
    // deploy contracts
    let ndc_elections_contract = worker.dev_deploy(include_bytes!("../../res/elections.wasm"));
    let ndc_elections_contract = ndc_elections_contract.await?;

    let admin = worker.dev_create_account().await?;
    let auth_flagger = worker.dev_create_account().await?;
    let iah_issuer = worker.dev_create_account().await?;
    let alice = worker.dev_create_account().await?;
    let bob = worker.dev_create_account().await?;
    let john = worker.dev_create_account().await?;

    let registry_contract = setup_registry(
        worker,
        admin.clone(),
        auth_flagger,
        iah_issuer.clone(),
        None,
    )
    .await?;

    // initialize contracts
    let res1 = ndc_elections_contract
        .call("new")
        .args_json(json!({
            "authority": admin.id(),
            "sbt_registry": registry_contract.id(),
            "policy": policy1(),
        }))
        .max_gas()
        .transact();

    assert!(res1.await?.is_success());

    // get current block time
    let block = worker.view_block().await?;
    let now = block.timestamp() / MSECOND; // timestamp in seconds
    let start_time = now + 20 * 1000; // below we are executing 5 transactions, first has 3 receipts, so the proposal is roughtly now + 20seconds
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
        expires_at: Some(block.timestamp() + 5000),
        reference: None,
        reference_hash: None,
    };

     // mint IAH sbt to bob
     let token_metadata_bob = TokenMetadata {
        class: 1,
        issued_at: Some(0),
        expires_at: Some(expires_at),
        reference: None,
        reference_hash: None,
    };

    let token_spec = vec![
        (alice.id(), vec![token_metadata]),
        (bob.id(), vec![token_metadata_bob]),
        (john.id(), vec![token_metadata_short_expire_at]),
    ];

    let res1 = iah_issuer
        .call(registry_contract.id(), "sbt_mint")
        .args_json(json!({ "token_spec": token_spec }))
        .deposit(parse_near!("1 N"))
        .max_gas()
        .transact()
        .await?;

    // create a proposal
    let res2 = admin
        .call(ndc_elections_contract.id(), "create_proposal")
        .args_json(json!({
            "typ": ProposalType::HouseOfMerit, "start": start_time,
            "end": u64::MAX, "cooldown": 604800000, "ref_link": "test.io", "quorum": 10,
            "credits": 5, "seats": 1, "candidates": [john.id(), alice.id()],
            "min_candidate_support": 2,
        }))
        .max_gas()
        .transact();

    accept_policy(ndc_elections_contract.clone(), john.clone(), policy1()).await?;
    accept_policy(ndc_elections_contract.clone(), alice.clone(), policy1()).await?;
    accept_policy(ndc_elections_contract.clone(), bob.clone(), policy1()).await?;

    assert!(res1.is_success(), "{:?}", res1);
    let proposal_id: u32 = res2.await?.json()?;

    Ok((
        ndc_elections_contract.to_owned(),
        alice,
        bob,
        john,
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

#[ignore]
#[tokio::test]
async fn vote_by_non_human() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, _, bob_acc, john_acc, proposal_id) = init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(60).await?;

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

#[ignore]
#[tokio::test]
async fn vote_expired_iah_token() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, alice_acc, _, john_acc, proposal_id) = init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(150).await?;

    let res = john_acc
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [alice_acc.id()],}))
        .deposit(VOTE_COST)
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_failure(), "resp should be a failure {:?}", res);
    let failures = format!("{:?}", res.receipt_failures());
    assert!(
        failures.contains("voter is not a verified human"),
        "{}",
        failures
    );

    Ok(())
}

#[tokio::test]
async fn vote_without_accepting_policy() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, _, _, john_acc, proposal_id) = init(&worker).await?;
    let zen_acc = worker.dev_create_account().await?;
    // fast forward to the voting period
    worker.fast_forward(10).await?;

    let res = zen_acc
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [john_acc.id()],}))
        .deposit(VOTE_COST)
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_failure(), "resp should be a failure {:?}", res);
    let failures = format!("{:?}", res.receipt_failures());
    assert!(
        failures.contains("user didn't accept the voting policy, or the accepted voting policy doesn't match the required one"),
        "{}",
        failures
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

async fn accept_policy(election: Contract, user: Account, policy: String) -> anyhow::Result<()> {
    let call_from = user.clone();
    let res = call_from
        .call(election.id(), "accept_fair_voting_policy")
        .args_json(json!({
            "policy": policy,
        }))
        .deposit(BOND_AMOUNT)
        .max_gas()
        .transact()
        .await?;

    assert!(res.is_success(), "{:?}", res);
    Ok(())
}

fn policy1() -> String {
    "f1c09f8686fe7d0d798517111a66675da0012d8ad1693a47e0e2a7d3ae1c69d4".to_owned()
}
