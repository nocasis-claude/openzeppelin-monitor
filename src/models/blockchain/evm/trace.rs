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

#[cfg(test)]
mod tests {
	use super::*;

	/// Verify CallTrace deserializes from real callTracer JSON output,
	/// including nested calls, optional fields, and renamed fields.
	#[test]
	fn test_call_trace_deserialization_from_call_tracer_json() {
		let json = serde_json::json!({
			"type": "CALL",
			"from": "0x58b704065b7aff3ed351052f8560019e05925023",
			"to": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
			"value": "0x0",
			"gas": "0x5208",
			"gasUsed": "0x3e8",
			"input": "0xa9059cbb000000000000000000000000f423d9c1ffeb6386639d024f3b241dab2331b6350000000000000000000000000000000000000000000000000000000005f5e100",
			"output": "0x0000000000000000000000000000000000000000000000000000000000000001",
			"calls": [
				{
					"type": "CALL",
					"from": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
					"to": "0x43506849d7c04f9138d1a2050bbf3a0c054402dd",
					"gas": "0x1234",
					"gasUsed": "0x100",
					"input": "0x40c10f19000000000000000000000000f423d9c1ffeb6386639d024f3b241dab2331b6350000000000000000000000000000000000000000000000000000000005f5e100",
					"output": "0x",
					"calls": [
						{
							"type": "DELEGATECALL",
							"from": "0x43506849d7c04f9138d1a2050bbf3a0c054402dd",
							"to": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
							"gas": "0xabc",
							"gasUsed": "0x50",
							"input": "0x40c10f19000000000000000000000000f423d9c1ffeb6386639d024f3b241dab2331b6350000000000000000000000000000000000000000000000000000000005f5e100",
							"output": "0x0000000000000000000000000000000000000000000000000000000000000001"
						}
					]
				}
			]
		});

		let trace: CallTrace = serde_json::from_value(json).expect("should deserialize");

		// Top-level call
		assert_eq!(trace.call_type, "CALL");
		assert_eq!(
			trace.from,
			"0x58b704065b7aff3ed351052f8560019e05925023"
				.parse::<Address>()
				.unwrap()
		);
		assert_eq!(
			trace.to.unwrap(),
			"0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
				.parse::<Address>()
				.unwrap()
		);
		assert!(trace.input.is_some());
		assert!(trace.output.is_some());
		assert_eq!(trace.gas.as_deref(), Some("0x5208"));
		assert_eq!(trace.gas_used.as_deref(), Some("0x3e8"));
		assert_eq!(trace.value, Some(U256::ZERO));
		assert!(trace.error.is_none());
		assert!(trace.revert_reason.is_none());

		// Nested calls
		let sub_calls = trace.calls.as_ref().unwrap();
		assert_eq!(sub_calls.len(), 1);
		assert_eq!(sub_calls[0].call_type, "CALL");

		// Depth-2 call
		let depth2 = sub_calls[0].calls.as_ref().unwrap();
		assert_eq!(depth2.len(), 1);
		assert_eq!(depth2[0].call_type, "DELEGATECALL");
		assert!(depth2[0].calls.is_none()); // leaf node
	}

	/// Verify CallTrace handles minimal JSON (only required fields).
	#[test]
	fn test_call_trace_deserialization_minimal() {
		let json = serde_json::json!({
			"type": "STATICCALL",
			"from": "0x0000000000000000000000000000000000000001"
		});

		let trace: CallTrace = serde_json::from_value(json).expect("should deserialize");
		assert_eq!(trace.call_type, "STATICCALL");
		assert!(trace.to.is_none());
		assert!(trace.input.is_none());
		assert!(trace.output.is_none());
		assert!(trace.value.is_none());
		assert!(trace.gas.is_none());
		assert!(trace.gas_used.is_none());
		assert!(trace.calls.is_none());
		assert!(trace.error.is_none());
		assert!(trace.revert_reason.is_none());
	}

	/// Verify deserialization of a reverted call with error and revertReason.
	#[test]
	fn test_call_trace_deserialization_with_error() {
		let json = serde_json::json!({
			"type": "CALL",
			"from": "0x0000000000000000000000000000000000000001",
			"to": "0x0000000000000000000000000000000000000002",
			"input": "0x12345678",
			"error": "execution reverted",
			"revertReason": "ERC20: transfer amount exceeds balance"
		});

		let trace: CallTrace = serde_json::from_value(json).expect("should deserialize");
		assert_eq!(trace.error.as_deref(), Some("execution reverted"));
		assert_eq!(
			trace.revert_reason.as_deref(),
			Some("ERC20: transfer amount exceeds balance")
		);
	}

	/// Verify CallTrace round-trips through serialize/deserialize.
	#[test]
	fn test_call_trace_serialization_roundtrip() {
		let json = serde_json::json!({
			"type": "CALL",
			"from": "0x58b704065b7aff3ed351052f8560019e05925023",
			"to": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
			"gas": "0x5208",
			"gasUsed": "0x3e8",
			"input": "0xa9059cbb",
			"output": "0x01",
			"calls": []
		});

		let trace: CallTrace = serde_json::from_value(json).expect("should deserialize");
		let serialized = serde_json::to_value(&trace).expect("should serialize");
		let trace2: CallTrace = serde_json::from_value(serialized).expect("should re-deserialize");

		assert_eq!(trace.call_type, trace2.call_type);
		assert_eq!(trace.from, trace2.from);
		assert_eq!(trace.to, trace2.to);
		assert_eq!(trace.gas, trace2.gas);
		assert_eq!(trace.gas_used, trace2.gas_used);
	}
}
