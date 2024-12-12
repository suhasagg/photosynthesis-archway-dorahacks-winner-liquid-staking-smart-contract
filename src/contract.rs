// src/contract.rs
//
// This file defines a CosmWasm smart contract that manages staking, liquidity, and rewards distribution
// in a modular and flexible way. It uses various storage maps and items (through cw_storage_plus) 
// to track different aspects of the contract's state, such as configuration data, contract metadata,
// staking information, redemption tokens, and reward distributions. The contract also supports 
// administrative operations (restricted to the owner) and handles periodic tasks via a "cron job"-like 
// mechanism that triggers actions based on elapsed time intervals.
//
// High-Level Responsibilities of this Contract:
// 1. Configuration Management: Store and update contract configurations, including intervals for 
//    various operations (liquid staking, redemption queries, etc.).
// 2. Contract Metadata: Keep track of per-contract metadata that defines reward distribution 
//    constraints (minimum/maximum reward amounts) and associated addresses (e.g., reward addresses).
// 3. Reward Management: Update rewards for contracts, bulk-update multiple contracts' rewards at once,
//    and ensure rewards are clamped within specified ranges.
// 4. Staking & Liquidity: Add stakes to contracts, convert "pending" stakes into "completed" ones, 
//    distribute liquidity based on completed stakes, and handle redemption tokens associated with 
//    certain contracts.
// 5. Periodic Tasks (Cron Jobs): Execute certain actions periodically by comparing the current block time 
//    against recorded timestamps, ensuring tasks only run if the required interval has passed. Implementation via callback support also available.
// 6. Querying Capabilities: Provide detailed queries that allow users (and other contracts or frontends) 
//    to retrieve contract configurations, stake ratios, redemption ratios, pending/completed deposit 
//    records, and reward summaries.

// Imports required from the CosmWasm standard library and other crates.
use cosmwasm_std::{
    entry_point, to_json_binary, Addr, Binary, Decimal, Deps, DepsMut, Env, Event, MessageInfo,
    Order, Response, StdError, StdResult, Storage, Timestamp, Uint128, to_binary, Api
};    
use cw_storage_plus::{Item, Map};
use std::collections::HashMap;

use crate::error::ContractError;
use crate::msg::{
    Distribution, ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg, RewardUpdate, RewardSummariesResponse, ContractRewardSummary
};
use crate::state::{
    Config, ContractMetadata, DepositRecord, CONFIG, CONTRACT_METADATA, CONTRACT_REWARDS,
    CONTRACT_STAKES, DEPOSIT_RECORDS, LAST_PROCESSING_TIMES, NEXT_DEPOSIT_RECORD_ID,
    REDEEM_TOKEN_RATIOS, REDEEM_TOKENS, STAKE_RATIOS, TOTAL_LIQUID_STAKE,
    REDEMPTION_RECORDS,
};

// Constants for keys used to track when certain periodic tasks last ran. These keys are used
// in the LAST_PROCESSING_TIMES map to store timestamps.
const LAST_LIQUID_STAKING_DAPP_REWARDS_TIME_KEY: &str = "last_liquid_staking_dapp_rewards_time";
const LAST_ARCH_LIQUID_STAKE_INTERVAL_TIME_KEY: &str = "last_arch_liquid_stake_interval_time";
const LAST_REDEMPTION_RATE_QUERY_TIME_KEY: &str = "last_redemption_rate_query_time";
const LAST_REWARDS_WITHDRAWAL_TIME_KEY: &str = "last_rewards_withdrawal_time";

// COMPLETED_STAKES: Tracks how much stake each contract has completed (fully processed and recognized).
// Uses contract address as key and a Uint128 for the completed stake amount.
pub static COMPLETED_STAKES: Map<&Addr, Uint128> = Map::new("completed_stakes");

/// The `instantiate` entry point is called exactly once when the contract is first deployed.
/// It sets up initial configuration values and state items.
///
/// Arguments:
/// - deps: mutable dependencies, includes storage
/// - env: environment info (e.g., block time, height)
/// - info: transaction info (e.g., sender address)
/// - msg: the InstantiateMsg containing initial configuration parameters
#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    // Build the initial config from the instantiation message. The owner is set to the sender.
    let config = Config {
        owner: info.sender.clone(),
        liquid_staking_interval: msg.liquid_staking_interval,
        arch_liquid_stake_interval: msg.arch_liquid_stake_interval,
        redemption_rate_query_interval: msg.redemption_rate_query_interval,
        rewards_withdrawal_interval: msg.rewards_withdrawal_interval,
        redemption_interval_threshold: msg.redemption_interval_threshold,
    };

    // Save the configuration to storage for persistent access.
    CONFIG.save(deps.storage, &config)?;

    // Initialize last processing times for various cron tasks to the current block time.
    let now = env.block.time.seconds();
    LAST_PROCESSING_TIMES.save(deps.storage, LAST_LIQUID_STAKING_DAPP_REWARDS_TIME_KEY, &now)?;
    LAST_PROCESSING_TIMES.save(deps.storage, LAST_ARCH_LIQUID_STAKE_INTERVAL_TIME_KEY, &now)?;
    LAST_PROCESSING_TIMES.save(deps.storage, LAST_REDEMPTION_RATE_QUERY_TIME_KEY, &now)?;
    LAST_PROCESSING_TIMES.save(deps.storage, LAST_REWARDS_WITHDRAWAL_TIME_KEY, &now)?;

    // Initialize total liquid stake as zero at the start.
    TOTAL_LIQUID_STAKE.save(deps.storage, &Uint128::zero())?;

    // Set the next deposit record ID to start at 1, ensuring a unique ID counter for deposit records.
    NEXT_DEPOSIT_RECORD_ID.save(deps.storage, &1u64)?;

    // Emit an event indicating that the contract has been instantiated successfully.
    let event = Event::new("instantiate")
        .add_attribute("action", "instantiate")
        .add_attribute("owner", info.sender.to_string())
        .add_attribute("liquid_staking_interval", msg.liquid_staking_interval.to_string())
        .add_attribute("arch_liquid_stake_interval", msg.arch_liquid_stake_interval.to_string())
        .add_attribute("redemption_rate_query_interval", msg.redemption_rate_query_interval.to_string())
        .add_attribute("rewards_withdrawal_interval", msg.rewards_withdrawal_interval.to_string())
        .add_attribute("redemption_interval_threshold", msg.redemption_interval_threshold.to_string())
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("timestamp", now.to_string());

    Ok(Response::new()
        .add_event(event)
        .add_attribute("method", "instantiate")
        .add_attribute("owner", info.sender))
}

