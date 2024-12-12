use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Invalid reward amount range")]
    InvalidRewardAmountRange {},

    #[error("Contract not found: {contract_address}")]
    ContractNotFound { contract_address: String },

    #[error("No redemption records found")]
    NoRedemptionRecords {},

    #[error("Unsupported query")]
    UnsupportedQuery {},

    #[error("Invalid funds sent")]
    InvalidFunds {},

    #[error("Insufficient funds to request callback")]
    InsufficientFunds {},

    #[error("Serialization error")]
    SerializationError {},
}

