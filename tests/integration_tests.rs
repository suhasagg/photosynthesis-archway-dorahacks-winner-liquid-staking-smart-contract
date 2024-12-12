#[cfg(test)]
mod integration_tests {
    // Import standard CosmWasm types
    use cosmwasm_std::{
        testing::{mock_dependencies, mock_env, mock_info},
        Addr, Uint128, Empty, Decimal, StdError, from_binary, to_binary
    };
    use cw_multi_test::{App, Contract, ContractWrapper, Executor};

    use cosmwasm_liquid_staking::contract::{execute, instantiate, query, migrate};

    use cosmwasm_liquid_staking::msg::{
        InstantiateMsg, ExecuteMsg, QueryMsg, RewardUpdate, Distribution, RewardSummariesResponse
    };

    use cosmwasm_liquid_staking::error::ContractError;
    use cosmwasm_liquid_staking::state::{
        CONFIG, CONTRACT_REWARDS, CONTRACT_METADATA, REDEEM_TOKENS, TOTAL_LIQUID_STAKE,
        REDEMPTION_RECORDS, STAKE_RATIOS, REDEEM_TOKEN_RATIOS,
        Config, ContractMetadata, DepositRecord,
    };


    pub fn contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            execute,
            instantiate,
            query,
        ).with_migrate(migrate);
        Box::new(contract)
    }

    fn mock_app() -> App {
        App::default()
    }

    fn init_contract(
        router: &mut App,
        owner: &str,
        init_msg: InstantiateMsg
    ) -> (Addr, u64) {
        let code_id = router.store_code(contract());
        let addr = router
            .instantiate_contract(
                code_id,
                Addr::unchecked(owner),
                &init_msg,
                &[],
                "TestContract",
                None,
            )
            .unwrap();
        (addr, code_id)
    }

    #[test]
    fn test_instantiate_and_query_config() {
        let mut app = mock_app();
        let owner = "wasm1ownerxyz";
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 3600,
            arch_liquid_stake_interval: 7200,
            redemption_rate_query_interval: 10800,
            rewards_withdrawal_interval: 14400,
            redemption_interval_threshold: 1800,
        };

        let (contract_addr, _) = init_contract(&mut app, owner, init_msg.clone());

        let config: Config = app.wrap().query_wasm_smart(&contract_addr, &QueryMsg::GetConfig {}).unwrap();
        assert_eq!(config.owner, Addr::unchecked(owner));
        assert_eq!(config.liquid_staking_interval, init_msg.liquid_staking_interval);
    }

    #[test]
    fn test_set_contract_metadata_and_queries() {
        let mut app = mock_app();
        let owner = "wasm1ownerxyz";
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 3600,
            arch_liquid_stake_interval: 7200,
            redemption_rate_query_interval: 10800,
            rewards_withdrawal_interval: 14400,
            redemption_interval_threshold: 1800,
        };
        let (contract_addr, _) = init_contract(&mut app, owner, init_msg);

        let dapp_contract = "wasm1dappxyz";

        // Non-owner attempt
        let err = app.execute_contract(
            Addr::unchecked("wasm1notownerxyz"),
            contract_addr.clone(),
            &ExecuteMsg::SetContractMetadata {
                contract_address: dapp_contract.to_string(),
                rewards_address: "wasm1rewardsxyz".to_string(),
                liquidity_provider_address: "wasm1lpxyz".to_string(),
                redemption_address: "wasm1redemptionxyz".to_string(),
                minimum_reward_amount: Uint128::new(100),
                maximum_reward_amount: Uint128::new(50),
            },
            &[]
        ).unwrap_err();
        assert!(matches!(err.downcast_ref::<ContractError>(), Some(ContractError::Unauthorized {})));

        // Invalid range as owner
        let err = app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::SetContractMetadata {
                contract_address: dapp_contract.to_string(),
                rewards_address: "wasm1rewardsxyz".to_string(),
                liquidity_provider_address: "wasm1lpxyz".to_string(),
                redemption_address: "wasm1redemptionxyz".to_string(),
                minimum_reward_amount: Uint128::new(100),
                maximum_reward_amount: Uint128::new(50),
            },
            &[]
        ).unwrap_err();
        assert!(matches!(err.downcast_ref::<ContractError>(), Some(ContractError::InvalidRewardAmountRange {})));

        // Valid set
        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::SetContractMetadata {
                contract_address: dapp_contract.to_string(),
                rewards_address: "wasm1rewardsxyz".to_string(),
                liquidity_provider_address: "wasm1lpxyz".to_string(),
                redemption_address: "wasm1redemptionxyz".to_string(),
                minimum_reward_amount: Uint128::new(100),
                maximum_reward_amount: Uint128::new(1000),
            },
            &[]
        ).unwrap();

        let meta: ContractMetadata = app.wrap().query_wasm_smart(
            &contract_addr,
            &QueryMsg::GetContractMetadata { contract: dapp_contract.to_string() }
        ).unwrap();
        assert_eq!(meta.rewards_address, "wasm1rewardsxyz");
    }

    #[test]
    fn test_add_stake_and_query_stake() {
        let mut app = mock_app();
        let owner = "wasm1ownerxyz";
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 3600,
            arch_liquid_stake_interval: 7200,
            redemption_rate_query_interval: 10800,
            rewards_withdrawal_interval: 1800,
            redemption_interval_threshold: 14600,
        };
        let (contract_addr, _) = init_contract(&mut app, owner, init_msg);

        let staker = "wasm1stakerxyz";
        app.execute_contract(
            Addr::unchecked(staker),
            contract_addr.clone(),
            &ExecuteMsg::AddStake {
                amount: Uint128::new(500),
            },
            &[]
        ).unwrap();

        let stake: Uint128 = app.wrap().query_wasm_smart(
            &contract_addr,
            &QueryMsg::GetContractStake { contract: staker.to_string() },
        ).unwrap();
        assert_eq!(stake, Uint128::new(500));
    }

    #[test]
    fn test_reward_updates() {
        let mut app = mock_app();
        let owner = "wasm1ownerxyz";
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 3600,
            arch_liquid_stake_interval: 7200,
            redemption_rate_query_interval: 10800,
            rewards_withdrawal_interval: 1800,
            redemption_interval_threshold: 14600,
        };

        let (contract_addr, _) = init_contract(&mut app, owner, init_msg);
        let dapp_contract = "wasm1dappxyz";

        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::SetContractMetadata {
                contract_address: dapp_contract.to_string(),
                rewards_address: "wasm1rewardsxyz".to_string(),
                liquidity_provider_address: "wasm1lpxyz".to_string(),
                redemption_address: "wasm1redemptionxyz".to_string(),
                minimum_reward_amount: Uint128::new(100),
                maximum_reward_amount: Uint128::new(2000),
            },
            &[]
        ).unwrap();

        // Non-owner update -> fail
        let err = app.execute_contract(
            Addr::unchecked("wasm1notownerxyz"),
            contract_addr.clone(),
            &ExecuteMsg::UpdateReward {
                rewards_address: dapp_contract.to_string(),
                amount: Uint128::new(300),
            },
            &[]
        ).unwrap_err();
        assert!(matches!(err.downcast_ref::<ContractError>(), Some(ContractError::Unauthorized {})));

        // Owner update
        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::UpdateReward {
                rewards_address: dapp_contract.to_string(),
                amount: Uint128::new(300),
            },
            &[]
        ).unwrap();

        let reward: Uint128 = app.wrap().query_wasm_smart(
            &contract_addr,
            &QueryMsg::GetReward { rewards_address: dapp_contract.to_string() },
        ).unwrap();
        assert_eq!(reward, Uint128::new(300));

        // Bulk update
        let updates = vec![
            RewardUpdate {
                contract_address: dapp_contract.to_string(),
                amount: Uint128::new(200),
            },
            RewardUpdate {
                contract_address: dapp_contract.to_string(),
                amount: Uint128::new(500),
            }
        ];

        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::BulkUpdateRewards { updates },
            &[]
        ).unwrap();

        let reward: Uint128 = app.wrap().query_wasm_smart(
            &contract_addr,
            &QueryMsg::GetReward { rewards_address: dapp_contract.to_string() },
        ).unwrap();
        assert_eq!(reward, Uint128::new(1000));
    }

    #[test]
    fn test_cron_job_execution() {
        let mut app = mock_app();
        let owner = "wasm1ownerxyz";

        let init_msg = InstantiateMsg {
            liquid_staking_interval: 1,
            arch_liquid_stake_interval: 3,
            redemption_rate_query_interval: 5,
            rewards_withdrawal_interval: 1,
            redemption_interval_threshold: 5,
        };

        let (contract_addr, _) = init_contract(&mut app, owner, init_msg);

        let dapp_contract = "wasm1dappxyz";
        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::SetContractMetadata {
                contract_address: dapp_contract.to_string(),
                rewards_address: "wasm1rewardsxyz".to_string(),
                liquidity_provider_address: "wasm1lpxyz".to_string(),
                redemption_address: "wasm1redemptionxyz".to_string(),
                minimum_reward_amount: Uint128::new(50),
                maximum_reward_amount: Uint128::new(1000),
            },
            &[]
        ).unwrap();

        // Add rewards
        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::UpdateReward {
                rewards_address: dapp_contract.to_string(),
                amount: Uint128::new(100),
            },
            &[]
        ).unwrap();

        // Advance time just enough for one cron cycle
        app.update_block(|block| {
            block.time = block.time.plus_seconds(2); // interval was 1 second
        });

        // Run cron once - this should create pending deposits, not complete them
        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::CronJob {},
            &[]
        ).unwrap();

        let records: Vec<DepositRecord> = app.wrap().query_wasm_smart(
            &contract_addr,
            &QueryMsg::GetDepositRecords { contract: dapp_contract.to_string() },
        ).unwrap();

        assert_eq!(records.len(), 1);
        // Ensure it's still pending (not completed)
        assert_eq!(records[0].status, "pending");
    }

    #[test]
    fn test_reset_all_completed_deposit_records() {
        let mut app = mock_app();
        let owner = "wasm1ownerxyz";
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 1,
            arch_liquid_stake_interval: 3,
            redemption_rate_query_interval: 5,
            rewards_withdrawal_interval: 1,
            redemption_interval_threshold: 5,
        };
        let (contract_addr, _) = init_contract(&mut app, owner, init_msg);
        let c = "wasm1testxyz";

        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::SetContractMetadata {
                contract_address: c.to_string(),
                rewards_address: "wasm1rxyz".to_string(),
                liquidity_provider_address: "wasm1lpxyz".to_string(),
                redemption_address: "wasm1rdxyz".to_string(),
                minimum_reward_amount: Uint128::new(10),
                maximum_reward_amount: Uint128::new(2000),
            },
            &[]
        ).unwrap();

        // Add reward to create deposit records
        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::UpdateReward {
                rewards_address: c.to_string(),
                amount: Uint128::new(100),
            },
            &[]
        ).unwrap();

        // First cron: create pending
        app.update_block(|b| b.time = b.time.plus_seconds(2));
        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::CronJob {},
            &[]
        ).unwrap();

        // Second cron: complete them
        app.update_block(|b| b.time = b.time.plus_seconds(2));
        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::CronJob {},
            &[]
        ).unwrap();

        let records: Vec<DepositRecord> = app.wrap().query_wasm_smart(
            &contract_addr,
            &QueryMsg::GetDepositRecords { contract: c.to_string() },
        ).unwrap();
        assert!(records.iter().any(|r| r.status == "completed"));

        // Reset completed
        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::ResetAllCompletedDepositRecords {},
            &[]
        ).unwrap();

        let records_after: Vec<DepositRecord> = app.wrap().query_wasm_smart(
            &contract_addr,
            &QueryMsg::GetDepositRecords { contract: c.to_string() },
        ).unwrap();
        assert!(!records_after.iter().any(|r| r.status == "completed"));
    }

    #[test]
    fn test_redeem_tokens_distribution_and_reset() {
        let mut app = mock_app();
        let owner = "wasm1ownerxyz";
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 3600,
            arch_liquid_stake_interval: 7200,
            redemption_rate_query_interval: 10800,
            rewards_withdrawal_interval: 14400,
            redemption_interval_threshold: 1800,
        };
        let (contract_addr, _) = init_contract(&mut app, owner, init_msg);

        let c1 = "wasm1redeemc1xyz";
        let c2 = "wasm1redeemc2xyz";
        for c in &[c1, c2] {
            app.execute_contract(
                Addr::unchecked(owner),
                contract_addr.clone(),
                &ExecuteMsg::SetContractMetadata {
                    contract_address: c.to_string(),
                    rewards_address: format!("{}r", c),
                    liquidity_provider_address: format!("{}lp", c),
                    redemption_address: format!("{}rd", c),
                    minimum_reward_amount: Uint128::new(10),
                    maximum_reward_amount: Uint128::new(2000),
                },
                &[]
            ).unwrap();
        }

        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::SetRedeemTokens {
                amount: Uint128::new(200),
                contract_address: c1.to_string(),
            },
            &[]
        ).unwrap();

        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::SetRedeemTokens {
                amount: Uint128::new(800),
                contract_address: c2.to_string(),
            },
            &[]
        ).unwrap();

        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::DistributeRedeemTokens {},
            &[]
        ).unwrap();

        let redemption_ratios: Vec<(String, String)> = app.wrap().query_wasm_smart(
            &contract_addr,
            &QueryMsg::GetAllRedemptionRatios {}
        ).unwrap();
        let mut ratio_map = std::collections::HashMap::new();
        for (addr, ratio_str) in redemption_ratios {
            ratio_map.insert(addr, ratio_str);
        }
        assert_eq!(ratio_map.get(c1), Some(&"0.2".to_string()));
        assert_eq!(ratio_map.get(c2), Some(&"0.8".to_string()));

        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::ResetRedemptionRatios {},
            &[]
        ).unwrap();

        let redemption_ratios_after: Vec<(String, String)> = app.wrap().query_wasm_smart(
            &contract_addr,
            &QueryMsg::GetAllRedemptionRatios {}
        ).unwrap();
        assert!(redemption_ratios_after.is_empty());
    }

    #[test]
    fn test_emit_events() {
        let mut app = mock_app();
        let owner = "wasm1ownerxyz";
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 3600,
            arch_liquid_stake_interval: 7200,
            redemption_rate_query_interval: 10800,
            rewards_withdrawal_interval: 14400,
            redemption_interval_threshold: 1800,
        };
        let (contract_addr, _) = init_contract(&mut app, owner, init_msg);

        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::EmitLiquidStakeEvent {
                total_liquid_stake: Uint128::new(1000),
                stuarch_obtained: Uint128::new(500),
                tx_hash: "test_tx".to_string(),
            },
            &[]
        ).unwrap();

        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::EmitDistributeLiquidityEvent {
                distributions: vec![
                    Distribution {
                        liquidity_address: "wasm1liquidity1xyz".to_string(),
                        amount: Uint128::new(100),
                    },
                    Distribution {
                        liquidity_address: "wasm1liquidity2xyz".to_string(),
                        amount: Uint128::new(200),
                    }
                ]
            },
            &[]
        ).unwrap();
    }

    #[test]
    fn test_reset_stake_ratios() {
        let mut app = mock_app();
        let owner = "wasm1ownerxyz";
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 1,
            arch_liquid_stake_interval: 1,
            redemption_rate_query_interval: 1,
            rewards_withdrawal_interval: 1,
            redemption_interval_threshold: 1,
        };
        let (contract_addr, _) = init_contract(&mut app, owner, init_msg);

        let c = "wasm1contracttestxyz";
        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::SetContractMetadata {
                contract_address: c.to_string(),
                rewards_address: "wasm1rxyz".to_string(),
                liquidity_provider_address: "wasm1lpxyz".to_string(),
                redemption_address: "wasm1rdxyz".to_string(),
                minimum_reward_amount: Uint128::new(10),
                maximum_reward_amount: Uint128::new(1000),
            },
            &[]
        ).unwrap();

        app.execute_contract(
            Addr::unchecked(c),
            contract_addr.clone(),
            &ExecuteMsg::AddStake { amount: Uint128::new(500) },
            &[]
        ).unwrap();
        app.update_block(|b| b.time = b.time.plus_seconds(2));
        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::CronJob {},
            &[]
        ).unwrap();
        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::DistributeLiquidity {},
            &[]
        ).unwrap();

        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::ResetStakeRatios {},
            &[]
        ).unwrap();

        let ratios: Vec<(String, String)> = app.wrap().query_wasm_smart(
            &contract_addr,
            &QueryMsg::GetAllStakeRatios {}
        ).unwrap();
        assert!(ratios.is_empty());
    }


    #[test]
    fn test_get_all_contracts_query() {
        let mut app = mock_app();
        let owner = "wasm1ownerxyz";
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 3600,
            arch_liquid_stake_interval: 7200,
            redemption_rate_query_interval: 10800,
            rewards_withdrawal_interval: 14400,
            redemption_interval_threshold: 1800,
        };
        let (contract_addr, _) = init_contract(&mut app, owner, init_msg);

        let c1 = "wasm1c1xyz";
        let c2 = "wasm1c2xyz";
        for c in &[c1, c2] {
            let res = app.execute_contract(
                Addr::unchecked(owner),
                contract_addr.clone(),
                &ExecuteMsg::SetContractMetadata {
                    contract_address: c.to_string(),
                    rewards_address: "wasm1rxyz".to_string(),
                    liquidity_provider_address: "wasm1lpxyz".to_string(),
                    redemption_address: "wasm1rdxyz".to_string(),
                    minimum_reward_amount: Uint128::new(10),
                    maximum_reward_amount: Uint128::new(1000),
                },
                &[]
            );
            assert!(res.is_ok());
        }

        let all_contracts: Vec<String> = app.wrap().query_wasm_smart(
            &contract_addr,
            &QueryMsg::GetAllContracts {}
        ).unwrap();
        assert!(all_contracts.contains(&c1.to_string()));
        assert!(all_contracts.contains(&c2.to_string()));
    }

    #[test]
    fn test_reward_summaries_query() {
        let mut app = mock_app();
        let owner = "wasm1ownerxyz";

        let init_msg = InstantiateMsg {
            liquid_staking_interval: 3600,
            arch_liquid_stake_interval: 7200,
            redemption_rate_query_interval: 10800,
            rewards_withdrawal_interval: 14400,
            redemption_interval_threshold: 1800,
        };
        let (contract_addr, _) = init_contract(&mut app, owner, init_msg);

        let c1 = "wasm1summaryc1xyz";
        let c2 = "wasm1summaryc2xyz";
        for c in &[c1, c2] {
            app.execute_contract(
                Addr::unchecked(owner),
                contract_addr.clone(),
                &ExecuteMsg::SetContractMetadata {
                    contract_address: c.to_string(),
                    rewards_address: format!("{}r", c),
                    liquidity_provider_address: format!("{}lp", c),
                    redemption_address: format!("{}rd", c),
                    minimum_reward_amount: Uint128::new(50),
                    maximum_reward_amount: Uint128::new(2000),
                },
                &[]
            ).unwrap();
        }

        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::UpdateReward {
                rewards_address: c1.to_string(),
                amount: Uint128::new(300),
            },
            &[]
        ).unwrap();

        app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::UpdateReward {
                rewards_address: c2.to_string(),
                amount: Uint128::new(150),
            },
            &[]
        ).unwrap();

        let summaries: RewardSummariesResponse = app.wrap().query_wasm_smart(
            &contract_addr,
            &QueryMsg::GetRewardSummaries {}
        ).unwrap();

        assert_eq!(summaries.contract_summaries.len(), 2);
        let c1_summary = summaries.contract_summaries.iter().find(|s| s.contract_address == c1).unwrap();
        let c2_summary = summaries.contract_summaries.iter().find(|s| s.contract_address == c2).unwrap();

        assert_eq!(c1_summary.pending_rewards, Uint128::new(300));
        assert_eq!(c2_summary.pending_rewards, Uint128::new(150));
        assert_eq!(summaries.total_pending_rewards, Uint128::new(450));
    }

    #[test]
    fn test_distribute_liquidity_with_no_stakes() {
        let mut app = mock_app();
        let owner = "wasm1ownerxyz";
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 1,
            arch_liquid_stake_interval: 1,
            redemption_rate_query_interval: 1,
            rewards_withdrawal_interval: 1,
            redemption_interval_threshold: 1,
        };
        let (contract_addr, _) = init_contract(&mut app, owner, init_msg);

        // No stakes added yet, just execute DistributeLiquidity
        let res = app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::DistributeLiquidity {},
            &[]
        );
        // Should succeed with no distribution made
        assert!(res.is_ok());
    }

    #[test]
    fn test_cron_job_no_task_if_time_not_elapsed() {
        let mut app = mock_app();
        let owner = "wasm1ownerxyz";
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 10,
            arch_liquid_stake_interval: 20,
            redemption_rate_query_interval: 30,
            rewards_withdrawal_interval: 40,
            redemption_interval_threshold: 10,
        };
        let (contract_addr, _) = init_contract(&mut app, owner, init_msg);

        // Run cron immediately without advancing time
        let res = app.execute_contract(
            Addr::unchecked(owner),
            contract_addr.clone(),
            &ExecuteMsg::CronJob {},
            &[]
        ).unwrap();

        // No tasks should have triggered since no time passed
        let records: Vec<DepositRecord> = app.wrap().query_wasm_smart(
            &contract_addr,
            &QueryMsg::GetDepositRecords { contract: "non_existent_contract".to_string() },
        ).unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn test_instantiate() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let msg = InstantiateMsg {
            liquid_staking_interval: 3600,
            arch_liquid_stake_interval: 7200,
            redemption_rate_query_interval: 10800,
            rewards_withdrawal_interval: 14400,
            redemption_interval_threshold: 1800,
        };

        let res = instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
        assert_eq!(res.messages.len(), 0);

        // Query config
        let config = CONFIG.load(&deps.storage).unwrap();
        assert_eq!(config.owner, Addr::unchecked("creator"));
        assert_eq!(config.liquid_staking_interval, 3600);
    }

    #[test]
    fn test_unauthorized_set_contract_metadata() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 3600,
            arch_liquid_stake_interval: 7200,
            redemption_rate_query_interval: 10800,
            rewards_withdrawal_interval: 14400,
            redemption_interval_threshold: 1800,
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), init_msg).unwrap();

        let unauth_info = mock_info("other", &[]);
        let exec_msg = ExecuteMsg::SetContractMetadata {
            contract_address: "contract1".to_string(),
            rewards_address: "rewards1".to_string(),
            liquidity_provider_address: "lp1".to_string(),
            redemption_address: "red1".to_string(),
            minimum_reward_amount: Uint128::new(100),
            maximum_reward_amount: Uint128::new(50),
        };
        let err = execute(deps.as_mut(), env.clone(), unauth_info, exec_msg).unwrap_err();
        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn test_invalid_reward_amount_range() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 3600,
            arch_liquid_stake_interval: 7200,
            redemption_rate_query_interval: 10800,
            rewards_withdrawal_interval: 14400,
            redemption_interval_threshold: 1800,
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), init_msg).unwrap();

        // Attempt to set invalid range as owner
        let exec_msg = ExecuteMsg::SetContractMetadata {
            contract_address: "contract1".to_string(),
            rewards_address: "rewards1".to_string(),
            liquidity_provider_address: "lp1".to_string(),
            redemption_address: "red1".to_string(),
            minimum_reward_amount: Uint128::new(100),
            maximum_reward_amount: Uint128::new(50),
        };
        let err = execute(deps.as_mut(), env.clone(), info.clone(), exec_msg).unwrap_err();
        assert!(matches!(err, ContractError::InvalidRewardAmountRange {}));
    }

    #[test]
    fn test_update_reward_authorized() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 3600,
            arch_liquid_stake_interval: 7200,
            redemption_rate_query_interval: 10800,
            rewards_withdrawal_interval: 14400,
            redemption_interval_threshold: 1800,
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), init_msg).unwrap();

        // Set metadata first
        let exec_msg = ExecuteMsg::SetContractMetadata {
            contract_address: "contract1".to_string(),
            rewards_address: "rewards1".to_string(),
            liquidity_provider_address: "lp1".to_string(),
            redemption_address: "red1".to_string(),
            minimum_reward_amount: Uint128::new(50),
            maximum_reward_amount: Uint128::new(1000),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), exec_msg).unwrap();

        // Now update reward
        let update_msg = ExecuteMsg::UpdateReward {
            rewards_address: "contract1".to_string(),
            amount: Uint128::new(500),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), update_msg).unwrap();

        let reward = CONTRACT_REWARDS
            .may_load(&deps.storage, &Addr::unchecked("contract1"))
            .unwrap()
            .unwrap();
        assert_eq!(reward, Uint128::new(500));
    }

    #[test]
    fn test_query_config() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 10,
            arch_liquid_stake_interval: 20,
            redemption_rate_query_interval: 30,
            rewards_withdrawal_interval: 40,
            redemption_interval_threshold: 5,
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), init_msg.clone()).unwrap();

        let bin = query(deps.as_ref(), env.clone(), QueryMsg::GetConfig {}).unwrap();
        let cfg: Config = from_binary(&bin).unwrap();
        assert_eq!(cfg.owner, Addr::unchecked("creator"));
        assert_eq!(cfg.arch_liquid_stake_interval, 20);
        assert_eq!(cfg.redemption_rate_query_interval, 30);
    }

    #[test]
    fn test_bulk_update_rewards_unauthorized() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 10,
            arch_liquid_stake_interval: 20,
            redemption_rate_query_interval: 30,
            rewards_withdrawal_interval: 40,
            redemption_interval_threshold: 5,
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), init_msg).unwrap();

        let other_info = mock_info("not_owner", &[]);
        let bulk_msg = ExecuteMsg::BulkUpdateRewards {
            updates: vec![RewardUpdate {
                contract_address: "contractx".to_string(),
                amount: Uint128::new(100),
            }],
        };
        let err = execute(deps.as_mut(), env.clone(), other_info, bulk_msg).unwrap_err();
        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn test_set_redeem_tokens() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 10,
            arch_liquid_stake_interval: 20,
            redemption_rate_query_interval: 30,
            rewards_withdrawal_interval: 40,
            redemption_interval_threshold: 5,
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), init_msg).unwrap();

        // Contract metadata must exist for contract1
        let meta_msg = ExecuteMsg::SetContractMetadata {
            contract_address: "contract1".to_string(),
            rewards_address: "r1".to_string(),
            liquidity_provider_address: "lp1".to_string(),
            redemption_address: "red1".to_string(),
            minimum_reward_amount: Uint128::new(10),
            maximum_reward_amount: Uint128::new(1000),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), meta_msg).unwrap();

        // Set redeem tokens
        let redeem_msg = ExecuteMsg::SetRedeemTokens {
            amount: Uint128::new(200),
            contract_address: "contract1".to_string(),
        };
        execute(deps.as_mut(), env.clone(), info.clone(), redeem_msg).unwrap();

        let tokens = REDEMPTION_RECORDS
            .may_load(&deps.storage, &Addr::unchecked("contract1"))
            .unwrap()
            .unwrap();
        assert_eq!(tokens, Uint128::new(200));
    }

    #[test]
    fn test_set_redeem_tokens_nonexistent_contract() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 10,
            arch_liquid_stake_interval: 20,
            redemption_rate_query_interval: 30,
            rewards_withdrawal_interval: 40,
            redemption_interval_threshold: 5,
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), init_msg).unwrap();

        // Attempt to set redeem tokens for a contract that has no metadata
        let redeem_msg = ExecuteMsg::SetRedeemTokens {
            amount: Uint128::new(100),
            contract_address: "nonexistent".to_string(),
        };
        let err = execute(deps.as_mut(), env.clone(), info.clone(), redeem_msg).unwrap_err();
        assert!(matches!(err, ContractError::ContractNotFound { .. }));
    }

    #[test]
    fn test_distribute_redeem_tokens_no_records() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 10,
            arch_liquid_stake_interval: 20,
            redemption_rate_query_interval: 30,
            rewards_withdrawal_interval: 40,
            redemption_interval_threshold: 5,
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), init_msg).unwrap();

        // Distribute redeem tokens with no records
        let err = execute(deps.as_mut(), env.clone(), info.clone(), ExecuteMsg::DistributeRedeemTokens {}).unwrap_err();
        assert!(matches!(err, ContractError::NoRedemptionRecords {}));
    }

    #[test]
    fn test_subtract_from_total_liquid_stake_unauthorized() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 10,
            arch_liquid_stake_interval: 20,
            redemption_rate_query_interval: 30,
            rewards_withdrawal_interval: 40,
            redemption_interval_threshold: 5,
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), init_msg).unwrap();

        // Set total liquid stake directly for testing
        TOTAL_LIQUID_STAKE.save(&mut deps.storage, &Uint128::new(1000)).unwrap();

        // Unauthorized attempt
        let other_info = mock_info("not_owner", &[]);
        let exec_msg = ExecuteMsg::SubtractFromTotalLiquidStake { amount: Uint128::new(200) };
        let err = execute(deps.as_mut(), env.clone(), other_info, exec_msg).unwrap_err();
        assert!(matches!(err, ContractError::Unauthorized {}));
    }

    #[test]
    fn test_subtract_from_total_liquid_stake_overflow() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 10,
            arch_liquid_stake_interval: 20,
            redemption_rate_query_interval: 30,
            rewards_withdrawal_interval: 40,
            redemption_interval_threshold: 5,
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), init_msg).unwrap();

        // Set total liquid stake directly
        TOTAL_LIQUID_STAKE.save(&mut deps.storage, &Uint128::new(500)).unwrap();

        // Attempt to subtract more than available
        let exec_msg = ExecuteMsg::SubtractFromTotalLiquidStake { amount: Uint128::new(1000) };
        let err = execute(deps.as_mut(), env.clone(), info.clone(), exec_msg).unwrap_err();
        match err {
            ContractError::Std(StdError::Overflow { .. }) => (),
            _ => panic!("Expected overflow error"),
        }
    }

    #[test]
    fn test_query_stake_ratios_empty() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 10,
            arch_liquid_stake_interval: 20,
            redemption_rate_query_interval: 30,
            rewards_withdrawal_interval: 40,
            redemption_interval_threshold: 5,
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), init_msg).unwrap();

        // No stake ratios set, query all stake ratios should return empty
        let bin = query(deps.as_ref(), env.clone(), QueryMsg::GetAllStakeRatios {}).unwrap();
        let ratios: Vec<(String, String)> = from_binary(&bin).unwrap();
        assert!(ratios.is_empty());
    }
    

    #[test]
    fn test_reset_redemption_ratios_unit() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let init_msg = InstantiateMsg {
            liquid_staking_interval: 10,
            arch_liquid_stake_interval: 20,
            redemption_rate_query_interval: 30,
            rewards_withdrawal_interval: 40,
            redemption_interval_threshold: 5,
        };
        instantiate(deps.as_mut(), env.clone(), info.clone(), init_msg).unwrap();

        // Insert some redemption ratios manually
        REDEEM_TOKEN_RATIOS.save(&mut deps.storage, &Addr::unchecked("c1"), &Decimal::percent(20)).unwrap();
        REDEEM_TOKEN_RATIOS.save(&mut deps.storage, &Addr::unchecked("c2"), &Decimal::percent(80)).unwrap();

        // Reset them
        execute(deps.as_mut(), env.clone(), info.clone(), ExecuteMsg::ResetRedemptionRatios {}).unwrap();

        let bin = query(deps.as_ref(), env.clone(), QueryMsg::GetAllRedemptionRatios {}).unwrap();
        let ratios: Vec<(String, String)> = from_binary(&bin).unwrap();
        assert!(ratios.is_empty());
    }
}

   
