use std::collections::HashMap;

use congress::view::{MembersOutput, ProposalOutput};
use congress::{ActionCall, HookPerm, PropKind, PropPerm, ProposalStatus, Vote};

use integrations::{instantiate_congress, setup_registry};
use near_sdk::base64::{decode, encode};
use near_sdk::json_types::{U128, U64};
use near_sdk::serde::Deserialize;
use near_sdk::AccountId as NearAccountId;
use near_units::parse_near;
use near_workspaces::{Account, AccountId, Contract, DevNetwork, Worker};
use serde_json::json;

/// 1s in ms
const MSECOND: u64 = 1_000_000;

#[derive(Deserialize, PartialEq, Debug)]
#[serde(crate = "near_sdk::serde")]
pub enum AccountFlag {
    /// Account is "blacklisted" when it was marked as a scam or suspectible to be a mnipulated account or not a human.
    Blacklisted,
    /// Manually verified account.
    Verified,
    /// Account misbehaved and should be refused to have a significant governance role. However
    /// it will be able to vote as a Voting Body member.
    GovBan,
}

pub struct InitStruct {
    pub hom_contract: Contract,
    pub coa_contract: Contract,
    pub tc_contract: Contract,
    pub registry_contract: Contract,
    pub alice: Account,
    pub bob: Account,
    pub john: Account,
    pub admin: Account,
    pub proposal_id: u32,
}

async fn vote(users: Vec<Account>, dao: &Contract, proposal_id: u32) -> anyhow::Result<()> {
    for user in users.into_iter() {
        let res = user
            .call(dao.id(), "vote")
            .args_json(json!({"id": proposal_id, "vote": Vote::Approve,}))
            .max_gas()
            .transact()
            .await?;
        assert!(res.is_success(), "{:?}", res);
    }
    Ok(())
}

async fn init(worker: &Worker<impl DevNetwork>) -> anyhow::Result<InitStruct> {
    // deploy contracts
    let mut hom_contract = worker
        .dev_deploy(include_bytes!("../../res/congress.wasm"))
        .await?;
    let mut coa_contract = worker
        .dev_deploy(include_bytes!("../../res/congress.wasm"))
        .await?;
    let mut tc_contract = worker
        .dev_deploy(include_bytes!("../../res/congress.wasm"))
        .await?;

    let admin = worker.dev_create_account().await?;
    let community_fund = worker.dev_create_account().await?;
    let iah_issuer = worker.dev_create_account().await?;
    let alice = worker.dev_create_account().await?;
    let bob = worker.dev_create_account().await?;
    let john = worker.dev_create_account().await?;

    let registry_contract = setup_registry(
        worker,
        admin.clone(),
        tc_contract.as_account().clone(),
        iah_issuer.clone(),
        vec![tc_contract.id().clone()],
    )
    .await?;

    // get current block time
    let block = worker.view_block().await?;
    let now = block.timestamp() / MSECOND; // timestamp in milliseconds

    // initialize TC
    tc_contract = instantiate_congress(
        tc_contract,
        now,
        vec![alice.id(), bob.id(), john.id()],
        vec![
            PropPerm::Text,
            PropPerm::FunctionCall,
            PropPerm::DismissAndBan,
        ],
        HashMap::new(),
        community_fund.clone(),
        registry_contract.id(),
        0,
    )
    .await?;

    let mut coa_hook = HashMap::new();
    coa_hook.insert(
        tc_contract.id().clone(),
        vec![HookPerm::Dismiss, HookPerm::Dissolve],
    );
    // initialize CoA
    coa_contract = instantiate_congress(
        coa_contract,
        now,
        vec![alice.id(), bob.id(), john.id()],
        vec![PropPerm::Text, PropPerm::FunctionCall],
        coa_hook,
        community_fund.clone(),
        registry_contract.id(),
        0,
    )
    .await?;

    let mut hom_hook = HashMap::new();
    hom_hook.insert(
        tc_contract.id().clone(),
        vec![HookPerm::Dismiss, HookPerm::Dissolve],
    );
    hom_hook.insert(coa_contract.id().clone(), vec![HookPerm::VetoAll]);
    // initialize HoM
    hom_contract = instantiate_congress(
        hom_contract,
        now,
        vec![alice.id(), bob.id(), john.id()],
        vec![
            PropPerm::Text,
            PropPerm::FunctionCall,
            PropPerm::FundingRequest,
            PropPerm::RecurrentFundingRequest,
        ],
        hom_hook,
        community_fund.clone(),
        registry_contract.id(),
        10 * 1000,
    )
    .await?;

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
        registry_contract,
    })
}

