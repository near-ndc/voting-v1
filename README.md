# NDC v1 Smart Contracts

In NDC v1, there will be only two mechanisms to vote: simple proposal voting and elections. Both are based on stake weight.

- Details about the Framework: [near-ndc/gov](https://github.com/near-ndc/gov).

## Proposal types

| Proposal                                                          | Voting Entity |
| :---------------------------------------------------------------- | :------------ |
| Elect house representatives                                       | Voting Body   |
| Constitution ratification                                         | Voting Body   |
| Dissolve a house and call for new elections                       | Voting Body   |
| Veto HoM (1) Big Budget Decisions, and (2) Recurring Budget Items | Voting Body   |
| Transfer Funds                                                    | HoM           |
| Budget proposal                                                   | HoM           |
| Veto                                                              | CoA           |
| Reinstate representative                                          | TC / CoA      |
| Investigate                                                       | TC            |
| Remove representative                                             | TC            |

NDC can only make Voting Body proposals. This repository provides smart contracts for NDC v1 Voting Body:

- `voting`: implements _approval_ proposals for constitution, house dissolve, and veto other proposal.
- `elections`: implements election proposals.

### General voting rules

- user can only vote for active proposals
- user can overwrite his vote

### Elections

Elections for NDC v1 Houses.
Candidates can only be submitted by the Voting Committee, following the process approved by the community.

- recast vote: not possible. Each voter can only vote once in each round (proposal).
- tie break:
  - only matters if there is a tie at the very tail
  - options: extend or reduce seats, tie break session - elections only for people at tie.
  - robert: tie break session with reduced voting period (eg 2 days)

### Approval Voting

The main purpose is Constitution Ratification - passes when [NEAR Supermajority Constent](https://github.com/near-ndc/gov/blob/main/framework-v1/ratification-and-election-process.md#voting) is met.

Setting a proposal will require a big bond of NEAR (to be defined). Such proposal passes when [NEAR Consent](https://github.com/near-ndc/gov/blob/main/framework-v1/ratification-and-election-process.md#voting) is met.

**Voting Body Veto** is a special proposal to veto other proposal made by a house. When a HoM or CoA proposal will pass it must not be executed immediately. There must be an challenge period, where a Voting Body or the TC can stop the proposal execution by successfully submitting a Veto proposal.

## TODO

- [ ] Decide about admin / gwg. Currently:
  - admin is a GWG DAO
  - only admin can create constitution proposal
- [ ] Decide about NDC proposal deposits to allow others to create proposals
  - would be good to consider adding veto in addition to yes/no/abstain votes.
- [ ] Fork Astro DAO to add Veto Hooks and challenge period and Veto proposal (to allow TC to veto HoM).
