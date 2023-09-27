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

New methods:

- `admin_set_finish_time(time)`: allows the contract authority to overwrite the existing finish time (extending the cooldown).
- `finish_time()`: query the finish time (time when the cooldown is over and unbonding is possible).

## v1.0.0 (2023-09-06)

### Features

New methods:

- `bond_by_sbt` query to check if a holder of given SBT bonded.

### Breaking Changes

- `winners_by_house` renamed to ``winners_by_proposal`

### Bug Fixes

- fix the calculated amount of bonded tokens in `bond` method.
- fix the amount of winners returned in the `winners_by_house` method.

## v1.0.0-beta1 (2023-08-29)

### Features

- `I VOTED` sbt will be minted to the user in while unbonding if voted for all the proposals.

#### New call methods

- `bond` - method to allow users to bond and re-bond (increase their bond). Bonding is required to vote. [docs](https://github.com/near-ndc/gov/blob/main/framework-v1/elections-voting.md)
- `unbond` - method to allow users to unbond the previously bonded amount. It is allowed only after the cooldown period. [docs](https://github.com/near-ndc/gov/blob/main/framework-v1/elections-voting.md)
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
- User doesn't need to make storage deposit to cover voting. Bond is used to cover that.

### Bug Fixes