/// The `execute` entry point handles mutable operations. Based on the `ExecuteMsg` variant received,
/// it routes to different handler functions. Only the contract owner can call certain administrative actions.
#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    // Match on the message variant to determine which action to take.
    match msg {
        ExecuteMsg::CronJob {} => execute_cron_job(deps, env),

        ExecuteMsg::SetContractMetadata {
            contract_address,
            rewards_address,
            liquidity_provider_address,
            redemption_address,
            minimum_reward_amount,
            maximum_reward_amount,
        } => execute_set_contract_metadata(
            deps,
            info,
            contract_address,
            rewards_address,
            liquidity_provider_address,
            minimum_reward_amount,
            maximum_reward_amount,
            redemption_address,
            env,
        ),

        ExecuteMsg::AddStake { amount } => execute_add_stake(deps, info, amount, env),

        ExecuteMsg::UpdateReward { rewards_address, amount } => {
            execute_update_reward(deps, info, rewards_address, amount, env)
        }

        ExecuteMsg::BulkUpdateRewards { updates } => {
            execute_bulk_update_rewards(deps, info, updates, env)
        }

        ExecuteMsg::ResetAllCompletedDepositRecords {} => {
            execute_reset_all_completed_deposit_records(deps, info, env)
        }

        ExecuteMsg::ResetStakeRatios {} => execute_reset_stake_ratios(deps, info, env),

        ExecuteMsg::DistributeLiquidity {} => {
            execute_distribute_liquidity(deps, env, info)
        }

        ExecuteMsg::EmitLiquidStakeEvent {
            total_liquid_stake,
            stuarch_obtained,
            tx_hash,
        } => emit_liquid_stake_event(
            deps,
            env,
            info,
            total_liquid_stake,
            stuarch_obtained,
            tx_hash,
        ),

        ExecuteMsg::EmitDistributeLiquidityEvent { distributions } => {
            emit_distribute_liquidity_event(deps, env, info, distributions)
        }

        ExecuteMsg::DistributeRedeemTokens {} => {
            execute_distribute_redeem_tokens(deps, env, info)
        }

        ExecuteMsg::ResetRedemptionRatios {} => {
            execute_reset_redemption_ratios(deps, env, info)
        }

        ExecuteMsg::SetRedeemTokens {
            amount,
            contract_address,
        } => execute_set_redeem_tokens(deps, info, amount, contract_address, env),

        ExecuteMsg::SubtractFromTotalLiquidStake { amount } => {
            execute_subtract_from_total_liquid_stake(deps, env, info, amount)
        }
    }
}

/// Execute function to update a specific contract's reward. Only the owner can do this.
/// This ensures that only authorized users can modify reward amounts for contracts.
fn execute_update_reward(
    deps: DepsMut,
    info: MessageInfo,
    contract_address: String,
    amount: Uint128,
    env: Env,
) -> Result<Response, ContractError> {
    // Authorization check against contract owner.
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Validate the contract address to ensure it's a properly formed bech32 address.
    let rewards_addr = deps.api.addr_validate(&contract_address)?;
    add_reward_to_contract(deps.storage, &rewards_addr, amount, &env)?;

    // Emit an event indicating the reward was successfully updated.
    let event = Event::new("update_reward")
        .add_attribute("action", "execute_update_reward")
        .add_attribute("sender", info.sender.to_string())
        .add_attribute("contract_address", rewards_addr.clone())
        .add_attribute("reward_amount", amount.to_string())
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("timestamp", env.block.time.seconds().to_string());

    Ok(Response::new()
        .add_event(event)
        .add_attribute("method", "update_reward"))
}

/// Update rewards for multiple contracts at once, reducing transaction overhead for the owner.
/// The owner can set rewards for multiple contracts in a single call.
fn execute_bulk_update_rewards(
    deps: DepsMut,
    info: MessageInfo,
    updates: Vec<RewardUpdate>,
    env: Env,
) -> Result<Response, ContractError> {
    // Only the owner can bulk update rewards.
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut res = Response::new();

    // Process each update in the provided vector of updates.
    for update in &updates {
        let contract_addr = deps.api.addr_validate(&update.contract_address)?;
        let event = add_reward_to_contract(deps.storage, &contract_addr, update.amount, &env)?;

        // Emit events for each contract updated in bulk.
        let update_event = Event::new("update_reward")
            .add_attribute("action", "execute_bulk_update_rewards")
            .add_attribute("sender", info.sender.to_string())
            .add_attribute("contract_address", update.contract_address.clone())
            .add_attribute("reward_amount", update.amount.to_string())
            .add_attribute("block_height", env.block.height.to_string())
            .add_attribute("timestamp", env.block.time.seconds().to_string());

        // Add both the event from add_reward_to_contract and the update_event to the response.
        res = res.add_event(event);
        res = res.add_event(update_event);
    }

    // Indicate the method used in the response attributes.
    res = res.add_attribute("method", "bulk_update_rewards");

    Ok(res)
}

/// Adds a specified reward amount to a contract's reward balance. This is a helper function used by 
/// execute_update_reward and execute_bulk_update_rewards to actually modify storage.
///
/// Arguments:
/// - storage: state storage for reading/writing contract state
/// - rewards_addr: address of the contract whose rewards are being updated
/// - amount: the amount of rewards to add
/// - env: environment for block info (for event attributes)
fn add_reward_to_contract(
    storage: &mut dyn Storage,
    rewards_addr: &Addr,
    amount: Uint128,
    env: &Env,
) -> Result<Event, ContractError> {
    // Load the current reward amount; default to zero if not set.
    let current_reward = CONTRACT_REWARDS
        .may_load(storage, rewards_addr)?
        .unwrap_or_default();
    let new_reward = current_reward + amount;

    // Save the updated reward amount.
    CONTRACT_REWARDS.save(storage, rewards_addr, &new_reward)?;

    // Emit an event indicating successful addition of rewards to the contract.
    let event = Event::new("add_reward_to_contract")
        .add_attribute("action", "add_reward_to_contract")
        .add_attribute("contract_address", rewards_addr.to_string())
        .add_attribute("reward_amount_added", amount.to_string())
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("timestamp", env.block.time.seconds().to_string());
    Ok(event)
}

