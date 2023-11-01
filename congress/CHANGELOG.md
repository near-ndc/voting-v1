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

### Features

### Breaking changes

### Bug Fixes

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
