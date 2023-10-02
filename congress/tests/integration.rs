use integrations::setup_registry;
use near_units::parse_near;
use serde_json::json;
use workspaces::{Account, AccountId, Contract, DevNetwork, Worker};

/// 1ms in seconds
const MSECOND: u64 = 1_000_000;

pub struct InitStruct {
    pub ndc_elections_contract: Contract,
    pub registry_contract: Contract,
    pub alice: Account,
    pub bob: Account,
    pub john: Account,
    pub auth_flagger: Account,
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

    let registry_contract = setup_registry(
        worker,
        admin.clone(),
        congress_contract.as_account().clone(),
        iah_issuer.clone(),
        vec![congress_contract.id().clone()],
    )
    .await?;

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
            "member_perms": vec![],
            "hook_auth": ,
            "budget_cap": ,
            "big_funding_threshold": ,
        }))
        .max_gas()
        .transact();

    assert!(res1.await?.is_success());

    // mint IAH sbt to alice and john
    let token_metadata = TokenMetadata {
        class: 1,
        issued_at: Some(0),
        expires_at: Some(proposal_expires_at * 20),
        reference: None,
        reference_hash: None,
    };

    let token_metadata_short_expire_at = TokenMetadata {
        class: 1,
        issued_at: Some(0),
        expires_at: Some(now + 9000),
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
            "end": proposal_expires_at, "cooldown": 1, "ref_link": "test.io", "quorum": 10,
            "credits": 5, "seats": 1, "candidates": [john.id(), alice.id()],
            "min_candidate_support": 2,
        }))
        .max_gas()
        .transact();

    accept_policy_and_bond(
        registry_contract.clone(),
        ndc_elections_contract.clone(),
        john.clone(),
        policy1(),
    )
    .await?;
    accept_policy_and_bond(
        registry_contract.clone(),
        ndc_elections_contract.clone(),
        alice.clone(),
        policy1(),
    )
    .await?;

    let res3 = auth_flagger
        .call(registry_contract.id(), "admin_flag_accounts")
        .args_json(
            json!({ "flag": "Verified", "accounts": [john.id(), alice.id(), bob.id()], "memo": ""}),
        )
        .max_gas()
        .transact()
        .await?;
    assert!(res3.is_success(), "{:?}", res3);

    assert!(res1.is_success(), "{:?}", res1);
    let proposal_id: u32 = res2.await?.json()?;

    Ok(InitStruct {
        ndc_elections_contract: ndc_elections_contract.to_owned(),
        registry_contract: registry_contract.to_owned(),
        alice,
        bob,
        john,
        auth_flagger,
        admin,
        proposal_id,
    })
}