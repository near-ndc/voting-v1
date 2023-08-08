# NDC v1 Smart Contracts

In NDC v1, there will be only two mechanisms to vote: simple proposal voting and elections. Both are based on stake weight.

- Details about the Framework: [near-ndc/gov](https://github.com/near-ndc/gov).

This repository provides smart contracts for NDC v1 Voting Body.

## Proposal types

| Proposal                                                 | Voting Entity | Contract    |
| :------------------------------------------------------- | :------------ | :---------- |
| Elect house representatives                              | Voting Body   | elections   |
| Constitution ratification                                | Voting Body   | voting_body |
| Dissolve a house and call for new elections              | Voting Body   | voting_body |
| Setup Budget (fund HoM DAO)                              | Voting Body   | voting_body |
| Veto any HoM proposal (in principle any fund deployment) | Voting Body   | voting_body |
| Transfer Funds                                           | HoM           | Astra++     |
| Budget proposal                                          | HoM           | Astra++     |
| Veto                                                     | CoA           | Astra++     |
| Reinstate representative                                 | TC / CoA      | Astra++     |
| Investigate                                              | TC            | Astra++     |
| Remove representative                                    | TC            | Astra++     |

In NDC v1, Voting Body can't make proposal for budget management. They can only veto budget proposals.

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

### Voting Body

Voting Body is set of human verified NEAR accounts constituting NDC.

Setting a proposal will require a big bond of NEAR (to be defined).

The main purposes is Constitution Ratifications - passes when [NEAR Supermajority Consent](https://github.com/near-ndc/gov/blob/main/framework-v1/ratification-and-election-process.md#voting) is met.

Moreover, Voting Body can make the following proposals, that will pass when [NEAR Consent](https://github.com/near-ndc/gov/blob/main/framework-v1/ratification-and-election-process.md#voting) is met.

- Propose and approve HoM **setup package**: a request to deploy funds from the [Community Treasury](https://github.com/near-ndc/gov/blob/main/framework-v1/community-treasury.md) to HoM DAO.
- **Voting Body Veto** is a special proposal to veto other proposal made by a house. When a HoM or CoA proposal will pass it must not be executed immediately. There must be an challenge period, where a Voting Body or the TC can stop the proposal execution by successfully submitting a Veto proposal.

## TODO

- [ ] Decide about admin / gwg. Currently:
  - admin is a GWG DAO
  - only admin can create constitution proposal
- [ ] Decide about NDC proposal deposits to allow others to create proposals
  - would be good to consider adding veto in addition to yes/no/abstain votes.
- [ ] Fork Astro DAO to add Veto Hooks and challenge period and Veto proposal (to allow TC to veto HoM).
