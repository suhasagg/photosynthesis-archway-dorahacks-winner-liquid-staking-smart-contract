// src/state.rs

use cosmwasm_std::{Addr, Decimal, Uint128};
use cw_storage_plus::{Item, Map};
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

// Configuration parameters
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr,
    pub liquid_staking_interval: u64,
    pub arch_liquid_stake_interval: u64,
    pub redemption_rate_query_interval: u64,
    pub rewards_withdrawal_interval: u64,
    pub redemption_interval_threshold: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ContractMetadata {
    pub rewards_address: String,
    pub liquidity_provider_address: String,
    pub minimum_reward_amount: Uint128,
    pub maximum_reward_amount: Uint128,
    pub redemption_address: String,
}

// Define DepositRecord with all necessary fields
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct DepositRecord {
    pub id: u64,
    pub contract_address: Addr,
    pub amount: Uint128,
    pub status: String, // "pending" or "completed"
    pub timestamp: u64,
    pub block_height: u64,
}

// Storage Items
pub const CONFIG: Item<Config> = Item::new("config");
pub const LAST_PROCESSING_TIMES: Map<&str, u64> = Map::new("last_processing_times");
pub const DEPOSIT_RECORDS: Map<&Addr, Vec<DepositRecord>> = Map::new("deposit_records");
pub const TOTAL_LIQUID_STAKE: Item<Uint128> = Item::new("total_liquid_stake");
pub const CONTRACT_STAKES: Map<&Addr, Uint128> = Map::new("contract_stakes");
pub const STAKE_RATIOS: Map<&Addr, Decimal> = Map::new("stake_ratios");
pub const REDEEM_TOKENS: Map<&Addr, Uint128> = Map::new("redeem_tokens");
pub const REDEEM_TOKEN_RATIOS: Map<&Addr, Decimal> = Map::new("redeem_token_ratios");
pub const CONTRACT_METADATA: Map<&Addr, ContractMetadata> = Map::new("contract_metadata");
pub const CONTRACT_REWARDS: Map<&Addr, Uint128> = Map::new("contract_rewards");
pub const NEXT_DEPOSIT_RECORD_ID: Item<u64> = Item::new("next_deposit_record_id");
pub const REDEMPTION_RECORDS: Map<&Addr, Uint128> = Map::new("redemption_records");
pub const REDEMPTION_TOKEN_RATIOS: Map<&Addr, Decimal> = Map::new("redemption_token_ratios");
pub const CALLBACK_INTERVAL_BLOCKS: u64 = 5;
pub const CALLBACK_JOB_ID: u64 = 1;

