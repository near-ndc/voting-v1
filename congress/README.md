# Congress

- [Diagrams](https://miro.com/app/board/uXjVMqJRr_U=/)
- [Framework Specification](https://near-ndc.notion.site/NDC-V1-Framework-V3-1-Updated-1af84fe7cc204087be70ea7ffee4d23f?pvs=4)

## Creating a Proposal

Each Congress house specifies which proposal kinds are allowed to be created. It is a subset of:

- `FunctionCall`: if approved, proposal execution will create a cross contract call.
- `Text`: text based proposal, no automated action is performed.
- `FundingRequest(Balance)`: request to fund a specific project. Balance is the amount of Near provided as funding. If Balance is bigger or equal than `big_funding_threshold` then it is eligible for `VetoBigOrReccurentFundingReq`. Proposal execution will fail if the total budget spend (including the one from the proposal) goes above the `contract.budget_cap`.
  NOTE: The contract doesn't track the monthly budget limit. That should be tracked off-chain.
- `RecurrentFundingRequest(Balance)`: funding request that will renew every month until the end of the terms. The balance parameter is the size of the single month spending for this funding request. The proposal is eligible for
  `VetoBigOrReccurentFundingReq`. Proposal execution will fail if the total budget spend (including the one from the proposal multiplied by the amount of remaining months) goes above the `contract.budget_cap`.
- `DismissAndBan(member, house)`: requests I Am Human registry to ban the member account (set `GovBan` flag in the IAH registry) and calls the dismiss hook on the house.

Each proposal comes with a description, which should provide motivation and a background.

### Cheat sheet

Below we present how to make a proposal for every possible Congress motion:

#### Veto

Veto is created by a separate account or DAO that has a permission to call the `veto_hook`. `VetoAll` permission allows to veto any proposal, while `VetoBigOrReccurentFundingReq` only allows to veto recurrent funding proposals or funding proposals above `big_funding_threshold`.
In NDC Gov v1 the following entities have permission to veto HoM proposals:

- `voting-body-v1.ndc-gwg.near`: `VetoBigOrReccurentFundingReq`
- `congress-coa-v1.ndc-gwg.near`: `VetoAl`

To create a veto within the congress houses, use the `create_proposal` function:

```json
near call congress-coa-v1.ndc-gwg.near create proposal '{
  "kind": {
    "FunctionCall": {
      "receiver_id": "congress-hom-v1.ndc-gwg.near",
      "actions": [
        {
          "method_name": "veto_hook",
          "args": {"id": "proposal_id to be vetoed"},
          "deposit": "100000000000000000000000",
          "gas": "300000000000000"
        }
      ]
    }
  },
  "description": "Your description"
}' --accountId your_account.near
```

See also the `Voting Body` documentation.

#### Dismiss

To initiate a dismiss, the executing house must have `Dismiss` permission. In NDC Gov v1, the authority to dismiss any member of the `HoM` and `CoA` rests with the `TC` (Transparency Commision).

To propose a dismiss proposal, call the `create_proposal` function:

```json
near call congress-coa-v1.ndc-gwg.near create proposal '{
  "kind": {
    "FunctionCall": {
      "receiver_id": "congress-hom-v1.ndc-gwg.near",
      "actions": [
        {
          "method_name": "dismiss_hook",
          "args": {"member": "member_to_dismiss.near"},
          "deposit": "100000000000000000000000",
          "gas": "300000000000000"
        }
      ]
    }
  },
  "description": "Your description"
}' --accountId your_account.near
```

#### Dismiss and Ban

To initiate a dismiss and ban action, the executing house must have `DismissAndBan` permission. In NDC Gov v1, the authority to dismiss any member of the `HoM` and `CoA` rests with the `TC` (Transparency Commision).

To propose a dismiss and ban proposal, call the `create_proposal` function:

```json
near call congress-tc-v1.ndc-gwg.near create proposal '{
  "kind": {
    "DismissAndBan": {
      "member": "member_to_dismiss_and_ban.near",
      "house": "congress-hom-v1.ndc-gwg.near",
    },
  },
  "description": "Your description"
}' --accountId your_account.near
```


## Proposal Lifecycle

When a proposal is created it will have `InProgress` status and the `submission_time` will be set.

```mermaid
---
title: Possible Proposal Status Flows
---
flowchart TB
    Create_Proposal --> InProgress
    InProgress --> Approved
    InProgress --> Rejected
    InProgress --> Vetoed
    Approved --> Executed
    Approved --> Failed
    Approved --> Vetoed
    Failed -- re-execute --> Executed
```

### Voting

Any member can vote for any `InProgress` proposal. Members can't overwrite their votes. Proposal is in progress until:

- all votes were cast
- OR `vote_duration` passed
- OR `min_vote_duration` passed and the tally can be finalized (proposal reached min amount of approval votes or have enough abstain + reject votes to block the approval).


Example CLI command to vote for a proposal:

``` shell
# vote must be one of: "Approve", "Reject", "Abstain"
near call HOUSE vote '{"id": PROP_ID, "vote": "Approve"}' --accountId YOU
```


### Vetoing

Any proposal can be vetoed (even an in progress one) until the cooldown is over.
A DAO `A` can veto a proposal `P` of house `H` if:

- `P` cooldown is not over (is in progress, approved or rejected).
- `H` gives veto permission to `A`: `contract.hook_auth[A]` contains `VetoAll` or `VetoBigOrReccurentFundingReq`. The latter will only allow `A` to veto big funding proposals or recurrent funding proposals.

### Execution

Anyone (not only a house member) can execute a proposal when a proposal that is:

- min vote duration passed;
- cooldown is over;
- proposal is _approved_ or _failed_.

A proposal is **approved** when:

- is not in progress;
- AND got enough #approved votes (`>= contract.threshold`).

Proposal reaches _failed_ status when it was approved, but the execution failed. In that can be re-executed again.

If proposal execution breaks an invariant check (eg: crossing the budget cap), then the transaction will succeed and a composed error will be returned: the `Ok(Err(ExecRespErr::**))` of `Result<PromiseOrValue<Result<(), ExecRespErr>>, ExecError>` type.

Example CLI command to execute a proposal:

``` shell
near call HOUSE execute '{"id": PROP_ID}' --gas 300000000000000 --accountId YOU
```

## Queries

- `get_proposals`: Query all proposals

  - `near view $CTR get_proposals '{"from_index": 0, "limit": 10}'`

- `get_proposal`: Query a specific proposal

  - `near view $CTR get_proposal '{"id": 1}'`

- `number_of_proposals`: Query a specific proposal

  - `near view $CTR number_of_proposals ''`

- `is_dissolved`: Check if contract is dissolved

  - `near view $CTR is_dissolved ''`

- `get_members`: Query all members with permissions

  - `near view $CTR get_members ''`

- `member_permissions`: Returns permissions for a specific member

  - `near view $CTR member_permissions '{"member": "user.testnet"}'`

- `hook_permissions`: Returns permissions for a specific member
  - `near view $CTR hook_permissions '{"user": "user.testnet"}'`
