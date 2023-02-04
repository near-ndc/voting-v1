# NDC v1 Smart Contracts

In NDC v1, there will be only two mechanisms to vote: simple proposal voting and elections. Both are based on stake weight.

- Details about the Framework: [near-ndc/gov](https://github.com/near-ndc/gov).

## Proposal types

- Constitution ratification → Voting Body
- Elect house representatives → Voting Body
- Dissolve a house and call for new elections → Voting Body
- Veto HoM (1) Big Budget Decisions, and (2) Recurring Budget Items → Voting Body
- Transfer Funds → HoM
- Budget proposal → HoM
- Veto → CoA
- Reinstate representative → TC / CoA
- Investigate → TC
- Remove representative → TC

NDC can only make Voting Body proposals. This repository provides smart contracts for NDC v1.

### General voting rules

- user can only vote for active proposals
- user can overwrite his vote

### Stake weighted voting

The main purpose is Constitution Ratification - passes when [NEAR Supermajority Constent](https://github.com/near-ndc/gov/blob/main/framework-v1/ratification-and-election-process.md#voting) is met.

Setting a proposal will require a big bond of NEAR (to be defined). Such proposal passes when [NEAR Consent](https://github.com/near-ndc/gov/blob/main/framework-v1/ratification-and-election-process.md#voting) is met.

### Stake Weighted Elections

Elections for NDC v1 Houses.
Candidates can only be submitted by GWG, following the process approved by the community.

## TODO

- [ ] Organize documentation between this repo and near-ndc/gov. Move some content to near-ndc/gov.
- [ ] Decide about admin / gwg features. Currently:
  - admin is a GWG DAO
  - only admin can create constitution proposal
- [ ] Decide about NDC proposal deposits
- [ ] Add vote overwrite (user should be able to vote multiple times)
