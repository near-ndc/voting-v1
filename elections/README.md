# Elections Smart Contract

## Requirements

- Only I Am Human verified accounts can vote.
- Each account can vote at most one time. Vote
- Only an authority (set during contract initialization) can create proposals. Each proposal specifies:

  - `typ`: must be HouseType variant
  - `start`: voting start time as UNIX time (in seconds)
  - `end`: voting start time as UNIX time (in seconds)
  - `ref_link`: string (can't be empty) - a link to external resource with more details (eg near social post). Max length is 120 characters.
  - `quorum`: minimum amount of legit accounts to vote to legitimize the elections.
  - `seats`: max number of candidates to elect, also max number of credits each user has when casting a vote.

## Usage

Below we show few CLI snippets:

```shell
CTR=elections-v1.gwg.testnet
REGISTRY=registry-1.i-am-human.testnet

# create proposal

near call $CTR creat_proposal '{"start": 1686221747, "end": 1686653747, "ref_link": "example.com", "quorum": 10, "candidates": ["candidate1.testnet", "candidate2.testnet", "candidate3.testnet", "candidate4.testnet"], "typ": "HouseOfMerit", "seats": 3}' --accountId $CTR

# vote

near call $CTR vote '{"prop_id": 1, "vote": ["candidate1.testnet", "candidate3.testnet"]}' --gas 70000000000000 --deposit 0.002 --accountId me.testnet
```
