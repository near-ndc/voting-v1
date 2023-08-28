use integrations::setup_registry;
use near_sdk::serde::{Serialize, Deserialize};
use near_units::parse_near;
use serde_json::json;
use workspaces::{Account, Contract, DevNetwork, Worker, AccountId};

/// 1ms in nano seconds
//extern crate elections;
use elections::{
    proposal::{ProposalType},
    ProposalView, TokenMetadata, ACCEPT_POLICY_COST, BOND_AMOUNT, MILI_NEAR, MINT_COST,
};

/// 1ms in seconds
const MSECOND: u64 = 1_000_000;

async fn init(
    worker: &Worker<impl DevNetwork>,
) -> anyhow::Result<(Contract, Contract, Account, Account, Account, Account, u32)> {
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
        auth_flagger.clone(),
        iah_issuer.clone(),
        vec![ndc_elections_contract.id().clone()],
    )
    .await?;

    // initialize contracts
    let res1 = ndc_elections_contract
        .call("new")
        .args_json(json!({
            "authority": admin.id(),
            "sbt_registry": registry_contract.id(),
            "policy": policy1(),
            "finish_time": 1,
        }))
        .max_gas()
        .transact();

    assert!(res1.await?.is_success());

    // get current block time
    let block = worker.view_block().await?;
    let now = block.timestamp() / MSECOND; // timestamp in seconds
    let start_time = now + 20 * 1000; // below we are executing 5 transactions, first has 3 receipts, so the proposal is roughtly now + 20seconds
    let expires_at: u64 = now + 100 * 1_000;
    let proposal_expires_at: u64 = expires_at + 25 * 1000;

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

    Ok((
        ndc_elections_contract.to_owned(),
        registry_contract.to_owned(),
        alice,
        bob,
        john,
        auth_flagger,
        proposal_id,
    ))
}

#[tokio::test]
async fn vote_by_human() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, _, alice, _, john, _, proposal_id) = init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(10).await?;

    let res = alice
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [john.id()],}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res);

    Ok(())
}

#[tokio::test]
async fn vote_by_non_human() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, _, _, john, _, _, proposal_id) = init(&worker).await?;

    let non_human = worker.dev_create_account().await?;
    // fast forward to the voting period
    worker.fast_forward(12).await?;

    let res = non_human
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [john.id()],}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_failure(), "resp should be a failure {:?}", res);
    let res_str = format!("{:?}", res);
    assert!(
        res_str.contains("user didn't accept the voting policy"),
        "{}",
        res_str
    );

    Ok(())
}

#[tokio::test]
async fn vote_expired_iah_token() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, _, alice, _, john, _, proposal_id) = init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(70).await?;

    let res = john
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [alice.id()],}))
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
    let (ndc_elections_contract, _, _, _, john, _, proposal_id) = init(&worker).await?;
    let zen_acc = worker.dev_create_account().await?;
    // fast forward to the voting period
    worker.fast_forward(10).await?;

    let res = zen_acc
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [john.id()],}))
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
async fn vote_without_deposit_bond() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, _, _, bob, john, _, proposal_id) = init(&worker).await?;

    let res = bob
        .call(ndc_elections_contract.id(), "accept_fair_voting_policy")
        .args_json(json!({
            "policy": policy1(),
        }))
        .deposit(ACCEPT_POLICY_COST)
        .max_gas()
        .transact()
        .await?;

    assert!(res.is_success(), "{:?}", res);

    // fast forward to the voting period
    worker.fast_forward(10).await?;

    let res = bob
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [john.id()],}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_failure(), "resp should be a failure {:?}", res);
    let failures = format!("{:?}", res.receipt_failures());
    assert!(failures.contains("Voter didn't bond"), "{}", failures);

    Ok(())
}

#[tokio::test]
async fn unbond_amount_before_election_end() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, registry_contract, alice, _, john, _, proposal_id) =
        init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(12).await?;

    let res = alice
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [john.id()],}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res);

    let res1 = alice
        .call(registry_contract.id(), "is_human_call")
        .args_json(
            json!({"ctr": ndc_elections_contract.id(), "function": "unbond", "payload": "{}"}),
        )
        .max_gas()
        .transact()
        .await?;
    assert!(res1.is_failure(), "resp should be a failure {:?}", res1);
    let failures = format!("{:?}", res1.receipt_failures());
    assert!(
        failures.contains("cannot unbond: election is still in progress"),
        "{}",
        failures
    );
    Ok(())
}

