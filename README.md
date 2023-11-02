# NDC v1 Smart Contracts

In NDC v1, there will be only two mechanisms to vote: simple proposal voting and elections. Both are based on stake weight.

- Details about the Framework: [near-ndc/gov](https://github.com/near-ndc/gov).

This repository provides smart contracts for NDC v1 Voting Body.

## Proposal types

| Proposal                                     | Voting Entity | Contract    |
| :------------------------------------------- | :------------ | :---------- |
| Elect house representatives                  | Voting Body   | elections   |
| Constitution ratification                    | Voting Body   | voting body |
| Dissolve a house and call for new elections  | Voting Body   | voting body |
| Setup Package                                | Voting Body   | voting body |
| Budget ratification                          | Voting Body   | voting body |
| Veto HoM Recurrent and Big Funding Proposals | Voting Body   | voting body |
| Budget                                       | HoM           | congress    |
| Transfer Funds                               | HoM           | congress    |
| Funding proposal                             | HoM           | congress    |
| Veto any HoM proposal                        | CoA           | congress    |
| Reinstate representative                     | TC / CoA      | congress    |
| Investigate                                  | TC            | congress    |
| Remove representative                        | TC            | congress    |

In NDC v1, Voting Body can't make proposal for budget management. They can only veto budget proposals.

### General voting rules

- user can only vote for active proposals
- user can not overwrite his vote

### Elections

Elections for NDC v1 Houses.

👉 [**Voting Process**](https://github.com/near-ndc/gov/blob/main/framework-v1/elections-voting.md)

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
