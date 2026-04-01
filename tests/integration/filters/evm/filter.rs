//! Integration tests for EVM chain monitoring.
//!
//! Tests the monitoring functionality for EVM-compatible blockchains,
//! including event and transaction filtering.

use alloy::primitives::Uint;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use openzeppelin_monitor::{
	models::{
		BlockType, ContractSpec, EVMReceiptLog, EVMTransactionReceipt, EventCondition,
		FunctionCondition, Monitor, MonitorMatch, TransactionCondition, TransactionStatus,
	},
	services::{
		blockchain::{EvmClient, TransportError},
		filter::{handle_match, FilterError, FilterService},
	},
	utils::tests::evm::{receipt::ReceiptBuilder, transaction::TransactionBuilder},
};

use crate::integration::{
	filters::common::{setup_trigger_execution_service, TestData, TestDataBuilder},
	mocks::MockEVMTransportClient,
};

fn setup_mock_transport(test_data: TestData) -> MockEVMTransportClient {
	let mut mock_transport = MockEVMTransportClient::new();
	let counter = std::sync::atomic::AtomicUsize::new(0);
	let receipts = test_data.receipts;

	mock_transport
		.expect_send_raw_request()
		.returning(move |method, _params| {
			let current = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
			match (method, current) {
				("net_version", _) => Ok(json!({"result": "1"})),
				("eth_getTransactionReceipt", i) => Ok(json!({
					"result": &receipts[i]
				})),
				("eth_getLogs", i) => Ok(json!({
					"result": &receipts[i].logs
				})),
				_ => Err(TransportError::http(
					reqwest::StatusCode::METHOD_NOT_ALLOWED,
					"random.url".to_string(),
					"Unexpected method call".to_string(),
					None,
					None,
				)),
			}
		});

	mock_transport
}

fn make_monitor_with_events(mut monitor: Monitor, include_expression: bool) -> Monitor {
	monitor.match_conditions.functions = vec![];
	monitor.match_conditions.transactions = vec![];
	monitor.match_conditions.events = vec![];
	monitor.match_conditions.events.push(EventCondition {
		signature: "Transfer(address,address,uint256)".to_string(),
		expression: if include_expression {
			Some(
				"to == 0xf423d9c1ffeb6386639d024f3b241dab2331b635 AND from == \
				 0x58b704065b7aff3ed351052f8560019e05925023 AND value > 8000000000"
					.to_string(),
			)
		} else {
			None
		},
	});
	monitor
}

fn make_monitor_with_functions(mut monitor: Monitor, include_expression: bool) -> Monitor {
	monitor.match_conditions.events = vec![];
	monitor.match_conditions.transactions = vec![];
	monitor.match_conditions.functions = vec![];
	monitor.match_conditions.functions.push(FunctionCondition {
		signature: "transfer(address,uint256)".to_string(),
		expression: if include_expression {
			Some("value > 0".to_string())
		} else {
			None
		},
		internal: false,
	});
	monitor
}

fn make_monitor_with_transactions(mut monitor: Monitor, include_expression: bool) -> Monitor {
	monitor.match_conditions.events = vec![];
	monitor.match_conditions.functions = vec![];
	monitor.match_conditions.transactions = vec![];
	monitor
		.match_conditions
		.transactions
		.push(TransactionCondition {
			status: TransactionStatus::Success,
			expression: if include_expression {
				Some("value == 0".to_string())
			} else {
				None
			},
		});
	monitor
}

fn make_monitor_with_tuples_expression_equality(mut monitor: Monitor) -> Monitor {
	monitor.match_conditions.events = vec![];
	monitor.match_conditions.functions = vec![];
	monitor.match_conditions.transactions = vec![];
	monitor.match_conditions.functions.push(FunctionCondition {
		signature:
			"inputNestedStruct((bool,string,string,uint256,address,string[],(string,uint256)))"
				.to_string(),
		expression: Some("nestedStruct == '(true,\"The Book Title\",\"Author Name\",123,\"0x1234567890abcdef1234567890abcdef12345678\",[\"fiction\",\"bestseller\"],(\"The Sequel\",321))'".to_string()),
		internal: false,
	});
	monitor
}

#[tokio::test]
async fn test_monitor_events_with_no_expressions() -> Result<(), Box<FilterError>> {
	// Load test data using common utility
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();
	// Create mock transport
	let mock_transport = setup_mock_transport(test_data.clone());
	let client = EvmClient::new_with_transport(mock_transport);

	let monitor = make_monitor_with_events(test_data.monitor, false);

	// Run filter_block with the test data
	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[0],
			&[monitor],
			None,
		)
		.await?;

	assert!(!matches.is_empty(), "Should have found matching events");
	assert_eq!(matches.len(), 1, "Expected exactly one match");

	match &matches[0] {
		MonitorMatch::EVM(evm_match) => {
			assert!(evm_match.matched_on.events.len() == 1);
			assert!(evm_match.matched_on.functions.is_empty());
			assert!(evm_match.matched_on.transactions.is_empty());
			assert!(
				evm_match.matched_on.events[0].signature == "Transfer(address,address,uint256)"
			);

			let matched_on_args = evm_match.matched_on_args.as_ref().unwrap();
			assert!(
				!matched_on_args.events.as_ref().unwrap().is_empty(),
				"Expected events arguments to be matched"
			);
		}
		_ => {
			panic!("Expected EVM match");
		}
	}

	Ok(())
}

#[tokio::test]
async fn test_monitor_events_with_expressions() -> Result<(), Box<FilterError>> {
	// Load test data using common utility
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();
	// Create mock transport
	let mock_transport = setup_mock_transport(test_data.clone());
	let client = EvmClient::new_with_transport(mock_transport);

	let monitor = make_monitor_with_events(test_data.monitor, true);

	// Run filter_block with the test data
	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[0],
			&[monitor],
			None,
		)
		.await?;

	assert!(!matches.is_empty(), "Should have found matching events");
	assert_eq!(matches.len(), 1, "Expected exactly one match");

	match &matches[0] {
		MonitorMatch::EVM(evm_match) => {
			assert!(evm_match.matched_on.events.len() == 1);
			assert!(evm_match.matched_on.functions.is_empty());
			assert!(evm_match.matched_on.transactions.is_empty());
			assert!(
				evm_match.matched_on.events[0].signature == "Transfer(address,address,uint256)"
			);

			let matched_on_args = evm_match.matched_on_args.as_ref().unwrap();
			let event_args = &matched_on_args.events.as_ref().unwrap()[0];

			assert_eq!(event_args.signature, "Transfer(address,address,uint256)");
			assert_eq!(
				event_args.hex_signature.as_ref().unwrap(),
				"0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
			);

			// Assert the argument values
			let args = event_args.args.as_ref().unwrap();
			assert_eq!(args[0].name, "from");
			assert_eq!(args[0].value, "0x58b704065b7aff3ed351052f8560019e05925023");
			assert_eq!(args[0].kind, "address");
			assert!(args[0].indexed);

			assert_eq!(args[1].name, "to");
			assert_eq!(args[1].value, "0xf423d9c1ffeb6386639d024f3b241dab2331b635");
			assert_eq!(args[1].kind, "address");
			assert!(args[1].indexed);

			assert_eq!(args[2].name, "value");
			assert_eq!(args[2].value, "8181710000");
			assert_eq!(args[2].kind, "uint256");
			assert!(!args[2].indexed);
		}
		_ => {
			panic!("Expected EVM match");
		}
	}

	Ok(())
}

