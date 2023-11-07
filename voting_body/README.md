# Voting Body

- [Diagrams](https://miro.com/app/board/uXjVMqJRr_U=/)
- [Framework Specification](https://www.notion.so/NDC-V1-Framework-V3-2-Updated-1af84fe7cc204087be70ea7ffee4d23f)

Voting Body is governance structure of all non-blacklisted human participants in the NEAR Ecosystem. There are no requirements for being a member of the voting body beyond completing ‘I am Human’ and not being on the blacklist.

Deployed Contracts

* mainnet: `voting-body-v1.ndc-gwg.near`
* testnet: `voting-body-v1.gwg.testnet`, IAH registry: `registry-unstable-v2.i-am-human.testnet`

## Contract Parameters

- `quorum`: a minimum amount of members that need to vote to approve a proposal.
- `pre_vote_support`: minimum amount of support, a proposal has to receive in order to move it to the active queue, where users can vote to approve a proposal.
- `pre_vote_duration`: max amount of time, users can express support to move a proposal to the active queue, before it will be removed.
- `pre_vote_bond`: amount of N required to add a proposal to the pre-vote queue.
- `active_queue_bond`: amount of N required to move a proposal directly to the active queue.
- `vote_duration`: max amount of time a proposal can be active in the active queue. If a proposal didn't get enough approvals by that time, it will be removed and bond returned.


## Creating proposals

Every human can create a proposal. The proposals are organized in 2 queues (pre-vote queue and active queue) in order to filter out spam proposals.
When creating a proposal, the submitter must stake a bond. If `pre-vote bond` is attached, then the proposal goes to the pre-vote queue. If active queue bond is attached then the proposal goes directly to active queue.

Proposal can only be created by an IAH verified account. We use `is_human_call` method. Example call:

```shell
near call IAH_REGISTRY is_human_call \
  '{"ctr": "VB.near", "function": "create_proposal", "payload": "{\"kind\": {\"Veto\": {\"dao\": \"hom.near\", \"prop_id\": 12}}, \"description\": \"this proposal is against the budget objectives\"}"}' \
  --accountId YOU \
  --depositYocto $pre_vote_bond


# Creating a text proposal with with a active_queue_bond (making it automatically advancing to the active queue)
near call IAH_REGISTRY  is_human_call \
  '{"ctr": "VB.near", "function": "create_proposal", "payload": "{\"kind\": \"Text\", \"description\": \"lets go\"}"}' \
  --accountId YOU --deposit $active_queue_bond
```

### Pre-vote queue

Proposals in this queue are not active. VB members can't vote for proposals in the pre-vote queue and UI doesn't display them by default. Instead, members can send a _pre_vote_support_ transaction. There are 3 ways to move a proposal to the active queue:

- get pre-vote support;
- top up with more NEAR to reach `active_queue_bond`;
- get a support by one of the Congress members using `support_proposal_by_congress`.

Note: originally only a congress support was required to move a proposal to the active queue. However, that creates a strong subjectivity and censorship (example: VB wants to dismiss a house - obviously house may not be happy and not "support" such a proposal).

Voting Body Support can only be made by an IAH verified account. We use `is_human_call_lock` method, which will lock the caller for soul transfers, to avoid double support. Example call:

```shell
lock_duration=pre_vote_duration+1
near call IAH_REGISTRY is_human_call_lock \
  '{"ctr": "VB.near", "function": "support_proposal", "payload": "1", "lock_duration": '$lock_duration', "with_proof": false}' \
  --accountId YOU
```

### Active queue

Proposals in this queue are eligible for voting and displayed by the default in the UI. Proposals from the active queue are not removed unless they are marked as spam (more about it in the voting section). They are preserved and anyone can query them, even when a proposal was rejected.

When a proposal is moved from the pre-vote queue to the active queue, the set of accounts that supported the proposal is cleared - it's not needed any more. We can save the space and proposal load time.

```mermaid
---
title: Proposal Queues
---
flowchart TB
    PVQ[pre-vote queue]
    AVQ[active queue]
    Trash(Proposal burned)

    Proposal-- stake \npre-vote bond -->PVQ
    Proposal-- stake \nactive queue bond-->AVQ
    PVQ-- top up bond -->AVQ
    PVQ-- receive \n#pre-vote support -->AVQ
    PVQ-- get a support from a \nCongress member -->AVQ
    PVQ-- timeout -->Trash
```

### Proposal Types

There are several types of proposals with specific functionalities and limitations:

1. **Dismiss Proposal**

   - Arguments: `dao`: `AccountId`, `member`: `AccountId`
   - Description: This proposal calls the `Dismiss` hook in the provided DAO when executed, resulting in the removal of the specified member.

2. **Dissolve Proposal**

   - Arguments: `dao`: `AccountId`
   - Description: Executing this proposal triggers the `Dissolve` hook in the provided DAO, dissolving the DAO itself.

3. **Veto Proposal**

   - Arguments: `dao`: `AccountId`, `prop_id`: `u32`
   - Description: When executed, this proposal invokes the `Veto` hook in the provided DAO and vetoes the proposal identified by the specified `prop_id`.

4. **Approve Budget Proposal**

   - Arguments: `dao`: `AccountId`, `prop_id`: `u32`
   - Description: This type of proposal serves as an approval mechanism for budget proposals without making any method calls.

5. **Text Proposal**

   - Description: A text proposal for general purposes, without specific arguments. It doesn't involve any method calls.

6. **FunctionCall Proposal**

   - Arguments: `receiver_id`: `AccountId`, `actions`: `Vec<ActionCall>`
   - Description: This proposal enables you to call the `receiver_id` with a list of method names in a single promise. It allows your contract to execute various actions in other contracts, excluding congress contracts. Attempting to create a proposal that calls any congress DAOs will result in an error, preventing the proposal from being created.

7. **UpdateBonds**

   - Arguments: `pre_vote_bond: U128`, `active_queue_bond: U128`
   - Description: allows VB to update contract configuration.

8. **UpdateVoteDuration**

   - Arguments: `pre_vote_duration: u64`, `vote_duration: u64`
   - Description: allows VB to update contract configuration.

## Proposal Lifecycle

```mermaid
---
title: Possible Proposal Status Flows
---
flowchart TB
    Trash([Trash])
    PreVote --> InProgress
    InProgress --> Approved
    InProgress --> Rejected
    InProgress --> Spam
    Approved --> Executed
    Executed -- tx fail --> Failed
    Failed -- re-execute --> Executed

    PreVote -- slashed --> Trash
    Spam -- slashed --> Trash
```

When proposal is created, but the creator doesn't deposit `active_queue_bond` immediately, then the status of a proposal is `PreVote`.
A proposal that doesn't advance to the active queue by the `pre_vote_duration` is eligible for slashing. In such case, any account can call `slash_prevote_proposal(id)` method: the proposal will be removed, `SLASH_REWARD` will be transferred (as in incentive) to the caller and the remainder bond will be sent to the community fund.

Proposal, that is moved to the active queue has status `InProgress` and keeps that status until the voting period is over (`proposal.start_time + vote_duration`). During that time all Members can vote for the proposal.

Once the voting period is over, a proposal will have `Approved`, `Rejected` or `Spam` status, based on the voting result.
During this time, anyone can call `execute(id)`. Note these statuses are only visible when we query a proposal and: a) voting is over b) and was not executed. Executing a proposal will set the `proposal.executed_at` property to the current time in milliseconds and will have the following effects:

