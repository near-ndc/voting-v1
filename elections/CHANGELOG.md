<!-- markdownlint-disable MD013 -->
<!-- markdownlint-disable MD024 -->

<!--
Changelogs are for humans, not machines.
There should be an entry for every single version.
The same types of changes should be grouped.
The latest version comes first.
The release date of each version is displayed.

Usage:

Change log entries are to be added to the Unreleased section. Example entry:

* [#<PR-number>](https://github.com/umee-network/umee/pull/<PR-number>) <description>

-->

# CHANGELOG

## Unreleased

## v1.0.0-rc1 (2023-08-29)

### Features

1. bond is called through `registry.is_human_call`

```rust
pub fn bond(
        &mut self,
        caller: AccountId,
        iah_proof: HumanSBTs,
        #[allow(unused_variables)] payload: serde_json::Value,
    ) -> PromiseOrValue<U128>
```

2. unbond is called through `registry.is_human_call`

```rust
pub fn unbond(
        &mut self,
        caller: AccountId,
        iah_proof: HumanSBTs,
        payload: serde_json::Value,
    ) -> Promise
```

3. fair voting policy

```rust
pub fn accept_fair_voting_policy(&mut self, policy: String) -> Promise
```

4. Returns the proposal status (view)

```rust
pub fn proposal_status(&self, prop_id: u32) -> Option<ProposalStatus>
```

5. Returns the policy if user has accepted it otherwise returns None (view)

```rust
pub fn accepted_policy(&self, user: AccountId) -> Option<String>
```

6. Returns all the users votes for all the proposals. If user has not voted yet a vector with None values will be returned. Eg. if we have 3 porposals and user only voted on first one then the return value will look like `[Some([1,2]), None, None]`
   NOTE: the response may not be consistent with the registry. If user will do a soul_transfer, then technically votes should be associated with other user. Here we return votes from the original account that voted for the given user. (view)

```rust
pub fn user_votes(&self, user: AccountId) -> Vec<Option<Vec<usize>>>
```

7. Returns true if user has voted on all proposals, otherwise false. (view)

```rust
pub fn has_voted_on_all_proposals(&self, user: AccountId) -> bool
```

8.  Returns the required policy (view)

```rust
pub fn policy(&self) -> String
```

9. Returns a list of winners of the proposal if the elections is over and the quorum has been reached, otherwise returns empty list. A candidate is considered the winner only if he reached the `min_candidate_support`. If the number of returned winners is smaller than the number of seats it means some of the candidates did not reach the required minimum support. (view)

```rust
pub fn winners_by_house(&self, prop_id: u32) -> Vec<AccountId>
```

### Breaking Changes

### Bug Fixes

## v0.1.0 `1c4ae7f` (2023-07-12)