#[tokio::test]
async fn test_monitor_functions_with_no_expressions() -> Result<(), Box<FilterError>> {
	// Load test data using common utility
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();
	// Create mock transport
	let mock_transport = setup_mock_transport(test_data.clone());
	let client = EvmClient::new_with_transport(mock_transport);

	let monitor = make_monitor_with_functions(test_data.monitor, false);

	let contract_spec = test_data.contract_spec.unwrap();
	let contract_with_spec: (String, ContractSpec) = (
		"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".to_string(),
		contract_spec.clone(),
	);

	// Run filter_block with the test data
	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[0],
			&[monitor],
			Some(&[contract_with_spec]),
		)
		.await?;

	assert!(!matches.is_empty(), "Should have found matching functions");
	assert_eq!(matches.len(), 1, "Expected exactly one match");

	match &matches[0] {
		MonitorMatch::EVM(evm_match) => {
			assert!(evm_match.matched_on.functions.len() == 1);
			assert!(evm_match.matched_on.events.is_empty());
			assert!(evm_match.matched_on.transactions.is_empty());
			assert!(evm_match.matched_on.functions[0].signature == "transfer(address,uint256)");

			let matched_on_args = evm_match.matched_on_args.as_ref().unwrap();
			assert!(
				!matched_on_args.functions.as_ref().unwrap().is_empty(),
				"Expected functions arguments to be matched"
			);
		}
		_ => {
			panic!("Expected EVM match");
		}
	}

	Ok(())
}

#[tokio::test]
async fn test_monitor_functions_with_expressions() -> Result<(), Box<FilterError>> {
	// Load test data using common utility
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();
	// Create mock transport
	let mock_transport = setup_mock_transport(test_data.clone());
	let client = EvmClient::new_with_transport(mock_transport);

	let monitor = make_monitor_with_functions(test_data.monitor, true);

	let contract_spec = test_data.contract_spec.unwrap();
	let contract_with_spec: (String, ContractSpec) = (
		"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".to_string(),
		contract_spec.clone(),
	);

	// Run filter_block with the test data
	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[0],
			&[monitor],
			Some(&[contract_with_spec]),
		)
		.await?;

	assert!(!matches.is_empty(), "Should have found matching functions");
	assert_eq!(matches.len(), 1, "Expected exactly one match");

	match &matches[0] {
		MonitorMatch::EVM(evm_match) => {
			assert!(evm_match.matched_on.functions.len() == 1);
			assert!(evm_match.matched_on.events.is_empty());
			assert!(evm_match.matched_on.transactions.is_empty());
			assert!(evm_match.matched_on.functions[0].signature == "transfer(address,uint256)");

			let matched_on_args = evm_match.matched_on_args.as_ref().unwrap();
			let function_args = &matched_on_args.functions.as_ref().unwrap()[0];

			assert_eq!(function_args.signature, "transfer(address,uint256)");
			assert_eq!(function_args.hex_signature.as_ref().unwrap(), "0xa9059cbb");

			// Assert the argument values
			let args = function_args.args.as_ref().unwrap();

			assert_eq!(args[0].name, "to");
			assert_eq!(args[0].value, "0xf423d9c1ffeb6386639d024f3b241dab2331b635");
			assert_eq!(args[0].kind, "address");
			assert!(!args[0].indexed);

			assert_eq!(args[1].name, "value");
			assert_eq!(args[1].value, "8181710000");
			assert_eq!(args[1].kind, "uint256");
			assert!(!args[1].indexed);
		}
		_ => {
			panic!("Expected EVM match");
		}
	}

	Ok(())
}

#[tokio::test]
async fn test_monitor_transactions_with_no_expressions() -> Result<(), Box<FilterError>> {
	// Load test data using common utility
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();
	// Create mock transport
	let mock_transport = setup_mock_transport(test_data.clone());
	let client = EvmClient::new_with_transport(mock_transport);

	let monitor = make_monitor_with_transactions(test_data.monitor, false);

	// Run filter_block with the test data
	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[0],
			&[monitor],
			None,
		)
		.await?;

	assert!(
		!matches.is_empty(),
		"Should have found matching transactions"
	);
	assert_eq!(matches.len(), 1, "Expected exactly one match");

	match &matches[0] {
		MonitorMatch::EVM(evm_match) => {
			assert!(evm_match.matched_on.transactions.len() == 1);
			assert!(evm_match.matched_on.events.is_empty());
			assert!(evm_match.matched_on.functions.is_empty());
			assert!(evm_match.matched_on.transactions[0].status == TransactionStatus::Success);
			assert!(evm_match.matched_on.transactions[0].expression.is_none());
		}
		_ => {
			panic!("Expected EVM match");
		}
	}

	Ok(())
}

#[tokio::test]
async fn test_monitor_transactions_with_expressions() -> Result<(), Box<FilterError>> {
	// Load test data using common utility
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();
	// Create mock transport
	let mock_transport = setup_mock_transport(test_data.clone());
	let client = EvmClient::new_with_transport(mock_transport);

	let monitor = make_monitor_with_transactions(test_data.monitor, true);

	// Run filter_block with the test data
	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[0],
			&[monitor],
			None,
		)
		.await?;

	assert!(
		!matches.is_empty(),
		"Should have found matching transactions"
	);
	assert_eq!(matches.len(), 1, "Expected exactly one match");

	match &matches[0] {
		MonitorMatch::EVM(evm_match) => {
			assert!(evm_match.matched_on.transactions.len() == 1);
			assert!(evm_match.matched_on.events.is_empty());
			assert!(evm_match.matched_on.functions.is_empty());
			assert!(evm_match.matched_on.transactions[0].status == TransactionStatus::Success);
			assert!(
				evm_match.matched_on.transactions[0].expression == Some("value == 0".to_string())
			);
		}
		_ => {
			panic!("Expected EVM match");
		}
	}

	Ok(())
}

