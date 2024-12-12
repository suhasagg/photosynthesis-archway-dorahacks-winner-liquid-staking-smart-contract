// src/msg.rs

use cosmwasm_std::{Uint128};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use cosmwasm_schema::cw_serde;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub liquid_staking_interval: u64,
    pub arch_liquid_stake_interval: u64,
    pub redemption_rate_query_interval: u64,
    pub rewards_withdrawal_interval: u64,
    pub redemption_interval_threshold: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsg {}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ContractRewardSummary {
    pub contract_address: String,
    pub pending_rewards: Uint128,
    pub deposit_pending: Uint128,
    pub deposit_completed: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Distribution {
    pub liquidity_address: String,
    pub amount: Uint128,
}

#[cw_serde]
pub enum ExecuteMsg {
    SetContractMetadata {
        contract_address: String,
        rewards_address: String,
        liquidity_provider_address: String,
        redemption_address: String,
        minimum_reward_amount: Uint128,
        maximum_reward_amount: Uint128,
    },
    AddStake {
        amount: Uint128,
    },
    UpdateReward {
        rewards_address: String,
        amount: Uint128,
    },
    BulkUpdateRewards {
        updates: Vec<RewardUpdate>,
    },
    ResetAllCompletedDepositRecords {},
    ResetStakeRatios {},
    DistributeLiquidity {},
    EmitLiquidStakeEvent {
        total_liquid_stake: Uint128,
        stuarch_obtained: Uint128,
        tx_hash: String,
    },
    EmitDistributeLiquidityEvent {
        distributions: Vec<Distribution>,
    },
    DistributeRedeemTokens {},
    ResetRedemptionRatios {},
    SetRedeemTokens {
        amount: Uint128,
        contract_address: String,
    },
    SubtractFromTotalLiquidStake {
        amount: Uint128,
    },
    CronJob {},
}

#[cw_serde]
pub enum QueryMsg {
    GetConfig {},
    GetTotalLiquidStakeQuery {},
    GetDepositRecords { contract: String },
    GetStakeRatio { contract: String },
    GetAllStakeRatios {},
    GetAllRedemptionRatios {},
    GetReward { rewards_address: String },
    GetRedeemTokens { contract: String },
    GetContractStake { contract: String },
    GetContractMetadata { contract: String },
    GetAllContracts {},
    /// Returns the reward summary for each contract and cumulative totals
    GetRewardSummaries {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RewardSummariesResponse {
    pub contract_summaries: Vec<ContractRewardSummary>,
    pub total_pending_rewards: Uint128,
    pub total_deposit_pending: Uint128,
    pub total_deposit_completed: Uint128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct RewardUpdate {
    pub contract_address: String,
    pub amount: Uint128,
}
