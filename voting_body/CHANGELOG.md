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

- `members_len` view method

### Breaking changes

- added `members_len` field to smart contract

### Bug Fixes

## v1.1.0 (2023-11-08)

### Features

- New proposal type: `TextSuper` - used for text proposals that require Near Supermajority Consent.

### Bug Fixes

- `PropKind::ApproveBudget` requires simple Near Consent (not Near Supermajority Consent as it was done before). 

## v1.0.1 (2023-11-08)


### Breaking changes

- Rename and change `REMOVE_REWARD = 1N` to `SLASH_REWARD = 0.9N`



## v1.0.0 (2023-11-07)
