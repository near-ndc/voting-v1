use std::collections::HashMap;

use congress::{PropKind, PropPerm, Vote, HookPerm};
use integrations::setup_registry;
use near_units::parse_near;
use serde_json::json;
use workspaces::{Account, Contract, DevNetwork, Worker, AccountId};

/// 1ms in seconds
const MSECOND: u64 = 1_000_000;

pub struct InitStruct {
    pub hom_contract: Contract,
    pub coa_contract: Contract,
    pub tc_contract: Contract,
    pub alice: Account,
    pub bob: Account,
    pub john: Account,
    pub admin: Account,
    pub proposal_id: u32,
}

async fn instantiate_congress(
    congress_contract: Contract, now: u64,
    members: Vec<&AccountId>, member_perms: Vec<PropPerm>,
    hook_auth: HashMap<AccountId, Vec<HookPerm>>, community_fund: Account
) -> anyhow::Result<Contract> {
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
        "members": members,
        "member_perms": member_perms,
        "hook_auth": hook_auth,
        "budget_cap": parse_near!("1 N").to_string(),
        "big_funding_threshold": parse_near!("0.3 N").to_string(),
    }))
    .max_gas()
    .transact();

    assert!(res1.await?.is_success());

    Ok(congress_contract)
}

async fn init(worker: &Worker<impl DevNetwork>) -> anyhow::Result<InitStruct> {
    // deploy contracts
    let mut hom_contract = worker.dev_deploy(include_bytes!("../../res/congress.wasm")).await?;
    let mut coa_contract = worker.dev_deploy(include_bytes!("../../res/congress.wasm")).await?;
    let mut tc_contract = worker.dev_deploy(include_bytes!("../../res/congress.wasm")).await?;

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

    // initialize HoM
    hom_contract = instantiate_congress(
        hom_contract,
        now,
        vec![alice.id(), bob.id(), john.id()],
        vec![PropPerm::Text, PropPerm::FunctionCall, PropPerm::FundingRequest, PropPerm::RecurrentFundingRequest],
        HashMap::new(), community_fund.clone()).await?;

    // initialize CoA
    coa_contract = instantiate_congress(
        coa_contract,
        now,
        vec![alice.id(), bob.id(), john.id()],
        vec![PropPerm::Text, PropPerm::FunctionCall, PropPerm::FundingRequest, PropPerm::RecurrentFundingRequest],
        HashMap::new(), community_fund.clone()).await?;

    // initialize TC
    tc_contract = instantiate_congress(
        tc_contract,
        now,
        vec![alice.id(), bob.id(), john.id()],
        vec![PropPerm::Text, PropPerm::FunctionCall, PropPerm::FundingRequest, PropPerm::RecurrentFundingRequest],
        HashMap::new(), community_fund).await?;

    // create a proposal
    let res2 = alice
        .call(hom_contract.id(), "create_proposal")
        .args_json(json!({
            "kind": PropKind::Text, "description": "Text proposal 1",
        }))
        .max_gas()
        .deposit(parse_near!("0.01 N"))
        .transact();
    let proposal_id: u32 = res2.await?.json()?;

    Ok(InitStruct {
        hom_contract: hom_contract.to_owned(),
        coa_contract: coa_contract.to_owned(),
        tc_contract: tc_contract.to_owned(),
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
        .call(setup.hom_contract.id(), "vote")
        .args_json(json!({"id": setup.proposal_id, "vote": Vote::Approve,}))
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
        .admin
        .call(setup.hom_contract.id(), "vote")
        .args_json(json!({"id": setup.proposal_id, "vote": Vote::Approve,}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_failure(), "{:?}", res);

    Ok(())
}