- Approved: bonds are returned. If a proposal involves a function call, then the call is scheduled. If the call fails, the proposal will have status `Failed` and anyone will be able to re-execute it again.
- Rejected: bonds are removed, and proposal won't be able to be re-executed.
- Spam: executor will receive a `SLASH_REWARD`, and the proposal will be slashed: removed, and the remaining bond (including the top-up) send to the community fund.

## Voting

Any VB member can vote on any _in progress_ proposal in the active queue. Voter can change his/her vote multiple times. Vote options:

- approve
- reject
- spam: strong conviction that the proposal is spam, should be removed and a deposit slashed.

A proposal voting is in progress when `now <= proposal.start_time + vote_duration`, where `proposal.start_time` is a time when the proposal is added to the active queue.

Syntax: #vote_type denotes number of votes of the specified type, eg: #approve means number of approve votes.

A proposal is **approved** when:

- voting time is over;
- AND consent is reached (quorum + threshold).

A proposal is marked as **spam** when:

- voting time is over;
- `#spam > #reject`;
- AND `#reject + #spam >= (1-threshold) * (#approve + #reject + #spam)`.

Spam proposals are removed, and the bond is slashed (sent to the community treasury).

A proposal is **rejected** if voting time is over (proposal is not in progress anymore), and it was not approved nor marked as spam.

Voting Body intentionally doesn't support optimistic execution, that is approving or rejecting a proposal once sufficient amount of votes are cast. We want to give a chance to every member vote and express their opinion providing more clear outcome of the voting.

Vote can only be made by an IAH verified account. We use `is_human_call_lock` method, which will lock the caller for soul transfers, to avoid double vote. Example call:

```shell
lock_duration=vote_duration+1  # minimum value is the time in milliseconds remaining to the voting end + 1.
near call IAH_REGISTRY is_human_call_lock \
  '{"ctr": "VB.near", "function": "vote", "payload": "{\"prop_id\": 3, \"vote\": \"Approve\"}", "lock_duration": '$lock_duration', "with_proof": false}' \
  --accountId YOU \
  --deposit 0.01 # used for vote storage, reminder will be returned.

near call VOTING_BODY get_vote \
  '{"id": 3, "voter": "YOU"}'
```

### Quorums and Thresholds

**Quorum** assures that enough of the VB members voted.
**Majority Threshold** assures that enough VB members approved a proposal. It is a fractional value. Proposal is approved when: `#approve > threshold * (#approve + #reject + #spam)`. It is either a simple majority or a super majority.

- **Near Consent:** quorum=(7% of the voting body) + **simple majority**=50%.
- **Near Supermajority Consent**: quorum=(12% of the voting body) + **super majority**=60%.

## Cheat Sheet

### Creating a Budget Approval proposal

1. HoM must create a Budget Proposal and approve it.

2. CoA must not veto it.

3. Once cooldown is over (cooldown starts once the proposal is internally approved), and it was not vetoed, then it's finalized.

4. Any human can can now create a VB Text proposal, referencing original HoM Budget proposal, example:

   ```shell
   near call IAH_REGISTRY is_human_call \
   '{"ctr": "VB.near", "function": "create_proposal", "payload": "{\"kind\": {\"ApproveBudget\": {\"dao\": \"HOM.near\", \"prop_id\": 12}}, \"description\": \"ADDITIONAL INFORMATION\"}"}' \
   --accountId YOU \
   --depositYocto $pre_vote_bond
   ```

5. Now we need to advance the proposal to the active queue. The easiest way is to ask any Congress member (HoM or other house) to support it. Below, `prop_id` must be the id of the proposal created above, `dao` must be the house address and the caller is member of (eg: `congress-hom-v1.ndc-gwg.near`).

   ```shell
   near call VB support_proposal_by_congress \
     '{"prop_id": 5, `dao`: "HOM"}' \
     --accountId YOU
   ```

6. Share the proposal ID with others and ask the VB to vote.
