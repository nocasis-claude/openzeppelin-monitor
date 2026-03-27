//! EVM-compatible blockchain client implementation.
//!
//! This module provides functionality to interact with Ethereum and other EVM-compatible
//! blockchains, supporting operations like block retrieval, transaction receipt lookup,
//! and log filtering.

use std::marker::PhantomData;

use anyhow::Context;
use async_trait::async_trait;
use futures;
use serde_json::json;
use tracing::instrument;

use crate::{
	models::{BlockType, EVMBlock, EVMReceiptLog, EVMTransactionReceipt, Network},
	services::{
		blockchain::{
			client::BlockChainClient,
			transports::{BlockchainTransport, EVMTransportClient, TransportError},
			BlockFilterFactory,
		},
		filter::{evm_helpers::string_to_h256, EVMBlockFilter},
	},
};

/// Client implementation for Ethereum Virtual Machine (EVM) compatible blockchains
///
/// Provides high-level access to EVM blockchain data and operations through HTTP transport.
#[derive(Clone)]
pub struct EvmClient<T: Send + Sync + Clone> {
	/// The underlying HTTP transport client for RPC communication
	http_client: T,
}

impl<T: Send + Sync + Clone> EvmClient<T> {
	/// Creates a new EVM client instance with a specific transport client
	pub fn new_with_transport(http_client: T) -> Self {
		Self { http_client }
	}
}

impl EvmClient<EVMTransportClient> {
	/// Creates a new EVM client instance
	///
	/// # Arguments
	/// * `network` - Network configuration containing RPC endpoints and chain details
	///
	/// # Returns
	/// * `Result<Self, anyhow::Error>` - New client instance or connection error
	pub async fn new(network: &Network) -> Result<Self, anyhow::Error> {
		let client = EVMTransportClient::new(network).await?;
		Ok(Self::new_with_transport(client))
	}
}

impl<T: Send + Sync + Clone + BlockchainTransport> BlockFilterFactory<Self> for EvmClient<T> {
	type Filter = EVMBlockFilter<Self>;
	fn filter() -> Self::Filter {
		EVMBlockFilter {
			_client: PhantomData,
		}
	}
}

/// Extended functionality specific to EVM-compatible blockchains
#[async_trait]
pub trait EvmClientTrait {
	/// Retrieves a transaction receipt by its hash
	///
	/// # Arguments
	/// * `transaction_hash` - The hash of the transaction to look up
	///
	/// # Returns
	/// * `Result<TransactionReceipt, anyhow::Error>` - Transaction receipt or error
	async fn get_transaction_receipt(
		&self,
		transaction_hash: String,
	) -> Result<EVMTransactionReceipt, anyhow::Error>;

	/// Retrieves logs for a range of blocks
	///
	/// # Arguments
	/// * `from_block` - Starting block number
	/// * `to_block` - Ending block number
	/// * `addresses` - Optional list of addresses to filter logs by
	/// # Returns
	/// * `Result<Vec<Log>, anyhow::Error>` - Collection of matching logs or error
	async fn get_logs_for_blocks(
		&self,
		from_block: u64,
		to_block: u64,
		addresses: Option<Vec<String>>,
	) -> Result<Vec<EVMReceiptLog>, anyhow::Error>;

	/// Traces a transaction's internal calls using debug_traceTransaction
	/// with the callTracer. Returns the full call tree.
	///
	/// # Arguments
	/// * `transaction_hash` - The hash of the transaction to trace
	///
	/// # Returns
	/// * `Result<CallTrace, anyhow::Error>` - The call trace tree or error
	async fn debug_trace_transaction(
		&self,
		transaction_hash: String,
	) -> Result<crate::models::CallTrace, anyhow::Error>;
}

#[async_trait]
impl<T: Send + Sync + Clone + BlockchainTransport> EvmClientTrait for EvmClient<T> {
	/// Retrieves a transaction receipt by hash with proper error handling
	#[instrument(skip(self), fields(transaction_hash))]
	async fn get_transaction_receipt(
		&self,
		transaction_hash: String,
	) -> Result<EVMTransactionReceipt, anyhow::Error> {
		let hash = string_to_h256(&transaction_hash)
			.map_err(|e| anyhow::anyhow!("Invalid transaction hash: {}", e))?;

		let params = json!([format!("0x{:x}", hash)])
			.as_array()
			.with_context(|| "Failed to create JSON-RPC params array")?
			.to_vec();

		let response = self
			.http_client
			.send_raw_request(
				"eth_getTransactionReceipt",
				Some(serde_json::Value::Array(params)),
			)
			.await
			.with_context(|| format!("Failed to get transaction receipt: {}", transaction_hash))?;

		// Extract the "result" field from the JSON-RPC response
		let receipt_data = response
			.get("result")
			.with_context(|| "Missing 'result' field")?;

		// Handle null response case
		if receipt_data.is_null() {
			return Err(anyhow::anyhow!("Transaction receipt not found"));
		}

		Ok(serde_json::from_value(receipt_data.clone())
			.with_context(|| "Failed to parse transaction receipt")?)
	}