/// Execute a cron job to process tasks that are due based on the elapsed time since their last run.
/// Tasks include handling liquid staking rewards, arch liquid stake intervals, and redemption rate queries.
fn execute_cron_job(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let mut res = Response::new();
    res = res.add_attribute("method", "execute_cron_job");

    let config = CONFIG.load(deps.storage)?;
    let now = env.block.time.seconds();

    // If enough time has passed since the last liquid staking DApp rewards, process them.
    if should_process_task(
        deps.storage,
        LAST_LIQUID_STAKING_DAPP_REWARDS_TIME_KEY,
        config.liquid_staking_interval,
        now,
    )? {
        let task_res = handle_liquid_staking_dapp_rewards(deps.storage, &env)?;
        LAST_PROCESSING_TIMES.save(
            deps.storage,
            LAST_LIQUID_STAKING_DAPP_REWARDS_TIME_KEY,
            &now,
        )?;
        // Add attributes and events from the task result to the main response.
        res = res.add_attributes(task_res.attributes);
        res = res.add_events(task_res.events);
        res = res.add_attribute("task", "liquid_staking_dapp_rewards");
    }

    // If enough time has passed for arch liquid stake intervals, handle that.
    if should_process_task(
        deps.storage,
        LAST_ARCH_LIQUID_STAKE_INTERVAL_TIME_KEY,
        config.arch_liquid_stake_interval,
        now,
    )? {
        let task_res = handle_arch_liquid_stake_interval(deps.storage, &env)?;
        LAST_PROCESSING_TIMES.save(
            deps.storage,
            LAST_ARCH_LIQUID_STAKE_INTERVAL_TIME_KEY,
            &now,
        )?;
        res = res.add_attributes(task_res.attributes);
        res = res.add_events(task_res.events);
        res = res.add_attribute("task", "arch_liquid_stake_interval");
    }

    // If enough time has passed for redemption rate queries, handle that as well.
    if should_process_task(
        deps.storage,
        LAST_REDEMPTION_RATE_QUERY_TIME_KEY,
        config.redemption_rate_query_interval,
        now,
    )? {
        let task_res = handle_redemption_rate_query(deps.storage, &config, env.clone())?;
        LAST_PROCESSING_TIMES.save(
            deps.storage,
            LAST_REDEMPTION_RATE_QUERY_TIME_KEY,
            &now,
        )?;
        res = res.add_attributes(task_res.attributes);
        res = res.add_events(task_res.events);
        res = res.add_attribute("task", "redemption_rate_query");
    }

    // Emit a final event summarizing the cron job execution.
    let event = Event::new("cron_job_executed")
        .add_attribute("action", "execute_cron_job")
        .add_attribute("timestamp", now.to_string())
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("processed_tasks", format!("{:?}", res.attributes));

    res = res.add_event(event);

    Ok(res)
}

/// Set metadata for a given contract, controlling min/max reward amounts and 
/// associated addresses for rewards and liquidity. Only the owner can call this.
fn execute_set_contract_metadata(
    deps: DepsMut,
    info: MessageInfo,
    contract_address: String,
    rewards_address: String,
    liquidity_provider_address: String,
    minimum_reward_amount: Uint128,
    maximum_reward_amount: Uint128,
    redemption_address: String,
    env: Env,
) -> Result<Response, ContractError> {
    // Authorization: only the owner can set metadata.
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Validate reward amount range.
    if maximum_reward_amount < minimum_reward_amount {
        return Err(ContractError::InvalidRewardAmountRange {});
    }

    // Create and save the ContractMetadata struct.
    let metadata = ContractMetadata {
        rewards_address: rewards_address.clone(),
        liquidity_provider_address: liquidity_provider_address.clone(),
        minimum_reward_amount,
        maximum_reward_amount,
        redemption_address: redemption_address.clone(),
    };

    CONTRACT_METADATA.save(
        deps.storage,
        &deps.api.addr_validate(&contract_address)?,
        &metadata,
    )?;

    // Emit an event indicating successful metadata setting.
    let event = Event::new("set_contract_metadata")
        .add_attribute("action", "execute_set_contract_metadata")
        .add_attribute("sender", info.sender.to_string())
        .add_attribute("contract_address", contract_address.clone())
        .add_attribute("rewards_address", rewards_address.clone())
        .add_attribute("liquidity_provider_address", liquidity_provider_address.clone())
        .add_attribute("minimum_reward_amount", minimum_reward_amount.to_string())
        .add_attribute("maximum_reward_amount", maximum_reward_amount.to_string())
        .add_attribute("redemption_address", redemption_address.clone())
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("timestamp", env.block.time.seconds().to_string());

    Ok(Response::new()
        .add_event(event)
        .add_attribute("method", "set_contract_metadata")
        .add_attribute("contract", contract_address))
}

/// Add stake for the sender. This increases the CONTRACT_STAKES mapping for the caller by the given amount.
fn execute_add_stake(
    deps: DepsMut,
    info: MessageInfo,
    amount: Uint128,
    env: Env,
) -> Result<Response, ContractError> {
    // Update the stake in storage.
    add_contract_stake(deps.storage, &info.sender, amount)?;

    // Emit an event indicating the stake addition.
    let event = Event::new("add_stake")
        .add_attribute("action", "execute_add_stake")
        .add_attribute("sender", info.sender.to_string())
        .add_attribute("stake_amount", amount.to_string())
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("timestamp", env.block.time.seconds().to_string());

    Ok(Response::new()
        .add_event(event)
        .add_attribute("method", "add_stake")
        .add_attribute("contract", info.sender.to_string())
        .add_attribute("amount", amount.to_string()))
}

