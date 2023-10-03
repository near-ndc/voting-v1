use congress::{PropKind, PropPerm, Vote};
use integrations::setup_registry;
use near_units::parse_near;
use serde_json::json;
use workspaces::{Account, Contract, DevNetwork, Worker};

/// 1ms in seconds
const MSECOND: u64 = 1_000_000;

pub struct InitStruct {
    pub congress_contract: Contract,
    pub alice: Account,
    pub bob: Account,
    pub john: Account,
    pub admin: Account,
    pub proposal_id: u32,
}

async fn init(worker: &Worker<impl DevNetwork>) -> anyhow::Result<InitStruct> {
    // deploy contracts
    let congress_contract = worker.dev_deploy(include_bytes!("../../res/congress.wasm"));
    let congress_contract = congress_contract.await?;

    let admin = worker.dev_create_account().await?;
    let community_fund = worker.dev_create_account().await?;
    let iah_issuer = worker.dev_create_account().await?;
    let alice = worker.dev_create_account().await?;
    let bob = worker.dev_create_account().await?;
    let john = worker.dev_create_account().await?;

    // let registry_contract = setup_registry(
    //     worker,
    //     admin.clone(),
    //     congress_contract.as_account().clone(),
    //     iah_issuer.clone(),
    //     vec![congress_contract.id().clone()],
    // )
    // .await?;

    // get current block time
    let block = worker.view_block().await?;
    let now = block.timestamp() / MSECOND; // timestamp in seconds
    let start_time = now + 20 * 1000;
    let end_time: u64 = now + 1000 * 1_000;
    let cooldown = now + 20 * 1000;
    let voting_duration = now + 40 * 1000;

    // initialize contracts
    let res1 = congress_contract
        .call("new")
        .args_json(json!({
            "community_fund": community_fund.id(),
            "start_time": start_time,
            "end_time": end_time,
            "cooldown": cooldown,
            "voting_duration": voting_duration,
            "members": vec![alice.id(), bob.id(), john.id()],
            "member_perms": vec![PropPerm::Text, PropPerm::FunctionCall, PropPerm::FundingRequest, PropPerm::RecurrentFundingRequest],
            "hook_auth": {},
            "budget_cap": parse_near!("1 N"),
            "big_funding_threshold": parse_near!("0.3 N"),
        }))
        .max_gas()
        .transact();

    assert!(res1.await?.is_success());

    // create a proposal
    let res2 = alice
        .call(congress_contract.id(), "create_proposal")
        .args_json(json!({
            "kind": PropKind::Text, "description": "Text proposal 1",
        }))
        .max_gas()
        .transact();
    let proposal_id: u32 = res2.await?.json()?;

    Ok(InitStruct {
        congress_contract: congress_contract.to_owned(),
        alice,
        bob,
        john,
        admin,
        proposal_id,
    })
}

#[tokio::test]
async fn vote_by_member() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let setup = init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(10).await?;

    let res = setup
        .alice
        .call(setup.congress_contract.id(), "vote")
        .args_json(json!({"prop_id": setup.proposal_id, "vote": Vote::Approve,}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res);

    Ok(())
}

#[tokio::test]
async fn vote_by_non_member() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let setup = init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(10).await?;

    let res = setup
        .alice
        .call(setup.congress_contract.id(), "vote")
        .args_json(json!({"prop_id": setup.proposal_id, "vote": Vote::Approve,}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_failure(), "{:?}", res);

    Ok(())
}