	/// Retrieves logs within the specified block range
	///
	/// # Arguments
	/// * `from_block` - Starting block number
	/// * `to_block` - Ending block number
	/// * `addresses` - Optional list of addresses to filter logs by
	/// # Returns
	/// * `Result<Vec<EVMReceiptLog>, anyhow::Error>` - Collection of matching logs or error
	#[instrument(skip(self), fields(from_block, to_block))]
	async fn get_logs_for_blocks(
		&self,
		from_block: u64,
		to_block: u64,
		addresses: Option<Vec<String>>,
	) -> Result<Vec<EVMReceiptLog>, anyhow::Error> {
		// Convert parameters to JSON-RPC format
		let params = json!([{
			"fromBlock": format!("0x{:x}", from_block),
			"toBlock": format!("0x{:x}", to_block),
			"address": addresses
		}])
		.as_array()
		.with_context(|| "Failed to create JSON-RPC params array")?
		.to_vec();

		let response = self
			.http_client
			.send_raw_request("eth_getLogs", Some(params))
			.await
			.with_context(|| {
				format!(
					"Failed to get logs for blocks: {} - {}",
					from_block, to_block
				)
			})?;

		// Extract the "result" field from the JSON-RPC response
		let logs_data = response
			.get("result")
			.with_context(|| "Missing 'result' field")?;

		// Parse the response into the expected type
		Ok(serde_json::from_value(logs_data.clone()).with_context(|| "Failed to parse logs")?)
	}

	#[instrument(skip(self), fields(transaction_hash))]
	async fn debug_trace_transaction(
		&self,
		transaction_hash: String,
	) -> Result<crate::models::CallTrace, anyhow::Error> {
		let params = json!([
			transaction_hash,
			{"tracer": "callTracer", "tracerConfig": {"onlyTopCall": false}}
		])
		.as_array()
		.with_context(|| "Failed to create JSON-RPC params array")?
		.to_vec();

		// Retry with exponential backoff on 429 (rate limit).
		// The endpoint_manager handles rotation for multi-endpoint configs,
		// but with a single endpoint it returns the 429 error immediately.
		// We retry here to handle both cases without modifying upstream code.
		const MAX_RETRIES: u32 = 3;
		const BACKOFF_MS: [u64; 3] = [200, 500, 1000];

		for attempt in 0..=MAX_RETRIES {
			if attempt > 0 {
				let delay = BACKOFF_MS
					.get((attempt - 1) as usize)
					.copied()
					.unwrap_or(1000);
				tracing::warn!(
					"debug_traceTransaction for {} failed (retryable), retry {}/{} after {}ms",
					transaction_hash,
					attempt,
					MAX_RETRIES,
					delay,
				);
				tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
			}

			match self
				.http_client
				.send_raw_request(
					"debug_traceTransaction",
					Some(serde_json::Value::Array(params.clone())),
				)
				.await
			{
				Ok(response) => {
					let trace_data = response
						.get("result")
						.with_context(|| "Missing 'result' field in trace response")?;

					return Ok(serde_json::from_value(trace_data.clone())
						.with_context(|| "Failed to parse call trace")?);
				}
				Err(e) => {
					let is_retryable = matches!(
						&e,
						TransportError::Http { status_code, .. }
							if matches!(status_code.as_u16(), 429 | 404 | 500 | 502 | 503 | 504)
					);

					if is_retryable && attempt < MAX_RETRIES {
						continue;
					}

					return Err(e).with_context(|| {
						format!("Failed to trace transaction: {}", transaction_hash)
					});
				}
			}
		}

		unreachable!("loop always returns")
	}
}

#[async_trait]
impl<T: Send + Sync + Clone + BlockchainTransport> BlockChainClient for EvmClient<T> {
	/// Retrieves the latest block number with retry functionality
	#[instrument(skip(self))]
	async fn get_latest_block_number(&self) -> Result<u64, anyhow::Error> {
		let response = self
			.http_client
			.send_raw_request::<serde_json::Value>("eth_blockNumber", None)
			.await
			.with_context(|| "Failed to get latest block number")?;

		// Extract the "result" field from the JSON-RPC response
		let hex_str = response
			.get("result")
			.and_then(|v| v.as_str())
			.ok_or_else(|| anyhow::anyhow!("Missing 'result' field"))?;

		// Parse hex string to u64
		u64::from_str_radix(hex_str.trim_start_matches("0x"), 16)
			.map_err(|e| anyhow::anyhow!("Failed to parse block number: {}", e))
	}

