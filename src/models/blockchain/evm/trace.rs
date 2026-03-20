use alloy::primitives::{Address, Bytes, U256};
use serde::{Deserialize, Serialize};

/// A single frame from the `callTracer` output of `debug_traceTransaction`.
///
/// Represents one call in the execution trace tree. Internal calls are
/// nested in the `calls` field. Used for matching function calls that
/// occur inside the call stack (not just the top-level transaction).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CallTrace {
	/// Call type: CALL, STATICCALL, DELEGATECALL, CREATE, CREATE2
	#[serde(rename = "type")]
	pub call_type: String,

	/// Address that initiated this call
	pub from: Address,

	/// Target address (None for CREATE/CREATE2)
	#[serde(default)]
	pub to: Option<Address>,

	/// Input data (function selector + encoded arguments)
	#[serde(default)]
	pub input: Option<Bytes>,

	/// Return data
	#[serde(default)]
	pub output: Option<Bytes>,

	/// ETH value sent with the call
	#[serde(default)]
	pub value: Option<U256>,

	/// Gas provided
	#[serde(default)]
	pub gas: Option<String>,

	/// Gas consumed
	#[serde(rename = "gasUsed", default)]
	pub gas_used: Option<String>,

	/// Nested sub-calls
	#[serde(default)]
	pub calls: Option<Vec<CallTrace>>,

	/// Error message if the call reverted
	#[serde(default)]
	pub error: Option<String>,

	/// Revert reason if available
	#[serde(rename = "revertReason", default)]
	pub revert_reason: Option<String>,
}
