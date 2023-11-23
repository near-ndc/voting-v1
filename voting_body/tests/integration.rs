use congress::view::ProposalOutput;
use congress::{HookPerm, PropKind, PropPerm, ProposalStatus};
use integrations::{instantiate_congress, setup_registry};
use near_sdk::serde::Deserialize;
use near_sdk::serde::Serialize;
use near_units::parse_near;
use near_workspaces::{Account, Contract, DevNetwork, Worker};
use serde_json::json;
use std::collections::HashMap;
use voting_body::types::{CreatePropPayload, VotePayload};
use voting_body::{Consent, Vote};
/// 1s in ms
const MSECOND: u64 = 1_000_000;

#[derive(Deserialize, Serialize, Clone)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Debug))]
#[serde(crate = "near_sdk::serde")]
pub struct TokenMetadata {
    pub class: u64,
    pub issued_at: Option<u64>,
    pub expires_at: Option<u64>,
    pub reference: Option<String>,
    pub reference_hash: Option<String>,
}

pub struct InitStruct {
    pub hom_contract: Contract,
    pub vb_contract: Contract,
    pub registry_contract: Contract,
    pub alice: Account,
    pub bob: Account,
    pub john: Account,
    pub admin: Account,
    pub proposal_id: u32,
    pub vb_members: Vec<Account>,
}

async fn vote(
    users: Vec<Account>,
    registry: &Contract,
    voting_body: &Contract,
    payload: &VotePayload,
) -> anyhow::Result<()> {
    for user in users.into_iter() {
        let res = user.call(registry.id(), "is_human_call_lock")
        .args_json(json!({"ctr": voting_body.id(), "function": "vote", "payload": serde_json::to_string(payload).unwrap(), "lock_duration": 1800000, "with_proof": false}))
        .max_gas()
        .deposit(parse_near!("1 N"))
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
    let vb_contract = worker
        .dev_deploy(include_bytes!("../../res/voting_body.wasm"))
        .await?;

    let admin = worker.dev_create_account().await?;
    let community_fund = worker.dev_create_account().await?;
    let iah_issuer = worker.dev_create_account().await?;
    let alice = worker.dev_create_account().await?;
    let bob = worker.dev_create_account().await?;
    let john = worker.dev_create_account().await?;
    let flagger = worker.dev_create_account().await?;
    let vb_member1 = worker.dev_create_account().await?;
    let vb_member2 = worker.dev_create_account().await?;
    let vb_member3 = worker.dev_create_account().await?;
    let vb_member4 = worker.dev_create_account().await?;

    let registry_contract = setup_registry(
        worker,
        admin.clone(),
        flagger.clone(),
        iah_issuer.clone(),
        vec![],
    )
    .await?;

    // get current block time
    let block = worker.view_block().await?;
    let now = block.timestamp() / MSECOND; // timestamp in milliseconds

    let mut hom_hook = HashMap::new();
    hom_hook.insert(
        vb_contract.id().clone(),
        vec![HookPerm::Dismiss, HookPerm::Dissolve, HookPerm::VetoAll],
    );
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

    let simple_conset = Consent {
        quorum: 2,
        threshold: 2,
    };
    let super_consent = Consent {
        quorum: 3,
        threshold: 2,
    };

    // init voting body
    let res = vb_contract
        .call("new")
        .args_json(json!({"pre_vote_duration": 1800000,
          "vote_duration": 11000, "pre_vote_support": 3,
          "pre_vote_bond": "50000",
          "active_queue_bond": "150000", "accounts": {
            "congress_hom": hom_contract.id(),
            "congress_coa": hom_contract.id(),
            "congress_tc": hom_contract.id(),
            "iah_registry": registry_contract.id(),
            "community_treasury": community_fund.id(),
            "admin": admin.id()
          }, "simple_consent":simple_conset, "super_consent": super_consent
        }))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res);

    // mint iah tokens
    let iah = vec![TokenMetadata {
        class: 1,
        issued_at: Some(0),
        expires_at: None,
        reference: None,
        reference_hash: None,
    }];

    let token_spec = vec![
        (vb_member1.id(), iah.clone()),
        (vb_member2.id(), iah.clone()),
        (vb_member3.id(), iah.clone()),
        (vb_member4.id(), iah),
    ];

    let res = iah_issuer
        .call(registry_contract.id(), "sbt_mint")
        .args_json(json!({ "token_spec": token_spec }))
        .deposit(parse_near!("1 N"))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    Ok(InitStruct {
        hom_contract: hom_contract.to_owned(),
        vb_contract: vb_contract.to_owned(),
        alice,
        bob,
        john,
        admin,
        proposal_id,
        registry_contract,
        vb_members: vec![vb_member1, vb_member2, vb_member3, vb_member4],
    })
}

