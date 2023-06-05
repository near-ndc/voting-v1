use std::vec;

use near_sdk::json_types::U128;
use near_sdk::test_utils::test_env::alice;
use near_sdk::ONE_YOCTO;
use near_units::parse_near;
use serde_json::json;
use workspaces::operations::Function;
use workspaces::result::ValueOrReceiptId;
use workspaces::{Account, AccountId, Contract, DevNetwork, Worker};

use crate::util::{Consent, PropType, TokenMetadata, SECOND};

mod util;

async fn init(
    worker: &Worker<impl DevNetwork>,
) -> anyhow::Result<(
    Contract,
    Contract,
    Account,
    Account,
    Account,
    Account,
    Account,
    u32,
)> {
    // deploy contracts
    let ndc_voting_contract = worker
        .dev_deploy(include_bytes!("../../res/ndc_voting.wasm"))
        .await?;

    let registry_contract = worker
        .dev_deploy(include_bytes!("../../res/registry.wasm"))
        .await?;

    let gwg_acc = worker.dev_create_account().await?;
    let sbt_gd_issuer_acc = worker.dev_create_account().await?;
    let authority_acc = worker.dev_create_account().await?;
    let alice_acc = worker.dev_create_account().await?;
    let bob_acc = worker.dev_create_account().await?;

    let conset = Consent {
        quorum: 5,
        threshold: 2,
    };
    let sup_consent = Consent {
        quorum: 5,
        threshold: 3,
    };

    // initialize contracts
    let res = ndc_voting_contract
        .call("new")
        .args_json((
            gwg_acc.to_owned().id(),
            registry_contract.to_owned().id(),
            sbt_gd_issuer_acc.to_owned().id(),
            1,
            sup_consent,
            conset,
            100,
            5,
        ))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    let res = registry_contract
        .call("new")
        .args_json((authority_acc.id(),))
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
        .args_json(json!({"token_spec": token_spec}))
        .deposit(parse_near!("1 N"))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success());

    let block_info = worker.view_block().await?;
    let current_timestamp = (block_info.timestamp() / SECOND) as u32;
    println!("BlockInfo pre-fast_forward {:?}", block_info);

    // create a proposal
    let proposal_id: u32 = alice_acc
    .call(ndc_voting_contract.id(), "creat_proposal")
    // .args_json(json!({"typ": PropType::Constitution, "start": current_timestamp, "title": "TEST", "ref_link": "test.io", "ref_hash": "test.hash" }))
    .args_json(json!({"typ": PropType::Constitution, "start": current_timestamp, "title": "TEST", "ref_link": "test.io", "ref_hash": "test.hash" }))
    .max_gas()
    .transact()
    .await?
    .json()?;

    return Ok((
        ndc_voting_contract,
        registry_contract,
        gwg_acc,
        sbt_gd_issuer_acc,
        authority_acc,
        alice_acc,
        bob_acc,
        1,
    ));
}

#[tokio::test]
async fn example() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_voting_contract, registry_contract, gwg_acc, _, _, alice_acc, bob_acc, proposal_id) =
        init(&worker).await?;

    Ok(())
}