/// Reset all completed deposit records to pending for all contracts. Only the owner can do this.
/// This might be used for testing or emergency measures.
fn execute_reset_all_completed_deposit_records(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
) -> Result<Response, ContractError> {
    // Authorization check.
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Perform the reset operation in storage.
    reset_all_completed_deposit_records(deps.storage)?;

    // Emit an event indicating the operation.
    let event = Event::new("reset_all_completed_deposit_records")
        .add_attribute("action", "execute_reset_all_completed_deposit_records")
        .add_attribute("sender", info.sender.to_string())
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("timestamp", env.block.time.seconds().to_string());

    Ok(Response::new()
        .add_event(event)
        .add_attribute("method", "reset_all_completed_deposit_records"))
}

/// Reset all redemption ratios to a clean state. Only the owner can perform this action.
fn execute_reset_redemption_ratios(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    // Owner authorization.
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Clear the REDEEM_TOKEN_RATIOS map.
    reset_redemption_ratios(deps.storage)?;

    // Emit an event indicating the ratios have been reset.
    let event = Event::new("reset_redemption_ratios")
        .add_attribute("action", "execute_reset_redemption_ratios")
        .add_attribute("sender", info.sender.to_string())
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("timestamp", env.block.time.seconds().to_string());

    Ok(Response::new()
        .add_event(event)
        .add_attribute("method", "reset_redemption_ratios"))
}

