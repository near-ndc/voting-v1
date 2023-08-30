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

### Breaking Changes

### Bug Fixes

- fix the calculated amount of bonded tokens in `bond` method.

## v1.0.0-rc1 (2023-08-29)

### Features

- `I VOTED` sbt will be minted to the user in while unbonding if voted for all the proposals.

#### New call methods

- `bond` - method to allow users to bond and re-bond (increase their bond). Bonding is required to vote. [docs](https://github.com/near-ndc/gov/blob/main/framework-v1/elections-voting.md)
- `unbond` - method to allow users to unbond the previosuly bonded amount. It is allowed only after the cooldown period. [docs](https://github.com/near-ndc/gov/blob/main/framework-v1/elections-voting.md)
- `accept_fair_voting_policy` - method to allow users to accept the fair voting policy. It is required to vote. [docs](https://github.com/near-ndc/gov/blob/main/framework-v1/elections-voting.md).

##### New query methods

- `proposal_status` - returns weather a proposal is active, at cooldown or finished.
- `accepted_policy` - returns a blake32 policy hash of the most recent accepted policy by the user.
- `user_votes` - returns all the users votes for all the proposals
- `has_voted_on_all_proposals` - returns true if user has voted on all proposals, otherwise false
- `policy` - returns the required policy
- `winners_by_house` - returns a list of winners of the proposal

### Breaking Changes

- The user needs to both accept the voting policy and bond before being allowed to vote.

### Bug Fixes
