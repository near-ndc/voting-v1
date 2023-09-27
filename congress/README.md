# Congress

- [Diagrams](https://miro.com/app/board/uXjVMqJRr_U=/)
- [Framework Specification](https://near-ndc.notion.site/NDC-V1-Framework-V3-1-Updated-1af84fe7cc204087be70ea7ffee4d23f?pvs=4)

# Queries

- `get_proposals`: Query all proposals
  - `near view $CTR get_proposals '{"from_index": 0, "limit": 10}'`

- `get_proposal`: Query a specific proposal
  - `near view $CTR get_proposals '{"id": 1}'`

- `is_dissolved`: Check if contract is dissolved
  - `near view $CTR is_dissolved ''`

- `get_members`: Query all members with permissions
  - `near view $CTR get_members ''`

- `member_permissions`: Returns permissions for a specific member
  - `near view $CTR member_permissions '{"member": "user.testnet"}'`

- `hook_permissions`: Returns permissions for a specific member
  - `near view $CTR hook_permissions '{"user": "user.testnet"}'`
