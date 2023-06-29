# Elections Smart Contract

## Requirements

- Only I Am Human verified accounts can vote.
- Each account can vote at most one time. Votes are not revocable, and can't be changed.
- Only the authority (set during contract initialization) can create proposals. Each proposal specifies:

  - `typ`: must be HouseType variant
  - `start`: voting start time as UNIX time (in seconds)
  - `end`: voting start time as UNIX time (in seconds)
  - `ref_link`: string (can't be empty) - a link to external resource with more details (eg near social post). Max length is 120 characters.
  - `quorum`: minimum amount of legit accounts to vote to legitimize the elections.
  - `seats`: max number of candidates to elect, also max number of credits each user has when casting a vote.

## Flow

- GWG deploys the elections smart contract and sets authority for creating new proposals.
- GWG authority creates new proposals before the election starts, with eligible candidates based on the `nominations` result. All proposals are created before the elections start.
  - NOTE: we may consider querying the candidates directly from the nominations contract.
- Once the proposals are created and the elections start (`now >= proposal.start`), all human verified near accounts can vote according to the NDC Elections [v1 Framework](../README.md#elections).
- Anyone can query the proposal and the ongoing result at any time.
- Voting is active until the `proposal.end` time.

## Usage

Below we show few CLI snippets:

```shell
CTR=elections-v1.gwg.testnet
REGISTRY=registry-1.i-am-human.testnet

# create proposal
# note: start and end time must be in milliseconds

near call $CTR create_proposal '{"start": 1686221747000, "end": 1686653747000, "ref_link": "example.com", "quorum": 10, "candidates": ["candidate1.testnet", "candidate2.testnet", "candidate3.testnet", "candidate4.testnet"], "typ": "HouseOfMerit", "seats": 3}' --accountId $CTR

# fetch all proposal

near view $CTR proposals ''

# query proposal by ID

near view $CTR proposals '{"prop_id": 2}'

# vote

near call $CTR vote '{"prop_id": 1, "vote": ["candidate1.testnet", "candidate3.testnet"]}' --gas 70000000000000 --deposit 0.002 --accountId me.testnet
```
