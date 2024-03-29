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

### Breaking changes

### Features

### Bug Fixes

## v1.2.0 (2023-12-28)

### Features

- `all_hook_permissions` query to return all hook permissions.

### Improvements

- Remove temporal `add_tc_dismiss_perm` and gas adjustments in the `execute` method related to the wrong Gas settings in the TC proposals (and missing checks mentioned in the v1.1.2 release).

## v1.1.2 (2023-12-28)

### Improvements

- `create_proposal` has additional checks for the function call actions. We observed that too much gas was set in the actions, prohibiting correct contract execution (not enough gas to do self execution).
- Temporal gas adjustment to allow TC execution of dismiss proposals (related to the point above).

### Bug Fixes

- Added Dismiss hook permission to dismiss members of TC by TC (`congress-tc-v1.ndc-gwg.near`).

## v1.1.1 (2023-12-16)

### Features

- Added `add_fun_call_perm` call, that can only be run by the contract authority.

### Bug Fixes

- Updating Transparency Commission mainnet instance to add `FunctionCall` permission.

## v1.1.0 (2023-11-20)

### Features

- `members_len` view method.

### Breaking changes

- added `members_len` field to smart contract.
- renamed `voting_duration` and `min_voting_duration` to `vote_duration` and `min_vote_duration` in the Config and initialization (`new`). Motivation is to make it consistent with the Voting Body Config.

## v1.0.0 (2023-11-01)

### Features

- Store vote timestamps.
- Added `min_voting_duration`. With non zero `min_voting_duration`, a proposal is in progress until:
  - all votes were cast
  - OR voting_duration passed
  - OR `min_voting_duration` passed and the tally can be finalized (proposal reached min amount of approval votes or have enough abstain + reject votes to block the approval).
- Extended `ConfigOutput` to include all contract parameters.

### Breaking changes

- `proposal.votes` map type has changed. The values are `VoteRecord` (vote and the timestamp) instead of `Vote`.
- `execute` method return type changed: from `Result<PromiseOrValue<()>, ExecError>` to `Result<PromiseOrValue<Result<(), ExecRespErr>>, ExecError>`. Note the inner result. If the result is `Ok(PromiseOrValue::Value(Err(..)))` then the transaction will pass (state change will be properly recorded), but the proposal fails to execute due to error reported through `ExecRespError`.

### Bug Fixes

- Proposal status calculation in when querying proposals.
- Proposal: handle budget overflow, when there are two competing proposals.

## v0.2.0 (2023-10-24)

### Features

- Added abstain vote type.
- new `is_member(&self, account: AccountId) -> bool` query function.

### Bug Fixes

- Proposal iterator to handle edge case for the limit parameter.

## v0.1.0 (2023-10-13)