#[tokio::test]
async fn veto() -> anyhow::Result<()> {
    let worker = near_workspaces::sandbox().await?;
    let setup = init(&worker).await?;

    let proposal: Option<ProposalOutput> = setup
        .hom_contract
        .call("get_proposal")
        .args_json(json!({"id": 1}))
        .max_gas()
        .transact()
        .await?
        .json()?;

    assert_eq!(
        ProposalStatus::InProgress,
        proposal.unwrap().proposal.status
    );

    let create_prop_payload = CreatePropPayload {
        kind: voting_body::PropKind::Veto {
            dao: near_sdk::AccountId::new_unchecked(setup.hom_contract.id().to_string()),
            prop_id: 1,
        },
        description: "veto".to_string(),
    };

    // create veto proposal
    let res = setup.vb_members[0].call(setup.registry_contract.id(), "is_human_call")
    .args_json(json!({"ctr": setup.vb_contract.id(), "function": "create_proposal", "payload": serde_json::to_string(&create_prop_payload).unwrap()}))
    .max_gas()
    .deposit(parse_near!("2 N"))
    .transact()
    .await?;
    assert!(res.is_success(), "{:?}", res);

    let vote_payload = VotePayload {
        prop_id: 1,
        vote: Vote::Approve,
    };
    vote(
        setup.vb_members,
        &setup.registry_contract,
        &setup.vb_contract,
        &vote_payload,
    )
    .await?;

    worker.fast_forward(10).await?;

    let res = setup
        .alice
        .call(setup.vb_contract.id(), "execute")
        .args_json(json!({"id": 1}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res);

    let proposal: Option<ProposalOutput> = setup
        .hom_contract
        .call("get_proposal")
        .args_json(json!({"id": 1}))
        .max_gas()
        .transact()
        .await?
        .json()?;

    assert_eq!(ProposalStatus::Vetoed, proposal.unwrap().proposal.status);

    Ok(())
}

#[tokio::test]
async fn dismiss() -> anyhow::Result<()> {
    let worker = near_workspaces::sandbox().await?;
    let setup = init(&worker).await?;

    let is_member: bool = setup
        .hom_contract
        .call("is_member")
        .args_json(json!({"account": setup.alice.id()}))
        .max_gas()
        .transact()
        .await?
        .json()?;

    assert_eq!(true, is_member);

    let create_prop_payload = CreatePropPayload {
        kind: voting_body::PropKind::Dismiss {
            dao: near_sdk::AccountId::new_unchecked(setup.hom_contract.id().to_string()),
            member: near_sdk::AccountId::new_unchecked(setup.alice.id().to_string()),
        },
        description: "dismiss".to_string(),
    };

    // create dismiss proposal
    let res = setup.vb_members[0].call(setup.registry_contract.id(), "is_human_call")
    .args_json(json!({"ctr": setup.vb_contract.id(), "function": "create_proposal", "payload": serde_json::to_string(&create_prop_payload).unwrap()}))
    .max_gas()
    .deposit(parse_near!("1 N"))
    .transact()
    .await?;
    assert!(res.is_success(), "{:?}", res);

    let vote_payload = VotePayload {
        prop_id: 1,
        vote: Vote::Approve,
    };
    vote(
        setup.vb_members,
        &setup.registry_contract,
        &setup.vb_contract,
        &vote_payload,
    )
    .await?;

    worker.fast_forward(10).await?;

    let res = setup
        .alice
        .call(setup.vb_contract.id(), "execute")
        .args_json(json!({"id": 1}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res);

    let is_member: bool = setup
        .hom_contract
        .call("is_member")
        .args_json(json!({"account": setup.alice.id()}))
        .max_gas()
        .transact()
        .await?
        .json()?;

    assert_eq!(false, is_member);

    Ok(())
}

#[tokio::test]
async fn dissolve() -> anyhow::Result<()> {
    let worker = near_workspaces::sandbox().await?;
    let setup = init(&worker).await?;

    let is_dissolved: bool = setup.hom_contract.view("is_dissolved").await?.json()?;

    assert_eq!(false, is_dissolved);

    let create_prop_payload = CreatePropPayload {
        kind: voting_body::PropKind::Dissolve {
            dao: near_sdk::AccountId::new_unchecked(setup.hom_contract.id().to_string()),
        },
        description: "dissolve".to_string(),
    };

    // create dismiss proposal
    let res = setup.vb_members[0].call(setup.registry_contract.id(), "is_human_call")
    .args_json(json!({"ctr": setup.vb_contract.id(), "function": "create_proposal", "payload": serde_json::to_string(&create_prop_payload).unwrap()}))
    .max_gas()
    .deposit(parse_near!("2 N"))
    .transact()
    .await?;
    assert!(res.is_success(), "{:?}", res);

    let vote_payload = VotePayload {
        prop_id: 1,
        vote: Vote::Approve,
    };
    vote(
        setup.vb_members,
        &setup.registry_contract,
        &setup.vb_contract,
        &vote_payload,
    )
    .await?;

    worker.fast_forward(10).await?;

    let res = setup
        .alice
        .call(setup.vb_contract.id(), "execute")
        .args_json(json!({"id": 1}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res);

    let is_dissolved: bool = setup.hom_contract.view("is_dissolved").await?.json()?;

    assert_eq!(true, is_dissolved);

    Ok(())
}