#[tokio::test]
async fn test_monitor_with_multiple_conditions() -> Result<(), Box<FilterError>> {
	// Load test data using common utility
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();
	// Create mock transport
	let mock_transport = setup_mock_transport(test_data.clone());
	let client = EvmClient::new_with_transport(mock_transport);

	let contract_spec = test_data.contract_spec.unwrap();
	let contract_with_spec: (String, ContractSpec) = (
		"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".to_string(),
		contract_spec.clone(),
	);

	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[0],
			&[test_data.monitor],
			Some(&[contract_with_spec]),
		)
		.await?;

	assert!(!matches.is_empty());

	if let MonitorMatch::EVM(evm_match) = &matches[0] {
		assert!(
			!evm_match.matched_on.events.is_empty(),
			"Should have matched events"
		);
		assert!(
			!evm_match.matched_on.functions.is_empty(),
			"Should have matched functions"
		);

		assert!(
			!evm_match.matched_on.transactions.is_empty(),
			"Should have matched transactions"
		);

		if let Some(args) = &evm_match.matched_on_args {
			if let Some(events) = &args.events {
				assert!(!events.is_empty(), "Should have event arguments");
				let event = &events[0];
				assert_eq!(event.signature, "Transfer(address,address,uint256)");
				assert_eq!(
					event.hex_signature.as_ref().unwrap(),
					"0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
				);
			}

			if let Some(functions) = &args.functions {
				assert!(!functions.is_empty(), "Should have function arguments");
				let function = &functions[0];
				assert_eq!(function.signature, "transfer(address,uint256)");
				assert_eq!(function.hex_signature.as_ref().unwrap(), "0xa9059cbb");
			}
		}
	}

	Ok(())
}

#[tokio::test]
async fn test_monitor_error_cases() -> Result<(), Box<FilterError>> {
	// Load test data using common utility
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();
	let client = EvmClient::new(&test_data.network).await.unwrap();

	// Create an invalid block type
	let invalid_block = BlockType::Stellar(Box::default());

	let result = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&invalid_block,
			&[test_data.monitor],
			None,
		)
		.await;

	assert!(result.is_err());
	assert!(matches!(
		result.unwrap_err(),
		FilterError::BlockTypeMismatch { .. }
	));

	Ok(())
}

#[tokio::test]
async fn test_handle_match() -> Result<(), Box<FilterError>> {
	// Load test data using common utility
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();
	// Create mock transport
	let mock_transport = setup_mock_transport(test_data.clone());
	let client = EvmClient::new_with_transport(mock_transport);
	let trigger_scripts = HashMap::new();

	let mut trigger_execution_service =
		setup_trigger_execution_service("tests/integration/fixtures/evm/triggers/trigger.json")
			.await;

	let contract_spec = test_data.contract_spec.unwrap();
	let contract_with_spec: (String, ContractSpec) = (
		"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".to_string(),
		contract_spec.clone(),
	);

	// Set up expectations for execute()
	trigger_execution_service.expect_execute()
		.withf(|trigger_name, variables, _monitor_match, _trigger_scripts| {
			trigger_name == ["example_trigger_slack"]
				// Event variables
				&& variables.get("events.0.signature") == Some(&"Transfer(address,address,uint256)".to_string())
				&& variables.get("events.0.args.from") == Some(&"0x58b704065b7aff3ed351052f8560019e05925023".to_string())
				&& variables.get("events.0.args.to") == Some(&"0xf423d9c1ffeb6386639d024f3b241dab2331b635".to_string())
				&& variables.get("events.0.args.value") == Some(&"8181710000".to_string())
				// Function variables
				&& variables.get("functions.0.signature") == Some(&"transfer(address,uint256)".to_string())
				&& variables.get("functions.0.args.to") == Some(&"0xf423d9c1ffeb6386639d024f3b241dab2331b635".to_string())
				&& variables.get("functions.0.args.value") == Some(&"8181710000".to_string())
				// Transaction variables
				&& variables.get("transaction.hash") == Some(&"0xd5069b22a3a89a36d592d5a1f72a281bc5d11d6d0bac6f0a878c13abb764b6d8".to_string())
				&& variables.get("transaction.from") == Some(&"0x58b704065b7aff3ed351052f8560019e05925023".to_string())
				&& variables.get("transaction.to") == Some(&"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".to_string())
				&& variables.get("transaction.value") == Some(&"0".to_string())
				// Monitor metadata
				&& variables.get("monitor.name") == Some(&"Mint USDC Token".to_string())
		})
		.once()
		.returning(|_, _, _, _| Ok(()));

	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[0],
			&[test_data.monitor],
			Some(&[contract_with_spec]),
		)
		.await?;

	assert!(!matches.is_empty(), "Should have found matches to handle");

	for matching_monitor in matches {
		let result = handle_match(
			matching_monitor.clone(),
			&trigger_execution_service,
			&trigger_scripts,
		)
		.await;
		assert!(result.is_ok(), "Handle match should succeed");
	}

	Ok(())
}

#[tokio::test]
async fn test_handle_match_with_no_args() -> Result<(), Box<FilterError>> {
	// Load test data using common utility
	let mut test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();

	// only keep the last receipt with increment() transaction
	test_data.receipts = vec![test_data.receipts.last().unwrap().clone()];

	// Create mock transport
	let mock_transport = setup_mock_transport(test_data.clone());
	let client = EvmClient::new_with_transport(mock_transport);

	let mut monitor = test_data.monitor;
	// Clear existing conditions and add functions without arguments
	monitor.match_conditions.functions = vec![FunctionCondition {
		signature: "increment()".to_string(),
		expression: None,
		internal: false,
	}];
	monitor.match_conditions.events = vec![];
	monitor.match_conditions.transactions = vec![];

	let contract_spec = test_data.contract_spec.unwrap();
	let contract_with_spec: (String, ContractSpec) = (
		"0xf18206b2289cf6ce15cddbee9c6f6a0f059efb56".to_string(),
		contract_spec.clone(),
	);

	// Run filter_block with the test data
	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[3], // block at index 3 contains increment() transaction
			&[monitor],
			Some(&[contract_with_spec]),
		)
		.await?;

	assert!(!matches.is_empty(), "Should have found matches");
	assert_eq!(matches.len(), 1, "Expected exactly one match");

	match &matches[0] {
		MonitorMatch::EVM(evm_match) => {
			assert!(evm_match.matched_on.functions.len() == 1);
			assert!(evm_match.matched_on.events.is_empty());
			assert!(evm_match.matched_on.transactions.is_empty());
			assert!(evm_match.matched_on.functions[0].signature == "increment()");

			// Now test handle_match to verify the data map contains signatures
			let trigger_scripts = HashMap::new();
			let mut trigger_execution_service = setup_trigger_execution_service(
				"tests/integration/fixtures/evm/triggers/trigger.json",
			)
			.await;

			// Set up expectations for execute()
			trigger_execution_service
				.expect_execute()
				.withf(|trigger_name, variables, _monitor_match, _trigger_scripts| {
					trigger_name == ["example_trigger_slack"]
						// Monitor metadata
						&& variables.get("monitor.name") == Some(&"Mint USDC Token".to_string())
						// Transaction variables
						&& variables.get("transaction.hash") == Some(&"0x6fb716f3fc4e2edec31f01c8bb67e565e3efacba965090a38835d3f297232bf6".to_string())
						&& variables.get("transaction.from") == Some(&"0x6b9501462d48f7e78ba11c98508ee16d29a03411".to_string())
						&& variables.get("transaction.to") == Some(&"0xf18206b2289cf6ce15cddbee9c6f6a0f059efb56".to_string())
						&& variables.get("transaction.value") == Some(&"0".to_string())
						// Function signature should be present even without args
						&& variables.get("functions.0.signature") == Some(&"increment()".to_string())
				})
				.once()
				.returning(|_, _, _, _| Ok(()));

			let result = handle_match(
				matches[0].clone(),
				&trigger_execution_service,
				&trigger_scripts,
			)
			.await;
			assert!(result.is_ok(), "Handle match should succeed");
		}
		_ => {
			panic!("Expected EVM match");
		}
	}

	Ok(())
}