	/// Retrieves blocks within the specified range with retry functionality
	///
	/// # Note
	/// If end_block is None, only the start_block will be retrieved
	#[instrument(skip(self), fields(start_block, end_block))]
	async fn get_blocks(
		&self,
		start_block: u64,
		end_block: Option<u64>,
	) -> Result<Vec<BlockType>, anyhow::Error> {
		let block_futures: Vec<_> = (start_block..=end_block.unwrap_or(start_block))
			.map(|block_number| {
				let params = json!([
					format!("0x{:x}", block_number),
					true // include full transaction objects
				]);
				let client = self.http_client.clone();

				async move {
					let response = client
						.send_raw_request("eth_getBlockByNumber", Some(params))
						.await
						.with_context(|| format!("Failed to get block: {}", block_number))?;

					let block_data = response
						.get("result")
						.ok_or_else(|| anyhow::anyhow!("Missing 'result' field"))?;

					if block_data.is_null() {
						return Err(anyhow::anyhow!("Block not found"));
					}

					let block: EVMBlock = serde_json::from_value(block_data.clone())
						.map_err(|e| anyhow::anyhow!("Failed to parse block: {}", e))?;

					Ok(BlockType::EVM(Box::new(block)))
				}
			})
			.collect();

		futures::future::join_all(block_futures)
			.await
			.into_iter()
			.collect::<Result<Vec<_>, _>>()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::services::blockchain::transports::{
		BlockchainTransport, RotatingTransport, TransportError,
	};
	use mockall::mock;
	use reqwest_middleware::ClientWithMiddleware;
	use serde_json::Value;
	use std::sync::atomic::{AtomicU32, Ordering};
	use std::sync::Arc;

	// Mirror the pattern from tests/integration/mocks/transports.rs:
	// Mock a concrete struct, then manually implement the generic trait.
	mock! {
		pub TestTransport {
			pub async fn send_raw_request(&self, method: &str, params: Option<Vec<Value>>) -> Result<Value, TransportError>;
			pub async fn get_current_url(&self) -> String;
		}

		impl Clone for TestTransport {
			fn clone(&self) -> Self;
		}
	}

	#[async_trait]
	impl BlockchainTransport for MockTestTransport {
		async fn get_current_url(&self) -> String {
			self.get_current_url().await
		}

		async fn send_raw_request<P>(
			&self,
			method: &str,
			params: Option<P>,
		) -> Result<Value, TransportError>
		where
			P: Into<Value> + Send + Clone,
		{
			let params_value = params.map(|p| p.into());
			self.send_raw_request(method, params_value.and_then(|v| v.as_array().cloned()))
				.await
		}

		fn update_endpoint_manager_client(
			&mut self,
			_: ClientWithMiddleware,
		) -> Result<(), anyhow::Error> {
			Ok(())
		}
	}

	#[async_trait]
	impl RotatingTransport for MockTestTransport {
		async fn try_connect(&self, _url: &str) -> Result<(), anyhow::Error> {
			Ok(())
		}
		async fn update_client(&self, _url: &str) -> Result<(), anyhow::Error> {
			Ok(())
		}
	}

	fn valid_trace_response() -> Value {
		serde_json::json!({
			"result": {
				"type": "CALL",
				"from": "0x0000000000000000000000000000000000000001",
				"to": "0x0000000000000000000000000000000000000002",
				"input": "0x",
				"output": "0x",
				"calls": []
			}
		})
	}

	fn error_429() -> TransportError {
		TransportError::http(
			reqwest::StatusCode::TOO_MANY_REQUESTS,
			"http://test".to_string(),
			"rate limited".to_string(),
			None,
			None,
		)
	}

	fn error_500() -> TransportError {
		TransportError::http(
			reqwest::StatusCode::INTERNAL_SERVER_ERROR,
			"http://test".to_string(),
			"server error".to_string(),
			None,
			None,
		)
	}

	#[tokio::test]
	async fn test_debug_trace_success_no_retry() {
		let mut mock = MockTestTransport::new();
		mock.expect_send_raw_request()
			.times(1)
			.returning(|_, _| Ok(valid_trace_response()));
		mock.expect_clone().returning(|| {
			let mut m = MockTestTransport::new();
			m.expect_send_raw_request()
				.returning(|_, _| Ok(valid_trace_response()));
			m
		});

		let client = EvmClient::new_with_transport(mock);
		let result = client
			.debug_trace_transaction("0xabc".to_string())
			.await;

		assert!(result.is_ok());
		let trace = result.unwrap();
		assert_eq!(trace.call_type, "CALL");
	}

	#[tokio::test]
	async fn test_debug_trace_429_then_success() {
		let call_count = Arc::new(AtomicU32::new(0));
		let call_count_clone = call_count.clone();

		let mut mock = MockTestTransport::new();
		mock.expect_send_raw_request()
			.times(2)
			.returning(move |_, _| {
				let n = call_count_clone.fetch_add(1, Ordering::SeqCst);
				if n == 0 {
					Err(error_429())
				} else {
					Ok(valid_trace_response())
				}
			});
		mock.expect_clone().returning(|| {
			let mut m = MockTestTransport::new();
			m.expect_send_raw_request()
				.returning(|_, _| Ok(valid_trace_response()));
			m
		});

		let client = EvmClient::new_with_transport(mock);
		let result = client
			.debug_trace_transaction("0xabc".to_string())
			.await;

		assert!(result.is_ok());
		assert_eq!(call_count.load(Ordering::SeqCst), 2);
	}

	#[tokio::test]
	async fn test_debug_trace_429_exhausts_retries() {
		let call_count = Arc::new(AtomicU32::new(0));
		let call_count_clone = call_count.clone();

		let mut mock = MockTestTransport::new();
		mock.expect_send_raw_request()
			.times(4) // 1 initial + 3 retries
			.returning(move |_, _| {
				call_count_clone.fetch_add(1, Ordering::SeqCst);
				Err(error_429())
			});
		mock.expect_clone().returning(|| {
			let mut m = MockTestTransport::new();
			m.expect_send_raw_request()
				.returning(|_, _| Err(error_429()));
			m
		});

		let client = EvmClient::new_with_transport(mock);
		let result = client
			.debug_trace_transaction("0xabc".to_string())
			.await;

		assert!(result.is_err());
		assert_eq!(call_count.load(Ordering::SeqCst), 4);
		let err_msg = format!("{}", result.unwrap_err());
		assert!(err_msg.contains("429") || err_msg.contains("trace transaction"));
	}

	#[tokio::test]
	async fn test_debug_trace_502_then_success() {
		let call_count = Arc::new(AtomicU32::new(0));
		let call_count_clone = call_count.clone();

		let mut mock = MockTestTransport::new();
		mock.expect_send_raw_request()
			.times(2)
			.returning(move |_, _| {
				let n = call_count_clone.fetch_add(1, Ordering::SeqCst);
				if n == 0 {
					Err(TransportError::http(
						reqwest::StatusCode::BAD_GATEWAY,
						"http://test".to_string(),
						"bad gateway".to_string(),
						None,
						None,
					))
				} else {
					Ok(valid_trace_response())
				}
			});
		mock.expect_clone().returning(|| {
			let mut m = MockTestTransport::new();
			m.expect_send_raw_request()
				.returning(|_, _| Ok(valid_trace_response()));
			m
		});

		let client = EvmClient::new_with_transport(mock);
		let result = client
			.debug_trace_transaction("0xabc".to_string())
			.await;

		assert!(result.is_ok());
		assert_eq!(call_count.load(Ordering::SeqCst), 2); // Retried once
	}

	#[tokio::test]
	async fn test_debug_trace_non_retryable_error_no_retry() {
		let call_count = Arc::new(AtomicU32::new(0));
		let call_count_clone = call_count.clone();

		let mut mock = MockTestTransport::new();
		mock.expect_send_raw_request()
			.times(1)
			.returning(move |_, _| {
				call_count_clone.fetch_add(1, Ordering::SeqCst);
				Err(TransportError::http(
					reqwest::StatusCode::FORBIDDEN, // 403 — not retryable
					"http://test".to_string(),
					"forbidden".to_string(),
					None,
					None,
				))
			});
		mock.expect_clone().returning(|| {
			let mut m = MockTestTransport::new();
			m.expect_send_raw_request()
				.returning(|_, _| Err(error_500()));
			m
		});

		let client = EvmClient::new_with_transport(mock);
		let result = client
			.debug_trace_transaction("0xabc".to_string())
			.await;

		assert!(result.is_err());
		assert_eq!(call_count.load(Ordering::SeqCst), 1); // No retry on 403
	}
}
