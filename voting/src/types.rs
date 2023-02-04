use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};

#[derive(BorshDeserialize, BorshSerialize)]
enum PropType {
    Constitution,
    Veto,
    HouseDismiss(HouseType),
}

#[derive(BorshDeserialize, BorshSerialize)]
enum HouseType {
    HouseOfMerit,
    CouncilOfAdvisors,
    TransparencyCommission,
}

#[derive(BorshDeserialize, BorshSerialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Consent {
    /// percent of total stake voting required to pass a proposal.
    pub quorum: u8,
    /// #yes votes threshold as percent value (eg 12 = 12%)
    pub threshold: u8,
    // TODO: min amount of accounts
}

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Proof {
    // TODO need to finish it.
    /// total NEAR balance an account staked
    pub total_credits: u128,
}

/// Simple vote: user uses his all power to vote for a single option.
#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub enum SimpleVote {
    Abstain,
    No,
    Yes,
}

/// Aggregated Vote: user can split his vote credits into multiple options. Useful if an
/// account votes on behalve on multiple other entites (eg, asset manager, custdies).
#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct AggregateVote {
    // TODO: maybe we can make some fields optional to make the UX simpler
    pub abstain: u128,
    pub no: u128,
    pub yes: u128,
}

impl SimpleVote {
    pub fn to_aggregated(self, credits: u128) -> AggregateVote {
        let mut av = AggregateVote {
            abstain: 0,
            no: 0,
            yes: 0,
        };
        match self {
            Self::Abstain => av.abstain = credits,
            Self::No => av.no = credits,
            Self::Yes => av.yes = credits,
        };
        av
    }
}