#[tokio::test]
async fn test_handle_match_with_key_collision() -> Result<(), Box<FilterError>> {
	// Load test data using common utility
	let test_data = TestDataBuilder::new("evm").build();

	// Setup trigger execution service and capture the data structure
	let data_capture = std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()));
	let data_capture_clone = data_capture.clone();

	let mut trigger_execution_service =
		setup_trigger_execution_service("tests/integration/fixtures/evm/triggers/trigger.json")
			.await;

	// Set up expectations for execute() with custom function to capture and verify data
	trigger_execution_service
		.expect_execute()
		.withf(
			move |_triggers, variables, _monitor_match, _trigger_scripts| {
				let mut captured = data_capture_clone.lock().unwrap();
				*captured = variables.clone();
				true
			},
		)
		.returning(|_, _, _, _| Ok(()));

	// Create a monitor match with an argument named "signature"
	use openzeppelin_monitor::models::{
		EVMMatchArguments, EVMMatchParamEntry, EVMMatchParamsMap, EVMMonitorMatch,
		FunctionCondition, MatchConditions,
	};

	// Create test monitor with a function that has an argument called "signature"
	let mut monitor = test_data.monitor.clone();
	monitor.match_conditions.functions = vec![FunctionCondition {
		signature: "dangerousFunc(bytes32 signature, uint256 value)".to_string(),
		expression: None,
		internal: false,
	}];

	fn create_test_evm_transaction_receipt() -> EVMTransactionReceipt {
		ReceiptBuilder::new().build()
	}

	fn create_test_evm_logs() -> Vec<EVMReceiptLog> {
		ReceiptBuilder::new().build().logs.clone()
	}

	// Create a match object
	let evm_match = EVMMonitorMatch {
		monitor,
		transaction: TransactionBuilder::new().build(),
		receipt: Some(create_test_evm_transaction_receipt()),
		logs: Some(create_test_evm_logs()),
		network_slug: "ethereum_mainnet".to_string(),
		matched_on: MatchConditions {
			functions: vec![FunctionCondition {
				signature: "dangerousFunc(bytes32 signature, uint256 value)".to_string(),
				expression: None,
				internal: false,
			}],
			events: vec![],
			transactions: vec![],
		},
		matched_on_args: Some(EVMMatchArguments {
			functions: Some(vec![EVMMatchParamsMap {
				signature: "dangerousFunc(bytes32 signature, uint256 value)".to_string(),
				args: Some(vec![
					EVMMatchParamEntry {
						name: "signature".to_string(),
						value: "0xabcdef1234567890".to_string(),
						kind: "bytes32".to_string(),
						indexed: false,
					},
					EVMMatchParamEntry {
						name: "value".to_string(),
						value: "123456789".to_string(),
						kind: "uint256".to_string(),
						indexed: false,
					},
				]),
				hex_signature: Some("0xdeadbeef".to_string()),
				caller: None,
				target: None,
			}]),
			events: None,
		}),
	};

	let match_wrapper = MonitorMatch::EVM(Box::new(evm_match));

	// Process the match directly using handle_match
	let result = handle_match(match_wrapper, &trigger_execution_service, &HashMap::new()).await;
	assert!(result.is_ok(), "Handle match should succeed");

	// Verify that data structure preserves both function signature and argument
	let captured_data = data_capture.lock().unwrap();

	// The key for the function signature should exist
	assert!(
		captured_data.contains_key("functions.0.signature"),
		"functions.0.signature should exist in the data structure"
	);

	// Check the value is correct
	assert_eq!(
		captured_data.get("functions.0.signature").unwrap(),
		"dangerousFunc(bytes32 signature, uint256 value)",
		"Function signature value should be preserved"
	);

	// The key for the argument should also exist
	assert!(
		captured_data.contains_key("functions.0.args.signature"),
		"functions.0.args.signature should exist in the data structure"
	);

	// Check that the argument value is correct
	assert_eq!(
		captured_data.get("functions.0.args.signature").unwrap(),
		"0xabcdef1234567890",
		"Function argument value should be correct"
	);

	// Verify that the values are different - no collision
	assert_ne!(
		captured_data.get("functions.0.signature").unwrap(),
		captured_data.get("functions.0.args.signature").unwrap(),
		"Function signature and argument values should be distinct"
	);

	// Also check for other expected fields
	assert!(
		captured_data.contains_key("transaction.hash"),
		"Transaction hash should be present"
	);
	assert!(
		captured_data.contains_key("monitor.name"),
		"Monitor name should be present"
	);

	Ok(())
}

