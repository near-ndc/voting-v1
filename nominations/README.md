# Nominations

Smart contract for nominations.
[Specification](https://near-ndc.notion.site/Nominations-b4281e30ac4e44cfbd894f0e2443bc88?pvs=4)

Smart contract is primarly used for NDC v1 Elections, but can be used also for other use cases (eg Kudos).

## Transactions

- `self_nominate(house: HouseType, comment: String,link: Option<String>)` - allows OG members to submit a nomination.
- `self_revoke()` - enables candidates to revoke their nomination.
- `upvote(candidate: AccountId)` - enables IAH token holders to upvote existing nominations.
- `remove_upvote(candiate: AccountId)` - removes the upvote from the caller for the specified candidate.
- `comment(candidate: AccountId, comment: String)` - enables IAH token holders to comment on existing nominations

## Queries

- `nominations(&self, house: HouseType) -> Vec<(AccountId, u32)>` - returns all the nominations for the given house with the numbers of upvotes recived eg. `[("candidate1.near", 16), ("candidate2.near", 5), ...]`.

Comment and upvote queries should be go through an indexer.

## Deployed Contracts

### Mainnet:

Coming Soon

### Testnet:

- **nominations-v1**: `nominations-v1.gwg.testnet`, initialized with values:
  ```JSON
  sbt_registry: registry-unstable.i-am-human.testnet,
  iah_issuer: i-am-human-staging.testnet,
  og_class: 1,
  og_issuer: community-v1.i-am-human.testnet,
  start_time: 0,
  end_time: 1844674407370955300`
  ```