#[tokio::test]
async fn unbond_amount() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, registry_contract, alice, _, john, _, proposal_id) =
        init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(12).await?;

    let res = alice
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [john.id()],}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res);

    let balance_before = alice.view_account().await?;
    // fast forward to the end of voting + cooldown period
    worker.fast_forward(200).await?;

    let res1 = alice
        .call(registry_contract.id(), "is_human_call")
        .args_json(
            json!({"ctr": ndc_elections_contract.id(), "function": "unbond", "payload": "{}"}),
        )
        .max_gas()
        .transact()
        .await?;
    assert!(res1.is_success(), "{:?}", res1);

    let balance_after = alice.view_account().await?;
    /*
    Make sure you get back your NEAR - Tx fees - Storage
    There is only one proposal, so all storage fees should be returned minus Tx fees and SBT Mint storage
    */
    let balance_diff = balance_after.balance - balance_before.balance;
    let tx_fees = 3 * MILI_NEAR;
    let min_diff = BOND_AMOUNT - MINT_COST - tx_fees;
    assert!(
        balance_diff > min_diff,
        "diff: {}, min_diff: {}",
        balance_diff,
        min_diff
    );

    // verify voter has i_voted sbt
    verify_i_voted_sbt_tokens_by_owner(registry_contract.id(), ndc_elections_contract.id(), alice).await?;

    Ok(())
}

#[tokio::test]
async fn state_change() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, _, alice, _, john, _, proposal_id) = init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(10).await?;

    let proposal = alice
        .call(ndc_elections_contract.id(), "proposal")
        .args_json(json!({ "prop_id": proposal_id }))
        .view()
        .await?
        .json::<ProposalView>()?;
    assert_eq!(proposal.voters_num, 0);

    let res = alice
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [john.id()],}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res);

    let proposal = alice
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

#[tokio::test]
async fn revoke_vote() -> anyhow::Result<()> {
    let worker = workspaces::sandbox().await?;
    let (ndc_elections_contract, registry_contract, alice, _, john, auth_flagger, proposal_id) =
        init(&worker).await?;

    // fast forward to the voting period
    worker.fast_forward(10).await?;

    // alice votes
    let res = alice
        .call(ndc_elections_contract.id(), "vote")
        .args_json(json!({"prop_id": proposal_id, "vote": [john.id()],}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res.receipt_failures());

    // try to revoke the vote (alice is not blacklisted)
    let res = john
        .call(ndc_elections_contract.id(), "revoke_vote")
        .args_json(json!({"prop_id": proposal_id, "user": alice.id()}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_failure(), "{:?}", res.receipt_outcomes());

    // flag alice as blacklisted
    let res = auth_flagger
        .call(registry_contract.id(), "admin_flag_accounts")
        .args_json(json!({"flag": "Blacklisted", "accounts": [alice.id()], "memo": "test"}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res.receipt_failures());

    // try to revoke the vote again (alice is now blacklisted)
    let res = john
        .call(ndc_elections_contract.id(), "revoke_vote")
        .args_json(json!({"prop_id": proposal_id, "user": alice.id()}))
        .max_gas()
        .transact()
        .await?;
    assert!(res.is_success(), "{:?}", res.receipt_failures());

    Ok(())
}

async fn accept_policy_and_bond(
    registry: Contract,
    election: Contract,
    user: Account,
    policy: String,
) -> anyhow::Result<()> {
    let call_from = user.clone();
    let res = call_from
        .call(election.id(), "accept_fair_voting_policy")
        .args_json(json!({
            "policy": policy,
        }))
        .deposit(ACCEPT_POLICY_COST)
        .max_gas()
        .transact()
        .await?;

    assert!(res.is_success(), "{:?}", res);

    let call_from2 = user.clone();
    let res1 = call_from2
        .call(registry.id(), "is_human_call")
        .args_json(json!({"ctr": election.id(), "function": "bond", "payload": "{}"}))
        .deposit(BOND_AMOUNT)
        .max_gas()
        .transact()
        .await?;
    assert!(res1.is_success(), "{:?}", res1);
    Ok(())
}

pub async fn verify_i_voted_sbt_tokens_by_owner(
    iah_registry: &AccountId,
    issuer: &AccountId,
    owner: Account,
) -> anyhow::Result<()> {
    let res = owner
        .view(iah_registry, "sbt_tokens_by_owner")
        .args_json(json!({
          "account": owner.id(),
          "issuer": issuer,
        }))
        .await?
        .json::<Vec<(AccountId, Vec<OwnedToken>)>>()?;

    if res[0].0.clone() != issuer.clone() || res[0].1.is_empty() {
        Err(anyhow::Error::msg("User does not have I_VOTED SBT"))
    } else {
        Ok(())
    }
}

fn policy1() -> String {
    "f1c09f8686fe7d0d798517111a66675da0012d8ad1693a47e0e2a7d3ae1c69d4".to_owned()
}

#[derive(Deserialize, Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct OwnedToken {
    pub token: u64,
    pub metadata: TokenMetadata,
}