#[tokio::test]
async fn test_filter_block_with_receipt_and_logs() -> Result<(), Box<FilterError>> {
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();
	let mock_transport = setup_mock_transport(test_data.clone());

	let client = EvmClient::new_with_transport(mock_transport);

	let mut monitor = test_data.monitor;
	monitor.match_conditions.events = vec![EventCondition {
		signature: "Transfer(address,address,uint256)".to_string(),
		expression: None,
	}];
	monitor.match_conditions.functions = vec![FunctionCondition {
		signature: "transfer(address,uint256)".to_string(),
		expression: None,
		internal: false,
	}];
	monitor.match_conditions.transactions = vec![TransactionCondition {
		status: TransactionStatus::Any,
		expression: Some("gas_used > 0".to_string()), // This is a test to ensure that the receipt is required
	}];

	let contract_spec = test_data.contract_spec.unwrap();
	let contract_with_spec: (String, ContractSpec) = (
		"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".to_string(),
		contract_spec.clone(),
	);

	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[0],
			&[monitor],
			Some(&[contract_with_spec]),
		)
		.await?;

	assert!(!matches.is_empty(), "Should have found matches");
	assert_eq!(matches.len(), 1, "Expected exactly one match");

	match &matches[0] {
		MonitorMatch::EVM(evm_match) => {
			assert!(
				evm_match.receipt.is_some(),
				"Transaction receipt should be present"
			);
			let receipt = evm_match.receipt.as_ref().unwrap();
			assert!(
				receipt.status.is_some(),
				"Transaction status should be present"
			);
			assert_eq!(
				receipt.status.unwrap(),
				Uint::from(1),
				"Transaction should be successful"
			);
			assert!(receipt.gas_used.is_some(), "Gas used should be present");
			assert!(
				receipt.gas_used.unwrap() > Uint::from(0),
				"Gas used should be greater than 0"
			);

			assert!(
				evm_match.logs.is_some(),
				"Transaction logs should be present"
			);
			let logs = evm_match.logs.as_ref().unwrap();
			assert!(!logs.is_empty(), "Should have at least one log entry");

			assert!(
				!evm_match.matched_on.events.is_empty(),
				"Should have matched events"
			);
			assert!(
				!evm_match.matched_on.functions.is_empty(),
				"Should have matched functions"
			);

			let matched_on_args = evm_match.matched_on_args.as_ref().unwrap();

			let event_args = &matched_on_args.events.as_ref().unwrap()[0];
			assert_eq!(event_args.signature, "Transfer(address,address,uint256)");
			assert_eq!(
				event_args.hex_signature.as_ref().unwrap(),
				"0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
			);

			let function_args = &matched_on_args.functions.as_ref().unwrap()[0];
			assert_eq!(function_args.signature, "transfer(address,uint256)");
			assert_eq!(function_args.hex_signature.as_ref().unwrap(), "a9059cbb");
		}
		_ => {
			panic!("Expected EVM match");
		}
	}

	Ok(())
}

#[tokio::test]
async fn test_filter_block_with_only_logs() -> Result<(), Box<FilterError>> {
	// Load test data using common utility
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();
	// Create mock transport
	let mock_transport = setup_mock_transport(test_data.clone());

	let client = EvmClient::new_with_transport(mock_transport);

	let mut monitor = test_data.monitor;
	monitor.match_conditions.events = vec![EventCondition {
		signature: "Transfer(address,address,uint256)".to_string(),
		expression: None,
	}];
	monitor.match_conditions.functions = vec![FunctionCondition {
		signature: "transfer(address,uint256)".to_string(),
		expression: None,
		internal: false,
	}];
	monitor.match_conditions.transactions = vec![]; // This ensures we do not need a receipt

	let contract_spec = test_data.contract_spec.unwrap();
	let contract_with_spec: (String, ContractSpec) = (
		"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".to_string(),
		contract_spec.clone(),
	);

	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[0],
			&[monitor],
			Some(&[contract_with_spec]),
		)
		.await?;

	assert!(!matches.is_empty(), "Should have found matches");
	assert_eq!(matches.len(), 1, "Expected exactly one match");

	match &matches[0] {
		MonitorMatch::EVM(evm_match) => {
			assert!(
				evm_match.receipt.is_none(),
				"Transaction receipt should not be present"
			);

			assert!(
				evm_match.logs.is_some(),
				"Transaction logs should be present"
			);
			let logs = evm_match.logs.as_ref().unwrap();
			assert!(!logs.is_empty(), "Should have at least one log entry");

			assert!(
				!evm_match.matched_on.events.is_empty(),
				"Should have matched events"
			);
			assert!(
				!evm_match.matched_on.functions.is_empty(),
				"Should have matched functions"
			);

			let matched_on_args = evm_match.matched_on_args.as_ref().unwrap();

			let event_args = &matched_on_args.events.as_ref().unwrap()[0];
			assert_eq!(event_args.signature, "Transfer(address,address,uint256)");
			assert_eq!(
				event_args.hex_signature.as_ref().unwrap(),
				"0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
			);

			// Check function arguments
			let function_args = &matched_on_args.functions.as_ref().unwrap()[0];
			assert_eq!(function_args.signature, "transfer(address,uint256)");
			assert_eq!(function_args.hex_signature.as_ref().unwrap(), "a9059cbb");
		}
		_ => {
			panic!("Expected EVM match");
		}
	}

	Ok(())
}

#[tokio::test]
async fn test_filter_block_needs_receipt_for_status() -> Result<(), Box<FilterError>> {
	// Load test data using common utility
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();

	let mut mock_transport = MockEVMTransportClient::new();
	let counter = std::sync::atomic::AtomicUsize::new(0);
	let receipts = test_data.receipts;

	mock_transport
		.expect_send_raw_request()
		.returning(move |method, _params| {
			let current = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
			match (method, current) {
				("net_version", _) => Ok(json!({"result": "1"})),
				("eth_getTransactionReceipt", i) => Ok(json!({
					"result": &receipts[i]
				})),
				("eth_getLogs", _) => Ok(json!({
					"result": Vec::<EVMReceiptLog>::new()
				})),
				_ => Err(TransportError::http(
					reqwest::StatusCode::METHOD_NOT_ALLOWED,
					"random.url".to_string(),
					"Unexpected method call".to_string(),
					None,
					None,
				)),
			}
		});

	let client = EvmClient::new_with_transport(mock_transport);

	let mut monitor = test_data.monitor;
	monitor.match_conditions.events = vec![];
	monitor.match_conditions.functions = vec![FunctionCondition {
		signature: "transfer(address,uint256)".to_string(),
		expression: None,
		internal: false,
	}];
	monitor.match_conditions.transactions = vec![TransactionCondition {
		status: TransactionStatus::Success, // This is to ensure that the receipt is required
		expression: None,
	}];

	let contract_spec = test_data.contract_spec.unwrap();
	let contract_with_spec: (String, ContractSpec) = (
		"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".to_string(),
		contract_spec.clone(),
	);

	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[0],
			&[monitor],
			Some(&[contract_with_spec]),
		)
		.await?;

	assert!(!matches.is_empty(), "Should have found matches");
	assert_eq!(matches.len(), 1, "Expected exactly one match");

	match &matches[0] {
		MonitorMatch::EVM(evm_match) => {
			assert!(
				evm_match.receipt.is_some(),
				"Transaction receipt should be present"
			);
			let receipt = evm_match.receipt.as_ref().unwrap();
			assert!(
				receipt.status.is_some(),
				"Transaction status should be present"
			);
			assert_eq!(
				receipt.status.unwrap(),
				Uint::from(1),
				"Transaction should be successful"
			);
			assert!(receipt.gas_used.is_some(), "Gas used should be present");
			assert!(
				receipt.gas_used.unwrap() > Uint::from(0),
				"Gas used should be greater than 0"
			);

			assert!(
				evm_match.logs.as_ref().unwrap().is_empty(),
				"Transaction logs should be empty"
			);

			assert!(
				evm_match.matched_on.events.is_empty(),
				"Should not have matched events"
			);
			assert!(
				!evm_match.matched_on.functions.is_empty(),
				"Should have matched functions"
			);

			let matched_on_args = evm_match.matched_on_args.as_ref().unwrap();

			let function_args = &matched_on_args.functions.as_ref().unwrap()[0];
			assert_eq!(function_args.signature, "transfer(address,uint256)");
			assert_eq!(function_args.hex_signature.as_ref().unwrap(), "a9059cbb");
		}
		_ => {
			panic!("Expected EVM match");
		}
	}

	Ok(())
}