/// Helper function to remove all entries from REDEEM_TOKEN_RATIOS, restoring it to an empty state.
fn reset_redemption_ratios(storage: &mut dyn Storage) -> Result<(), ContractError> {
    let keys: Vec<Addr> = REDEEM_TOKEN_RATIOS
        .keys(storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<Addr>>>()?;

    for key in keys {
        REDEEM_TOKEN_RATIOS.remove(storage, &key);
    }

    Ok(())
}

/// Reset all stake ratios and completed stakes to zero, restoring initial conditions for stake distribution.
fn execute_reset_stake_ratios(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
) -> Result<Response, ContractError> {
    // Only owner can reset stake ratios.
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Clear STAKE_RATIOS and reset COMPLETED_STAKES.
    reset_stake_ratios(deps.storage)?;

    // Emit an event indicating the reset action.
    let event = Event::new("reset_stake_ratios")
        .add_attribute("action", "execute_reset_stake_ratios")
        .add_attribute("sender", info.sender.to_string())
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("timestamp", env.block.time.seconds().to_string());

    Ok(Response::new()
        .add_event(event)
        .add_attribute("method", "reset_stake_ratios"))
}

/// Adds a given stake amount to the CONTRACT_STAKES map for a specific contract address.
/// This is a fundamental operation called by functions that need to track added stakes.
fn add_contract_stake(
    storage: &mut dyn Storage,
    contract_addr: &Addr,
    amount: Uint128,
) -> Result<(), ContractError> {
    let current_stake = CONTRACT_STAKES
        .may_load(storage, contract_addr)?
        .unwrap_or_default();
    let new_stake = current_stake + amount;
    CONTRACT_STAKES.save(storage, contract_addr, &new_stake)?;
    Ok(())
}

/// Checks if a given task should be processed now by comparing the current time with the last processed time
/// and ensuring the specified interval has elapsed.
fn should_process_task(
    storage: &dyn Storage,
    key: &str,
    interval_in_seconds: u64,
    current_time: u64,
) -> Result<bool, ContractError> {
    let last_time = LAST_PROCESSING_TIMES.load(storage, key)?;
    Ok(current_time >= last_time + interval_in_seconds)
}

/// Retrieves all contracts that have associated metadata stored in CONTRACT_METADATA.
fn get_all_contracts(storage: &dyn Storage) -> Result<Vec<Addr>, ContractError> {
    let contracts: Vec<Addr> = CONTRACT_METADATA
        .keys(storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<Addr>>>()?;

    Ok(contracts)
}

/// Handle logic for liquid staking DApp rewards triggered by the cron job. It computes how much reward
/// each contract gets and converts pending rewards into deposit records if they exceed the minimum 
/// reward amount.
fn handle_liquid_staking_dapp_rewards(
    storage: &mut dyn Storage,
    env: &Env,
) -> Result<Response, ContractError> {
    let mut res = Response::new();

    let reward_map = get_cumulative_reward_amount(storage)?;

    // Process each contract: check its metadata, determine final reward amount, and create deposit records.
    let contracts = get_all_contracts(storage)?;
    for contract in contracts {
        let metadata = CONTRACT_METADATA.may_load(storage, &contract)?;
        if let Some(meta) = metadata {
            let rewards_addr = Addr::unchecked(&meta.rewards_address);
            let raw_amount = reward_map
                .get(&contract)
                .cloned()
                .unwrap_or(Uint128::zero());

            // Clamp the reward to be within [minimum_reward_amount, maximum_reward_amount].
            let amount = if raw_amount > meta.maximum_reward_amount {
                meta.maximum_reward_amount
            } else {
                raw_amount
            };

            // Only proceed if the amount meets the minimum reward criteria.
            if amount >= meta.minimum_reward_amount {
                // Create a deposit record indicating a pending stake due to these rewards.
                let record = create_contract_liquid_stake_deposit_record(
                    storage,
                    &contract,
                    amount,
                    &rewards_addr,
                    env,
                );

                // Append this record to the DEPOSIT_RECORDS for the contract.
                let mut records = DEPOSIT_RECORDS
                    .may_load(storage, &contract)?
                    .unwrap_or_default();
                records.push(record.clone());
                DEPOSIT_RECORDS.save(storage, &contract, &records)?;

                // Increase the contract's stake and reset its CONTRACT_REWARDS to zero since rewards are now accounted for.
                add_contract_stake(storage, &contract, amount)?;
                CONTRACT_REWARDS.save(storage, &contract, &Uint128::zero())?;

                // Emit an event indicating the processing of liquid staking rewards for this contract.
                let event = Event::new("handle_liquid_staking_dapp_rewards")
                    .add_attribute("contract_address", contract.to_string())
                    .add_attribute("pending_deposit_record_amount", amount.to_string())
                    .add_attribute("reward_address", rewards_addr.to_string())
                    .add_attribute("deposit_record_id", record.id.to_string())
                    .add_attribute("deposit_record_status", record.status.clone())
                    .add_attribute("block_height", env.block.height.to_string())
                    .add_attribute("timestamp", env.block.time.seconds().to_string());

                res = res.add_event(event);
            }
        }
    }

    Ok(res)
}

/// Handle the arch liquid stake interval triggered by cron jobs. It aggregates pending deposits into 
/// completed stakes and updates the total liquid stake.
fn handle_arch_liquid_stake_interval(
    storage: &mut dyn Storage,
    env: &Env,
) -> Result<Response, ContractError> {
    let mut res = Response::new();

    // Update total liquid stake by processing pending deposit records.
    let total_stake_res = get_total_liquid_stake(storage, env)?;
    res = res.add_events(total_stake_res.events);
    res = res.add_attributes(total_stake_res.attributes);

    // Emit an event indicating the handling of arch liquid stake interval.
    let event = Event::new("handle_arch_liquid_stake_interval")
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("timestamp", env.block.time.seconds().to_string());

    res = res.add_event(event);

    Ok(res)
}

/// Handle redemption rate queries if implemented. Currently stubbed out, but in a production
/// environment, this would fetch redemption rates and log redemptions based on thresholds on chain, redemption rate at which tokens were redeemed
fn handle_redemption_rate_query(
    _storage: &mut dyn Storage,
    _config: &Config,
    _env: Env,
) -> Result<Response, ContractError> {
    // Stub: For now, do nothing and return an empty response.
    Ok(Response::new())
}

/// Compute cumulative reward amounts across all contracts from CONTRACT_REWARDS.
fn get_cumulative_reward_amount(
    storage: &dyn Storage,
) -> Result<HashMap<Addr, Uint128>, ContractError> {
    let mut reward_map = HashMap::new();

    // Retrieve all contract addresses that have reward entries.
    let rewards_addresses: Vec<Addr> = CONTRACT_REWARDS
        .keys(storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<_>>>()?;

    // Accumulate rewards in a HashMap keyed by contract address.
    for rewards_addr in rewards_addresses {
        let reward_amount = CONTRACT_REWARDS
            .may_load(storage, &rewards_addr)?
            .unwrap_or_default();

        if !reward_amount.is_zero() {
            reward_map.insert(rewards_addr.clone(), reward_amount);
        }
    }

    Ok(reward_map)
}

/// Create a pending deposit record for a contract representing a future staking action.
/// The record starts as "pending" and will later be marked "completed" once processed.
fn create_contract_liquid_stake_deposit_record(
    storage: &mut dyn Storage,
    contract_addr: &Addr,
    amount: Uint128,
    _reward_address: &Addr,
    env: &Env,
) -> DepositRecord {
    // Increment and retrieve the next deposit record ID.
    let next_id = NEXT_DEPOSIT_RECORD_ID
        .update(storage, |id| -> StdResult<u64> { Ok(id + 1) })
        .unwrap_or(1);

    DepositRecord {
        id: next_id,
        contract_address: contract_addr.clone(),
        amount,
        status: "pending".to_string(),
        timestamp: env.block.time.seconds(),
        block_height: env.block.height,
    }
}

/// Distribute liquidity tokens among contracts based on their proportion of completed stakes.
/// Contracts with higher completed stakes receive a larger share of liquidity tokens.
fn distribute_liquidity(
    storage: &mut dyn Storage,
    env: &Env,
) -> Result<Response, ContractError> {
    let mut res = Response::new();

    // Get the total liquid stake that is recognized.
    let total_liquid_stake = TOTAL_LIQUID_STAKE.load(storage)?;
    let liquidity_amount = total_liquid_stake.u128();

    let contracts = get_all_contracts(storage)?;
    let mut cumulative_stakes = HashMap::new();
    let mut total_stake = Uint128::zero();

    // Compute the total completed stake across all contracts from COMPLETED_STAKES.
    for contract in &contracts {
        let contract_stake = COMPLETED_STAKES
            .may_load(storage, contract)?
            .unwrap_or_default();
        cumulative_stakes.insert(contract.clone(), contract_stake);
        total_stake += contract_stake;
    }

    // If no stake is present, nothing to distribute.
    if total_stake.is_zero() {
        return Ok(res);
    }

    // Distribute liquidity proportionally to each contract based on stake ratio.
    for (contract_addr, contract_stake) in cumulative_stakes {
        let stake_proportion = Decimal::from_ratio(contract_stake.u128(), total_stake.u128());
        let liquidity_tokens_amount =
            Uint128::from((stake_proportion * Uint128::from(liquidity_amount)).u128());

        // Save this ratio in STAKE_RATIOS for future reference.
        STAKE_RATIOS.save(storage, &contract_addr, &stake_proportion)?;

        // Emit an event detailing how much liquidity this contract received.
        let distribute_event = Event::new("distribute_liquidity")
            .add_attribute("contract_address", contract_addr.to_string())
            .add_attribute("stake_proportion", stake_proportion.to_string())
            .add_attribute("liquidity_tokens_amount", liquidity_tokens_amount.to_string())
            .add_attribute("block_height", env.block.height.to_string())
            .add_attribute("timestamp", env.block.time.seconds().to_string());

        res = res.add_event(distribute_event);
    }

    Ok(res)
}

/// Entry point to trigger liquidity distribution by the owner. Calls the `distribute_liquidity` function
/// and emits a summary event.
fn execute_distribute_liquidity(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    // Owner authorization check.
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut res = Response::new();

    // Perform liquidity distribution.
    let distribute_res = distribute_liquidity(deps.storage, &env)?;
    res = res.add_events(distribute_res.events);
    res = res.add_attributes(distribute_res.attributes);

    // Emit an event summarizing the liquidity distribution action.
    let event = Event::new("execute_distribute_liquidity")
        .add_attribute("action", "distribute_liquidity")
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("timestamp", env.block.time.seconds().to_string());

    res = res.add_event(event);

    Ok(res)
}

/// Allows the owner to set redeem tokens for a specified contract. Redeem tokens might represent
/// tokens to be claimed by the contract later.
fn execute_set_redeem_tokens(
    deps: DepsMut,
    info: MessageInfo,
    amount: Uint128,
    contract_address: String,
    env: Env,
) -> Result<Response, ContractError> {
    // Owner-only action.
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let validated_contract_address = deps.api.addr_validate(&contract_address)?;

    // Verify that the contract has metadata before setting redeem tokens.
    if !CONTRACT_METADATA.has(deps.storage, &validated_contract_address) {
        return Err(ContractError::ContractNotFound {
            contract_address: contract_address.clone(),
        });
    }

    // Update the redemption record for the contract by adding the specified amount.
    let current_amount = REDEMPTION_RECORDS
        .may_load(deps.storage, &validated_contract_address)?
        .unwrap_or_default();
    let new_amount = current_amount + amount;
    REDEMPTION_RECORDS.save(deps.storage, &validated_contract_address, &new_amount)?;

    // Emit an event indicating redeem tokens have been set.
    let event = Event::new("set_redeem_tokens")
        .add_attribute("action", "execute_set_redeem_tokens")
        .add_attribute("sender", info.sender.to_string())
        .add_attribute("contract_address", validated_contract_address.to_string())
        .add_attribute("redeem_amount", amount.to_string())
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("timestamp", env.block.time.seconds().to_string());

    Ok(Response::new()
        .add_event(event)
        .add_attribute("method", "set_redeem_tokens")
        .add_attribute("contract_address", validated_contract_address.to_string())
        .add_attribute("amount", amount.to_string()))
}

/// Distribute redeem tokens across all contracts that have pending redemption records. Only the owner can do this.
/// After computing redemption ratios, it resets the redemption records and emits distribution events.
fn execute_distribute_redeem_tokens(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    // Owner-only action.
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut res = Response::new();

    // Gather all contracts and check their redemption records.
    let contracts = get_all_contracts(deps.storage)?;
    let mut total_redeem_tokens = Uint128::zero();
    let mut redemption_records = HashMap::new();

    for contract_addr in contracts.iter() {
        let amount = REDEMPTION_RECORDS
            .may_load(deps.storage, contract_addr)?
            .unwrap_or_default();
        if !amount.is_zero() {
            redemption_records.insert(contract_addr.clone(), amount);
            total_redeem_tokens += amount;
        }
    }

    if total_redeem_tokens.is_zero() {
        // If no redemption records exist, return an error indicating no data to process.
        return Err(ContractError::NoRedemptionRecords {});
    }

    // Calculate redemption ratios for each contract and emit distribution events.
    for (contract_addr, amount) in redemption_records.iter() {
        let redemption_ratio = Decimal::from_ratio(amount.u128(), total_redeem_tokens.u128());
        REDEEM_TOKEN_RATIOS.save(deps.storage, contract_addr, &redemption_ratio)?;

        // Emit event indicating how many tokens this contract got.
        let event = Event::new("distribute_redeem_tokens")
            .add_attribute("contract_address", contract_addr.to_string())
            .add_attribute("redemption_ratio", redemption_ratio.to_string())
            .add_attribute("redeem_tokens_amount", amount.to_string())
            .add_attribute("block_height", env.block.height.to_string())
            .add_attribute("timestamp", env.block.time.seconds().to_string());

        res = res.add_event(event);

        // Reset the redemption record for this contract now that we've distributed tokens.
        REDEMPTION_RECORDS.save(deps.storage, contract_addr, &Uint128::zero())?;
    }

    // Summarize the redemption token distribution with a final event.
    let summary_event = Event::new("redeem_tokens_distributed")
        .add_attribute("total_redeem_tokens", total_redeem_tokens.to_string())
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("timestamp", env.block.time.seconds().to_string());

    res = res.add_event(summary_event);

    Ok(res)
}

/// Update total liquid stake by converting pending deposit records into completed ones. This may be triggered
/// by certain intervals to recognize stakes as completed and update COMPLETED_STAKES and TOTAL_LIQUID_STAKE.
fn get_total_liquid_stake(
    storage: &mut dyn Storage,
    env: &Env,
) -> Result<Response, ContractError> {
    let mut res = Response::new();

    // Load current total liquid stake.
    let mut total_liquid_stake = TOTAL_LIQUID_STAKE
        .may_load(storage)?
        .unwrap_or_default();

    let contracts = get_all_contracts(storage)?;

    // For each contract, check deposit records and finalize those that are still pending.
    for contract in contracts {
        let mut deposit_records = DEPOSIT_RECORDS
            .may_load(storage, &contract)?
            .unwrap_or_default();
        let mut updated_records = vec![];

        for mut record in deposit_records {
            if record.status == "pending" {
                // Convert from pending to completed and update the total liquid stake counter.
                total_liquid_stake += record.amount;
                record.status = "completed".to_string();

                // Update COMPLETED_STAKES to reflect that these stakes are now completed.
                let current_completed_stake = COMPLETED_STAKES
                    .may_load(storage, &contract)?
                    .unwrap_or_default();
                let new_completed_stake = current_completed_stake + record.amount;
                COMPLETED_STAKES.save(storage, &contract, &new_completed_stake)?;

                // Reduce the CONTRACT_STAKES by the completed amount.
                let current_contract_stake = CONTRACT_STAKES
                    .may_load(storage, &contract)?
                    .unwrap_or_default();
                let new_contract_stake = current_contract_stake
                    .checked_sub(record.amount)
                    .map_err(|e| ContractError::Std(StdError::Overflow { source: e }))?;
                CONTRACT_STAKES.save(storage, &contract, &new_contract_stake)?;

                // Emit an event per deposit record updated.
                let deposit_event = Event::new("deposit_record_updated")
                    .add_attribute("contract_address", contract.to_string())
                    .add_attribute("deposit_record_id", record.id.to_string())
                    .add_attribute("completed_deposit_record_amount", record.amount.to_string())
                    .add_attribute("deposit_record_status", record.status.clone())
                    .add_attribute("timestamp", env.block.time.seconds().to_string())
                    .add_attribute("block_height", env.block.height.to_string());

                res = res.add_event(deposit_event);
            }
            updated_records.push(record);
        }

        DEPOSIT_RECORDS.save(storage, &contract, &updated_records)?;
    }

    // Save the updated total liquid stake after processing all pending records.
    TOTAL_LIQUID_STAKE.save(storage, &total_liquid_stake)?;

    // Emit an event summarizing the new total liquid stake.
    let total_stake_event = Event::new("get_total_liquid_stake")
        .add_attribute("total_liquid_stake", total_liquid_stake.to_string())
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("timestamp", env.block.time.seconds().to_string());

    res = res.add_event(total_stake_event);

    Ok(res)
}

/// Reset all completed deposit records back to pending 
fn reset_all_completed_deposit_records(storage: &mut dyn Storage) -> Result<(), ContractError> {
    let contracts = get_all_contracts(storage)?;

    for contract in contracts {
        let deposit_records = DEPOSIT_RECORDS
            .may_load(storage, &contract)?
            .unwrap_or_default();

        // Only keep records that are not completed.
        let pending_records: Vec<DepositRecord> = deposit_records
            .into_iter()
            .filter(|record| record.status != "completed")
            .collect();

        DEPOSIT_RECORDS.save(storage, &contract, &pending_records)?;

       
        let _event = Event::new("reset_completed_deposit_records")
            .add_attribute("contract_address", contract.to_string())
            .add_attribute("remaining_records", pending_records.len().to_string());
    }

    Ok(())
}

/// A helper query to get the total currently recognized liquid stake without triggering any updates.
fn get_total_liquid_stake_query(
    deps: Deps,
) -> Result<Uint128, ContractError> {
    let total_completed_stake = TOTAL_LIQUID_STAKE
        .may_load(deps.storage)?
        .unwrap_or_default();

    Ok(total_completed_stake)
}

/// Allows the owner to subtract a specified amount from the TOTAL_LIQUID_STAKE
fn execute_subtract_from_total_liquid_stake(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
) -> Result<Response, ContractError> {
    // Owner-only operation.
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Load the current total liquid stake and subtract the given amount.
    let mut total_liquid_stake = TOTAL_LIQUID_STAKE.load(deps.storage)?;
    total_liquid_stake = total_liquid_stake
        .checked_sub(amount)
        .map_err(|e| ContractError::Std(StdError::Overflow { source: e }))?;

    TOTAL_LIQUID_STAKE.save(deps.storage, &total_liquid_stake)?;

    // Emit an event indicating the subtraction action.
    let event = Event::new("subtract_from_total_liquid_stake")
        .add_attribute("action", "execute_subtract_from_total_liquid_stake")
        .add_attribute("sender", info.sender.to_string())
        .add_attribute("amount_subtracted", amount.to_string())
        .add_attribute("new_total_liquid_stake", total_liquid_stake.to_string())
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("timestamp", env.block.time.seconds().to_string());

    Ok(Response::new()
        .add_event(event)
        .add_attribute("method", "subtract_from_total_liquid_stake"))
}

/// Emit a custom event associated with liquid staking activities. Only the owner can do this.
/// This is used for external integrations or logging important stake-related milestones.
fn emit_liquid_stake_event(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    total_liquid_stake: Uint128,
    stuarch_obtained: Uint128,
    tx_hash: String,
) -> Result<Response, ContractError> {
    // Owner check.
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    // Emit an event describing the liquid stake event and associated data.
    let event = Event::new("liquid_stake_event")
        .add_attribute("action", "liquid_stake_event")
        .add_attribute("total_liquid_stake", total_liquid_stake.to_string())
        .add_attribute("stuarch_obtained", stuarch_obtained.to_string())
        .add_attribute("tx_hash", tx_hash)
        .add_attribute("block_height", env.block.height.to_string())
        .add_attribute("timestamp", env.block.time.seconds().to_string());

    Ok(Response::new()
        .add_event(event)
        .add_attribute("method", "emit_liquid_stake_event"))
}

/// Emit an event representing the distribution of liquidity to multiple addresses. Only the owner can do this.
/// This allows the contract owner to log distribution actions that occur off-chain or from external triggers.
fn emit_distribute_liquidity_event(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    distributions: Vec<Distribution>,
) -> Result<Response, ContractError> {
    // Owner-only action.
    let config = CONFIG.load(deps.storage)?;
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut res = Response::new();
    res = res.add_attribute("method", "emit_distribute_liquidity_event");

    // Emit events for each distribution specified in the input vector.
    for distribution in distributions.iter() {
        let event = Event::new("distribute_liquidity_event")
            .add_attribute("action", "distribute_liquidity_event")
            .add_attribute("liquidity_address", distribution.liquidity_address.clone())
            .add_attribute("liquidity_amount", distribution.amount.to_string())
            .add_attribute("block_height", env.block.height.to_string())
            .add_attribute("timestamp", env.block.time.seconds().to_string());

        res = res.add_event(event);
    }

    Ok(res)
}

/// Reset all stake ratios and set COMPLETED_STAKES to zero for all contracts, effectively reverting
/// liquidity distribution calculations to an initial state.
fn reset_stake_ratios(storage: &mut dyn Storage) -> Result<(), ContractError> {
    let keys: Vec<Addr> = STAKE_RATIOS
        .keys(storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<Addr>>>()?;

    // Remove all stake ratio entries.
    for key in keys {
        STAKE_RATIOS.remove(storage, &key);
    }

    // Reset all COMPLETED_STAKES to zero.
    let completed_stake_keys: Vec<Addr> = COMPLETED_STAKES
        .keys(storage, None, None, Order::Ascending)
        .collect::<StdResult<Vec<Addr>>>()?;

    for key in completed_stake_keys {
        COMPLETED_STAKES.save(storage, &key, &Uint128::zero())?;
    }

    Ok(())
}

/// Get the current stake amount for a specific contract from CONTRACT_STAKES.
fn get_contract_stake(
    storage: &dyn Storage,
    contract_addr: &Addr,
) -> Result<Uint128, ContractError> {
    Ok(CONTRACT_STAKES
        .may_load(storage, contract_addr)?
        .unwrap_or_default())
}

/// Obtain summaries of rewards and deposit records for all contracts. This query helps users understand 
/// pending rewards, pending deposits, and completed deposits at a glance.
fn get_reward_summaries(
    storage: &dyn Storage,
    api: &dyn Api,
) -> Result<RewardSummariesResponse, ContractError> {
    let contracts = get_all_contracts(storage)?;
    let mut contract_summaries = Vec::new();

    let mut total_pending_rewards = Uint128::zero();
    let mut total_deposit_pending = Uint128::zero();
    let mut total_deposit_completed = Uint128::zero();

    for contract_addr in contracts {
        let contract_address = contract_addr.to_string();

        // Retrieve contract metadata to confirm its existence and get associated addresses.
        let metadata = CONTRACT_METADATA.load(storage, &contract_addr)?;
        let _rewards_addr = api.addr_validate(&metadata.rewards_address)?;

        // Get the pending rewards from CONTRACT_REWARDS for this contract.
        let pending_rewards = CONTRACT_REWARDS
            .may_load(storage, &contract_addr)?
            .unwrap_or_default();

        total_pending_rewards += pending_rewards;

        // Retrieve deposit records and categorize them into pending and completed totals.
        let deposit_records = DEPOSIT_RECORDS
            .may_load(storage, &contract_addr)?
            .unwrap_or_default();

        let mut deposit_pending = Uint128::zero();
        let mut deposit_completed = Uint128::zero();

        for record in deposit_records {
            if record.status == "pending" {
                deposit_pending += record.amount;
            } else if record.status == "completed" {
                deposit_completed += record.amount;
            }
        }

        total_deposit_pending += deposit_pending;
        total_deposit_completed += deposit_completed;

        contract_summaries.push(ContractRewardSummary {
            contract_address,
            pending_rewards,
            deposit_pending,
            deposit_completed,
        });
    }

    Ok(RewardSummariesResponse {
        contract_summaries,
        total_pending_rewards,
        total_deposit_pending,
        total_deposit_completed,
    })
}

/// The `query` entry point handles read-only queries. Each query variant retrieves specific pieces 
/// of information about the contract state (e.g., config, total stake, records, metadata, etc.).
#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> Result<Binary, ContractError> {
    match msg {
        QueryMsg::GetConfig {} => to_json_binary(&CONFIG.load(deps.storage)?)
            .map_err(ContractError::from),

        QueryMsg::GetTotalLiquidStakeQuery {} => {
            let total_stake = get_total_liquid_stake_query(deps)?;
            // Use to_json_binary for encoding responses if you prefer JSON format consistently.
            to_json_binary(&total_stake).map_err(ContractError::from)
        }

        QueryMsg::GetDepositRecords { contract } => {
            let addr = deps.api.addr_validate(&contract)?;
            let records = DEPOSIT_RECORDS
                .may_load(deps.storage, &addr)?
                .unwrap_or_default();
            to_json_binary(&records).map_err(ContractError::from)
        }

        QueryMsg::GetStakeRatio { contract } => {
            let addr = deps.api.addr_validate(&contract)?;
            let stake_ratio = STAKE_RATIOS
                .may_load(deps.storage, &addr)?
                .unwrap_or_default();
            to_json_binary(&stake_ratio.to_string()).map_err(ContractError::from)
        }

        QueryMsg::GetAllStakeRatios {} => {
            let ratios = get_all_stake_ratios(deps.storage)?;
            to_json_binary(&ratios).map_err(ContractError::from)
        }

        QueryMsg::GetContractMetadata { contract } => {
            let addr = deps.api.addr_validate(&contract)?;
            let metadata = CONTRACT_METADATA.load(deps.storage, &addr)?;
            to_json_binary(&metadata).map_err(ContractError::from)
        }

        QueryMsg::GetContractStake { contract } => {
            let addr = deps.api.addr_validate(&contract)?;
            let stake = CONTRACT_STAKES
                .may_load(deps.storage, &addr)?
                .unwrap_or_default();
            to_json_binary(&stake).map_err(ContractError::from)
        }

        QueryMsg::GetReward { rewards_address } => {
            let addr = deps.api.addr_validate(&rewards_address)?;
            let reward = CONTRACT_REWARDS
                .may_load(deps.storage, &addr)?
                .unwrap_or_default();
            to_json_binary(&reward).map_err(ContractError::from)
        }

        QueryMsg::GetRedeemTokens { contract } => {
            let addr = deps.api.addr_validate(&contract)?;
            let tokens = REDEEM_TOKENS
                .may_load(deps.storage, &addr)?
                .unwrap_or_default();
            to_json_binary(&tokens).map_err(ContractError::from)
        }

        QueryMsg::GetAllContracts {} => {
            let contracts = get_all_contracts(deps.storage)?;
            let contract_list: Vec<String> = contracts
                .into_iter()
                .map(|c| c.to_string())
                .collect();
            to_json_binary(&contract_list).map_err(ContractError::from)
        }

        QueryMsg::GetAllRedemptionRatios {} => {
            let ratios = get_all_redeem_token_ratios(deps.storage)?;
            to_json_binary(&ratios).map_err(ContractError::from)
        }

        QueryMsg::GetRewardSummaries {} => {
            let reward_summaries = get_reward_summaries(deps.storage, deps.api)?;
            to_json_binary(&reward_summaries).map_err(ContractError::from)
        }
    }
}

/// Retrieve all stake ratios stored in STAKE_RATIOS, returning them as a vector of (contract, ratio) strings.
fn get_all_stake_ratios(storage: &dyn Storage) -> Result<Vec<(String, String)>, ContractError> {
    let ratios = STAKE_RATIOS
        .range(storage, None, None, Order::Ascending)
        .map(|item| {
            let (addr, ratio) = item?;
            Ok((addr.to_string(), ratio.to_string()))
        })
        .collect::<StdResult<Vec<(String, String)>>>()?;
    Ok(ratios)
}

/// Retrieve all redemption token ratios from REDEEM_TOKEN_RATIOS in ascending order, returning them as (contract, ratio) pairs.
fn get_all_redeem_token_ratios(
    storage: &dyn Storage,
) -> Result<Vec<(String, String)>, ContractError> {
    let ratios = REDEEM_TOKEN_RATIOS
        .range(storage, None, None, Order::Ascending)
        .map(|item| {
            let (addr, ratio) = item?;
            Ok((addr.to_string(), ratio.to_string()))
        })
        .collect::<StdResult<Vec<(String, String)>>>()?;
    Ok(ratios)
}

/// The `migrate` entry point is invoked to migrate the contract to a new code version. 
/// In this contract, migrate does nothing and just returns a default (no-op) response.
#[entry_point]
pub fn migrate(
    _deps: DepsMut,
    _env: Env,
    _msg: MigrateMsg,
) -> Result<Response, ContractError> {
    Ok(Response::default())
}

