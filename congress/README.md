# Congress

- [Diagrams](https://miro.com/app/board/uXjVMqJRr_U=/)
- [Framework Specification](https://near-ndc.notion.site/NDC-V1-Framework-V3-1-Updated-1af84fe7cc204087be70ea7ffee4d23f?pvs=4)

# Queries

- `get_proposals`: Query all proposals
  - `near view $CTR get_proposals '{"from_index": 0, "limit": 10}'`

- `get_proposal`: Query a specific proposal
  - `near view $CTR get_proposals '{"id": 1}'`

- `is_dissolve`: Check if contract is dissolved
  - `near view $CTR is_dissolved ''`

- `get_members`: Query all members with permissions
  - `near view $CTR get_members ''`

- `member_permissions`: Returns permissions for a specific member
  - `near view $CTR member_permissions '{"member": "user.testnet"}'`

- `hook_permissions`: Returns permissions for a specific member
  - `near view $CTR hook_permissions '{"user": "user.testnet"}'`

# Execution

 // initialize
- `near call congress-test.testnet new '{"community_fund": "ayt.testnet", "start_time": 1695726777895, "end_time": 1698318813000, "cooldown": 3600000, "voting_duration": 14400000, "members": ["ay.testnet", "megha19.testnet", "rubycoptest.testnet"], "member_perms": ["FunctionCall", "Text", "FundingRequest", "RecurrentFundingRequest"], "hook_auth": {"vbody.testnet": ["Dismiss", "Dissolve"], "coa3.testnet": ["Veto"]}, "budget_cap": "1000000000000000000000000000000", "big_budget_balance": "100000000000000000000000000000"}' --accountId congress-test.testnet`

// create_proposal
- 

// vote on proposal
- 

// execute proposal
- 