#[tokio::test]
async fn test_filter_block_with_tuples_contains_expression() -> Result<(), Box<FilterError>> {
	// Load test data using common utility
	let test_data = TestDataBuilder::new("evm")
		.with_monitor("monitor_with_tuples.json")
		.with_contract_spec("contract_spec_tuples.json")
		.build();

	let filter_service = FilterService::new();

	// Create mock transport
	let mock_transport = setup_mock_transport(test_data.clone());
	let client = EvmClient::new_with_transport(mock_transport);

	let mut monitor = test_data.monitor;
	monitor.match_conditions.events = vec![];
	monitor.match_conditions.functions = vec![FunctionCondition {
		signature:
			"inputNestedStruct((bool,string,string,uint256,address,string[],(string,uint256)))"
				.to_string(),
		expression: Some("nestedStruct contains 'The Book Title'".to_string()),
		internal: false,
	}];
	monitor.match_conditions.transactions = vec![];

	let contract_spec = test_data.contract_spec.unwrap();
	let contract_with_spec: (String, ContractSpec) = (
		"0x774283785a2e4845E7dF23e18af5849c3D23722f".to_string(),
		contract_spec.clone(),
	);

	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			test_data.blocks.last().unwrap(),
			&[monitor],
			Some(&[contract_with_spec]),
		)
		.await?;

	assert!(!matches.is_empty(), "Should have found matches");
	assert_eq!(matches.len(), 1, "Expected exactly one match");

	match &matches[0] {
		MonitorMatch::EVM(evm_match) => {
			assert!(
				evm_match.receipt.is_none(),
				"Transaction receipt should not be present"
			);
			assert!(
				evm_match.logs.is_some(),
				"Transaction logs should be present"
			);
			assert!(
				evm_match.matched_on.events.is_empty(),
				"Should not have matched events"
			);
			assert!(
				!evm_match.matched_on.functions.is_empty(),
				"Should have matched functions"
			);
			assert!(
				evm_match.matched_on_args.is_some(),
				"Should have matched on args"
			);
			let matched_on_args = evm_match.matched_on_args.as_ref().unwrap();
			let function_args = &matched_on_args.functions.as_ref().unwrap()[0];
			assert_eq!(
				function_args.signature,
				"inputNestedStruct((bool,string,string,uint256,address,string[],(string,uint256)))"
			);
		}
		_ => panic!("Expected EVM match"),
	}

	Ok(())
}

#[tokio::test]
async fn test_filter_block_with_tuples_expression_equality() -> Result<(), Box<FilterError>> {
	// Load test data using common utility
	let test_data = TestDataBuilder::new("evm")
		.with_monitor("monitor_with_tuples.json")
		.with_contract_spec("contract_spec_tuples.json")
		.build();

	let filter_service = FilterService::new();

	// Create mock transport
	let mock_transport = setup_mock_transport(test_data.clone());
	let client = EvmClient::new_with_transport(mock_transport);

	let mut monitor = make_monitor_with_tuples_expression_equality(test_data.monitor);
	monitor.match_conditions.events = vec![];
	monitor.match_conditions.functions = vec![FunctionCondition {
		signature:
			"inputNestedStruct((bool,string,string,uint256,address,string[],(string,uint256)))"
				.to_string(),
		expression: Some("nestedStruct == '(true,\"The Book Title\",\"Author Name\",123,\"0x1234567890abcdef1234567890abcdef12345678\",[\"fiction\",\"bestseller\"],(\"The Sequel\",321))'".to_string()),
		internal: false,
	}];
	monitor.match_conditions.transactions = vec![];

	let contract_spec = test_data.contract_spec.unwrap();
	let contract_with_spec: (String, ContractSpec) = (
		"0x774283785a2e4845E7dF23e18af5849c3D23722f".to_string(),
		contract_spec.clone(),
	);

	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			test_data.blocks.last().unwrap(),
			&[monitor],
			Some(&[contract_with_spec]),
		)
		.await?;

	assert!(!matches.is_empty(), "Should have found matches");
	assert_eq!(matches.len(), 1, "Expected exactly one match");

	match &matches[0] {
		MonitorMatch::EVM(evm_match) => {
			assert!(
				evm_match.receipt.is_none(),
				"Transaction receipt should not be present"
			);
			assert!(
				evm_match.logs.is_some(),
				"Transaction logs should be present"
			);
			assert!(
				evm_match.matched_on.events.is_empty(),
				"Should not have matched events"
			);
			assert!(
				!evm_match.matched_on.functions.is_empty(),
				"Should have matched functions"
			);
			assert!(
				evm_match.matched_on_args.is_some(),
				"Should have matched on args"
			);
			let matched_on_args = evm_match.matched_on_args.as_ref().unwrap();
			let function_args = &matched_on_args.functions.as_ref().unwrap()[0];
			assert_eq!(
				function_args.signature,
				"inputNestedStruct((bool,string,string,uint256,address,string[],(string,uint256)))"
			);
		}
		_ => panic!("Expected EVM match"),
	}

	Ok(())
}

// ============================================================================
// Internal call matching tests
// ============================================================================

/// Helper: builds a monitor with a single `internal: true` function condition for mint(address,uint256).
/// Uses the USDC address from the standard fixture as the monitored address.
fn make_monitor_with_internal_mint(
	monitor: &Monitor,
	include_expression: bool,
) -> Monitor {
	let mut m = monitor.clone();
	m.match_conditions.events = vec![];
	m.match_conditions.transactions = vec![];
	m.match_conditions.functions = vec![FunctionCondition {
		signature: "mint(address,uint256)".to_string(),
		expression: if include_expression {
			Some("_amount > 1000".to_string())
		} else {
			None
		},
		internal: true,
	}];
	m
}