#[tokio::test]
async fn full_prop_flow() -> anyhow::Result<()> {
    let worker = near_workspaces::sandbox().await?;
    let setup = init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(10).await?;

    vote(
        vec![setup.alice, setup.john],
        &setup.hom_contract,
        setup.proposal_id,
    )
    .await?;

    // fast forward to after cooldown
    worker.fast_forward(100).await?;

    let res = setup
        .bob
        .call(setup.hom_contract.id(), "execute")
        .args_json(json!({"id": setup.proposal_id,}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res);

    worker.fast_forward(100).await?;
    // fast forward after end time is over
    let res = setup
        .bob
        .call(setup.hom_contract.id(), "execute")
        .args_json(json!({"id": setup.proposal_id,}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_failure(), "{:?}", res);

    Ok(())
}

#[tokio::test]
async fn vote_by_non_member() -> anyhow::Result<()> {
    let worker = near_workspaces::sandbox().await?;
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

// Interhouse

#[tokio::test]
async fn tc_dismiss_coa() -> anyhow::Result<()> {
    let worker = near_workspaces::sandbox().await?;
    let setup = init(&worker).await?;

    let encoded = encode(json!({"member": setup.alice.id()}).to_string());

    let res2 = setup
        .alice
        .call(setup.tc_contract.id(), "create_proposal")
        .args_json(json!({
            "kind": PropKind::FunctionCall { receiver_id: to_near_account(setup.coa_contract.id()), actions: [ActionCall {
                method_name: "dismiss_hook".to_string(),
                args: decode(encoded).unwrap().into(),
                deposit: U128(0),
                gas: U64(10_000_000_000_000),
            }].to_vec() }, "description": "Veto proposal 1",
        }))
        .max_gas()
        .deposit(parse_near!("0.01 N"))
        .transact();
    let proposal_id: u32 = res2.await?.json()?;

    vote(
        vec![setup.john.clone(), setup.bob.clone()],
        &setup.tc_contract,
        proposal_id,
    )
    .await?;

    // after removal less members
    let members = setup
        .alice
        .call(setup.coa_contract.id(), "get_members")
        .view()
        .await?
        .json::<MembersOutput>()?;

    let mut expected = vec![
        to_near_account(setup.bob.id()),
        to_near_account(setup.john.id()),
    ];
    expected.sort();
    assert_eq!(members.members, expected);

    Ok(())
}

#[tokio::test]
async fn coa_veto_hom() -> anyhow::Result<()> {
    let worker = near_workspaces::sandbox().await?;
    let setup = init(&worker).await?;

    let encoded = encode(json!({"id": setup.proposal_id}).to_string());

    let res2 = setup
        .alice
        .call(setup.coa_contract.id(), "create_proposal")
        .args_json(json!({
            "kind": PropKind::FunctionCall { receiver_id: to_near_account(setup.hom_contract.id()), actions: [ActionCall {
                method_name: "veto_hook".to_string(),
                args: decode(encoded).unwrap().into(),
                deposit: U128(0),
                gas: U64(10_000_000_000_000),
            }].to_vec() }, "description": "Veto proposal 1",
        }))
        .max_gas()
        .deposit(parse_near!("0.01 N"))
        .transact();
    let proposal_id: u32 = res2.await?.json()?;

    vote(
        vec![setup.john.clone(), setup.bob.clone()],
        &setup.coa_contract,
        proposal_id,
    )
    .await?;

    // after execution proposal should be in Vetoed
    let members = setup
        .alice
        .call(setup.hom_contract.id(), "get_proposal")
        .args_json(json!({"id": setup.proposal_id}))
        .view()
        .await?
        .json::<Option<ProposalOutput>>()?;
    assert_eq!(members.unwrap().proposal.status, ProposalStatus::Vetoed);

    Ok(())
}

#[tokio::test]
async fn tc_ban_and_dismiss() -> anyhow::Result<()> {
    let worker = near_workspaces::sandbox().await?;
    let setup = init(&worker).await?;

    let res2 = setup
        .bob
        .call(setup.tc_contract.id(), "create_proposal")
        .args_json(json!({
            "kind": PropKind::DismissAndBan { member: to_near_account(setup.alice.id()), house:  to_near_account(setup.coa_contract.id())
            },
            "description": "Dismiss and ban alice".to_string()
        }))
        .max_gas()
        .deposit(parse_near!("0.01 N"))
        .transact();
    let proposal_id: u32 = res2.await?.json()?;

    let res = setup
        .bob
        .call(setup.tc_contract.id(), "vote")
        .args_json(json!({"id": proposal_id, "vote": Vote::Approve,}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res);

    let res = setup
        .john
        .call(setup.tc_contract.id(), "vote")
        .args_json(json!({"id": proposal_id, "vote": Vote::Approve,}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res);

    // after removal less members
    let members = setup
        .alice
        .call(setup.coa_contract.id(), "get_members")
        .view()
        .await?
        .json::<MembersOutput>()?;

    let mut expected = vec![
        to_near_account(setup.bob.id()),
        to_near_account(setup.john.id()),
    ];
    expected.sort();
    assert_eq!(members.members, expected);

    // verify
    // admin flag
    let res = setup
        .alice
        .call(setup.registry_contract.id(), "account_flagged")
        .args_json(json!({"account": to_near_account(setup.alice.id())}))
        .view()
        .await?
        .json::<Option<AccountFlag>>()?;

    assert_eq!(res, Some(AccountFlag::GovBan));

    Ok(())
}

#[tokio::test]
async fn tc_ban_and_dismiss_fail_cases() -> anyhow::Result<()> {
    let worker = near_workspaces::sandbox().await?;
    let setup = init(&worker).await?;

    let res2 = setup
        .bob
        .call(setup.tc_contract.id(), "create_proposal")
        .args_json(json!({
            "kind": PropKind::DismissAndBan { member: to_near_account(setup.alice.id()), house:  to_near_account(setup.coa_contract.id())
            },
            "description": "Dismiss and ban alice".to_string()
        }))
        .max_gas()
        .deposit(parse_near!("0.01 N"))
        .transact();
    let proposal_id: u32 = res2.await?.json()?;

    // remove tc as flagger
    let res = setup
        .admin
        .call(
            setup.registry_contract.id(),
            "admin_set_authorized_flaggers",
        )
        .args_json(json!({
        "authorized_flaggers": [],
        }))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    vote(
        vec![setup.john.clone(), setup.bob.clone()],
        &setup.tc_contract,
        proposal_id,
    )
    .await?;

    // after removal less members
    let members = setup
        .alice
        .call(setup.coa_contract.id(), "get_members")
        .view()
        .await?
        .json::<MembersOutput>()?;

    let mut expected = vec![
        to_near_account(setup.bob.id()),
        to_near_account(setup.john.id()),
    ];
    expected.sort();
    assert_eq!(members.members, expected);

    // verify
    // admin flag
    let res = setup
        .alice
        .call(setup.registry_contract.id(), "account_flagged")
        .args_json(json!({"account": to_near_account(setup.alice.id())}))
        .view()
        .await?
        .json::<Option<AccountFlag>>()?;

    assert_eq!(res, None);

    let proposal = setup
        .alice
        .call(setup.tc_contract.id(), "get_proposal")
        .args_json(json!({ "id": proposal_id }))
        .view()
        .await?
        .json::<Option<ProposalOutput>>()?;
    assert_eq!(proposal.unwrap().proposal.status, ProposalStatus::Failed);

    // execute after adding flagger again
    // remove tc as flagger
    let res = setup
        .admin
        .call(
            setup.registry_contract.id(),
            "admin_set_authorized_flaggers",
        )
        .args_json(json!({
        "authorized_flaggers": [setup.tc_contract.id()],
        }))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    let res = setup
        .bob
        .call(setup.tc_contract.id(), "execute")
        .args_json(json!({"id": proposal_id,}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res);

    // no dismiss(already succeeded)
    let members = setup
        .alice
        .call(setup.coa_contract.id(), "get_members")
        .view()
        .await?
        .json::<MembersOutput>()?;

    let mut expected = vec![
        to_near_account(setup.bob.id()),
        to_near_account(setup.john.id()),
    ];
    expected.sort();
    assert_eq!(members.members, expected);

    // verify
    // admin flag
    let res = setup
        .alice
        .call(setup.registry_contract.id(), "account_flagged")
        .args_json(json!({"account": to_near_account(setup.alice.id())}))
        .view()
        .await?
        .json::<Option<AccountFlag>>()?;

    assert_eq!(res, Some(AccountFlag::GovBan));

    let proposal = setup
        .alice
        .call(setup.tc_contract.id(), "get_proposal")
        .args_json(json!({ "id": proposal_id }))
        .view()
        .await?
        .json::<Option<ProposalOutput>>()?;
    assert_eq!(proposal.unwrap().proposal.status, ProposalStatus::Executed);

    Ok(())
}

#[ignore]
#[tokio::test]
async fn migration_mainnet() -> anyhow::Result<()> {
    let worker_sandbox = near_workspaces::sandbox().await?;
    let worker_mainnet = near_workspaces::mainnet().await?;
    let congress_hom_id: AccountId = "congress-hom-v1.ndc-gwg.near".parse()?;
    let congress = worker_sandbox
        .import_contract(&congress_hom_id, &worker_mainnet)
        .initial_balance(parse_near!("10000000 N"))
        .with_data()
        .transact()
        .await?;

    // query the pre-migrated contract
    let num_of_proposals: u64 = congress
        .call("number_of_proposals")
        .max_gas()
        .transact()
        .await?
        .json()?;

    let members: MembersOutput = congress
        .call("get_members")
        .max_gas()
        .transact()
        .await?
        .json()?;

    // deploy the new contract
    let new_congress = congress
        .as_account()
        .deploy(include_bytes!("../../res/congress.wasm"))
        .await?
        .into_result()?;

    // call the migrate method
    let res = new_congress.call("migrate").max_gas().transact().await?;
    assert!(res.is_success(), "{:?}", res.receipt_failures());

    let res: u64 = new_congress
        .call("number_of_proposals")
        .max_gas()
        .transact()
        .await?
        .json()?;
    assert_eq!(res, num_of_proposals);

    let prop: Option<ProposalOutput> = new_congress
        .call("get_proposal")
        .args_json(json!({"id": 1}))
        .max_gas()
        .transact()
        .await?
        .json()?;

    let members_len: u8 = new_congress
        .call("members_len")
        .max_gas()
        .transact()
        .await?
        .json()?;

    assert_eq!(members.members.len() as u8, members_len);

    print!("{:?}", prop.unwrap().proposal);

    Ok(())
}

fn to_near_account(acc: &AccountId) -> NearAccountId {
    NearAccountId::new_unchecked(acc.to_string())
}