/// Build a CallTrace JSON value representing a 3-deep call tree where
/// a mint(address,uint256) call to the USDC address occurs at depth 3.
///
/// Tree shape:
///   root (CALL from sender -> USDC with approve calldata)
///     -> depth1 (CALL from USDC -> intermediary)
///       -> depth2 (CALL from intermediary -> USDC  with mint calldata)
fn build_mint_trace_json() -> serde_json::Value {
	// mint(address,uint256) ABI-encoded calldata
	// selector 0x40c10f19, _to = 0xf423...b635, _amount = 5000000
	let mint_input = "0x40c10f19\
		000000000000000000000000f423d9c1ffeb6386639d024f3b241dab2331b635\
		00000000000000000000000000000000000000000000000000000000004c4b40";

	json!({
		"type": "CALL",
		"from": "0x58b704065b7aff3ed351052f8560019e05925023",
		"to": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
		"gas": "0x30000",
		"gasUsed": "0x10000",
		"input": "0x095ea7b3000000000000000000000000f423d9c1ffeb6386639d024f3b241dab2331b635ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
		"output": "0x0000000000000000000000000000000000000000000000000000000000000001",
		"calls": [
			{
				"type": "CALL",
				"from": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
				"to": "0x1111111111111111111111111111111111111111",
				"gas": "0x20000",
				"gasUsed": "0x8000",
				"input": "0xdeadbeef",
				"output": "0x",
				"calls": [
					{
						"type": "CALL",
						"from": "0x1111111111111111111111111111111111111111",
						"to": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
						"gas": "0x10000",
						"gasUsed": "0x4000",
						"input": mint_input,
						"output": "0x0000000000000000000000000000000000000000000000000000000000000001"
					}
				]
			}
		]
	})
}

/// Creates a mock transport that dispatches on method name.
/// Handles eth_getLogs (empty), and optionally debug_traceTransaction (returns provided trace).
/// Tracks whether debug_traceTransaction was called via the `trace_called` flag.
fn setup_mock_transport_for_internal_calls(
	trace_json: Option<serde_json::Value>,
	trace_called: Arc<AtomicBool>,
) -> MockEVMTransportClient {
	let mut mock_transport = MockEVMTransportClient::new();

	mock_transport
		.expect_send_raw_request()
		.returning(move |method, _params| {
			match method {
				"net_version" => Ok(json!({"result": "1"})),
				"eth_getLogs" => Ok(json!({"result": []})),
				"debug_traceTransaction" => {
					trace_called.store(true, Ordering::SeqCst);
					match &trace_json {
						Some(trace) => Ok(json!({"result": trace})),
						None => Err(TransportError::http(
							reqwest::StatusCode::METHOD_NOT_ALLOWED,
							"random.url".to_string(),
							"debug_traceTransaction not expected".to_string(),
							None,
							None,
						)),
					}
				}
				_ => Err(TransportError::http(
					reqwest::StatusCode::METHOD_NOT_ALLOWED,
					"random.url".to_string(),
					format!("Unexpected method: {}", method),
					None,
					None,
				)),
			}
		});

	mock_transport
}

#[tokio::test]
async fn test_internal_function_matching() -> Result<(), Box<FilterError>> {
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();

	let trace_called = Arc::new(AtomicBool::new(false));
	let mock_transport = setup_mock_transport_for_internal_calls(
		Some(build_mint_trace_json()),
		trace_called.clone(),
	);
	let client = EvmClient::new_with_transport(mock_transport);

	let monitor = make_monitor_with_internal_mint(&test_data.monitor, false);

	// Use the monitor's embedded ABI (which includes mint) rather than the
	// standalone contract_spec.json (which only has transfer/increment)
	let usdc_address = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
	let contract_spec = test_data
		.monitor
		.addresses
		.iter()
		.find(|a| a.address.to_lowercase() == usdc_address)
		.expect("USDC address in monitor")
		.contract_spec
		.clone()
		.expect("USDC ABI present");
	let contract_with_spec: (String, ContractSpec) = (
		usdc_address.to_string(),
		contract_spec,
	);

	// Use block[0] which has a TX to the USDC address
	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[0],
			&[monitor],
			Some(&[contract_with_spec]),
		)
		.await?;

	// debug_traceTransaction should have been called
	assert!(
		trace_called.load(Ordering::SeqCst),
		"debug_traceTransaction should be called for internal: true conditions"
	);

	assert!(!matches.is_empty(), "Should have found internal function match");

	// Find the match that contains the mint function
	let mint_match = matches.iter().find(|m| {
		if let MonitorMatch::EVM(evm) = m {
			evm.matched_on
				.functions
				.iter()
				.any(|f| f.signature.contains("mint"))
		} else {
			false
		}
	});

	assert!(mint_match.is_some(), "Should have found a mint function match");

	if let MonitorMatch::EVM(evm_match) = mint_match.unwrap() {
		// Verify the matched function condition
		let mint_fn = evm_match
			.matched_on
			.functions
			.iter()
			.find(|f| f.signature.contains("mint"))
			.unwrap();
		assert_eq!(mint_fn.signature, "mint(address,uint256)");
		assert!(mint_fn.internal, "Matched condition should have internal: true");

		// Verify matched_on_args contains decoded parameters
		let matched_on_args = evm_match.matched_on_args.as_ref().unwrap();
		let functions = matched_on_args.functions.as_ref().unwrap();
		let mint_args = functions
			.iter()
			.find(|f| f.signature.contains("mint"))
			.unwrap();
		assert_eq!(mint_args.signature, "mint(address,uint256)");

		let args = mint_args.args.as_ref().unwrap();
		assert_eq!(args.len(), 2, "mint has 2 parameters");

		// First param: _to (address)
		assert_eq!(args[0].name, "_to");
		assert_eq!(args[0].kind, "address");
		assert_eq!(
			args[0].value.to_lowercase(),
			"0xf423d9c1ffeb6386639d024f3b241dab2331b635"
		);

		// Second param: _amount (uint256) = 5000000
		assert_eq!(args[1].name, "_amount");
		assert_eq!(args[1].kind, "uint256");
		assert_eq!(args[1].value, "5000000");

		// Verify hex_signature is present
		assert!(mint_args.hex_signature.is_some());
		assert_eq!(mint_args.hex_signature.as_ref().unwrap(), "0x40c10f19");

		// Verify caller and target are set for internal call matches
		assert!(
			mint_args.caller.is_some(),
			"caller should be set for internal call match"
		);
		assert_eq!(
			mint_args.caller.as_ref().unwrap().to_lowercase(),
			"0x1111111111111111111111111111111111111111",
			"caller should be the contract that called mint()"
		);
		assert!(
			mint_args.target.is_some(),
			"target should be set for internal call match"
		);
		assert_eq!(
			mint_args.target.as_ref().unwrap().to_lowercase(),
			"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
			"target should be the monitored address (USDC)"
		);
	}

	Ok(())
}

#[tokio::test]
async fn test_no_trace_when_no_internal_conditions() -> Result<(), Box<FilterError>> {
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();

	let trace_called = Arc::new(AtomicBool::new(false));
	// Pass None for trace_json -- if debug_traceTransaction is called, it will error
	let mock_transport = setup_mock_transport_for_internal_calls(
		None,
		trace_called.clone(),
	);
	let client = EvmClient::new_with_transport(mock_transport);

	// Standard function condition (internal: false) -- should NOT trigger trace
	let mut monitor = test_data.monitor.clone();
	monitor.match_conditions.events = vec![];
	monitor.match_conditions.transactions = vec![];
	monitor.match_conditions.functions = vec![FunctionCondition {
		signature: "transfer(address,uint256)".to_string(),
		expression: None,
		internal: false,
	}];

	let contract_spec = test_data.contract_spec.unwrap();
	let contract_with_spec: (String, ContractSpec) = (
		"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".to_string(),
		contract_spec.clone(),
	);

	let _matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[0],
			&[monitor],
			Some(&[contract_with_spec]),
		)
		.await?;

	// debug_traceTransaction should NOT have been called
	assert!(
		!trace_called.load(Ordering::SeqCst),
		"debug_traceTransaction should NOT be called when no internal: true conditions exist"
	);

	Ok(())
}

#[tokio::test]
async fn test_internal_function_matching_with_expression() -> Result<(), Box<FilterError>> {
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();

	let trace_called = Arc::new(AtomicBool::new(false));
	let mock_transport = setup_mock_transport_for_internal_calls(
		Some(build_mint_trace_json()),
		trace_called.clone(),
	);
	let client = EvmClient::new_with_transport(mock_transport);

	// Monitor with expression: _amount > 1000
	// The trace has _amount = 5000000, so this should match
	let monitor = make_monitor_with_internal_mint(&test_data.monitor, true);

	// Use the monitor's embedded ABI (which includes mint)
	let usdc_address = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
	let contract_spec = test_data
		.monitor
		.addresses
		.iter()
		.find(|a| a.address.to_lowercase() == usdc_address)
		.expect("USDC address in monitor")
		.contract_spec
		.clone()
		.expect("USDC ABI present");
	let contract_with_spec: (String, ContractSpec) = (
		usdc_address.to_string(),
		contract_spec,
	);

	let matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[0],
			&[monitor.clone()],
			Some(&[contract_with_spec.clone()]),
		)
		.await?;

	assert!(
		trace_called.load(Ordering::SeqCst),
		"debug_traceTransaction should be called"
	);

	// Find the mint match
	let has_mint = matches.iter().any(|m| {
		if let MonitorMatch::EVM(evm) = m {
			evm.matched_on
				.functions
				.iter()
				.any(|f| f.signature.contains("mint"))
		} else {
			false
		}
	});
	assert!(has_mint, "Should match: _amount (5000000) > 1000");

	// Now test with expression that should NOT match: _amount > 10000000
	let trace_called2 = Arc::new(AtomicBool::new(false));
	let mock_transport2 = setup_mock_transport_for_internal_calls(
		Some(build_mint_trace_json()),
		trace_called2.clone(),
	);
	let client2 = EvmClient::new_with_transport(mock_transport2);

	let mut monitor_no_match = monitor.clone();
	monitor_no_match.match_conditions.functions = vec![FunctionCondition {
		signature: "mint(address,uint256)".to_string(),
		expression: Some("_amount > 10000000".to_string()),
		internal: true,
	}];

	let matches2 = filter_service
		.filter_block(
			&client2,
			&test_data.network,
			&test_data.blocks[0],
			&[monitor_no_match],
			Some(&[contract_with_spec]),
		)
		.await?;

	assert!(
		trace_called2.load(Ordering::SeqCst),
		"debug_traceTransaction should still be called"
	);

	// The expression _amount > 10000000 should NOT match since _amount = 5000000
	let has_mint2 = matches2.iter().any(|m| {
		if let MonitorMatch::EVM(evm) = m {
			evm.matched_on
				.functions
				.iter()
				.any(|f| f.signature.contains("mint"))
		} else {
			false
		}
	});
	assert!(
		!has_mint2,
		"Should NOT match: _amount (5000000) is not > 10000000"
	);

	Ok(())
}

/// Verifies that a monitor with only event conditions (no function conditions)
/// does NOT call debug_traceTransaction, even if internal calls exist in the
/// block's transactions.
///
/// This is the expected behavior for L2 system contract monitors (e.g.,
/// ContractDeployer, L2BaseToken) which watch for events only. Adding
/// `internal: true` to a function condition triggers tracing, but having
/// no function conditions at all means tracing is skipped entirely.
#[tokio::test]
async fn test_no_trace_when_only_event_conditions() -> Result<(), Box<FilterError>> {
	let test_data = TestDataBuilder::new("evm").build();
	let filter_service = FilterService::new();

	let trace_called = Arc::new(AtomicBool::new(false));
	// Pass None for trace -- if debug_traceTransaction is called, it will error
	let mock_transport = setup_mock_transport_for_internal_calls(
		None,
		trace_called.clone(),
	);
	let client = EvmClient::new_with_transport(mock_transport);

	// Monitor with ONLY event conditions, no function conditions at all.
	// This simulates L2 system contract monitors like ForceUpgrade or
	// EOA Collision which match on ContractDeployed events.
	let mut monitor = test_data.monitor.clone();
	monitor.match_conditions.functions = vec![];
	monitor.match_conditions.transactions = vec![];
	// Keep events from the default monitor fixture

	let contract_spec = test_data.contract_spec.unwrap();
	let contract_with_spec: (String, ContractSpec) = (
		"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".to_string(),
		contract_spec,
	);

	let _matches = filter_service
		.filter_block(
			&client,
			&test_data.network,
			&test_data.blocks[0],
			&[monitor],
			Some(&[contract_with_spec]),
		)
		.await?;

	// debug_traceTransaction should NOT be called -- there are no function
	// conditions (internal or otherwise), so the trace code path is never entered.
	assert!(
		!trace_called.load(Ordering::SeqCst),
		"debug_traceTransaction should NOT be called when monitor has only event conditions \
		and no function conditions. Event matching does not require tracing."
	);

	Ok(())
}
