//! Utility functions: base64 encoding/decoding, pattern matching, deep equality.
//!
//! Mirrors: `@x402/core/src/utils/index.ts`

use base64::{engine::general_purpose::STANDARD, Engine};
use std::collections::HashMap;

use crate::error::X402Error;

/// Encode a string to base64 format.
///
/// Mirrors TS: `export function safeBase64Encode(data: string): string`
pub fn safe_base64_encode(data: &str) -> String {
    STANDARD.encode(data.as_bytes())
}

/// Decode a base64 string back to its original UTF-8 format.
///
/// Mirrors TS: `export function safeBase64Decode(data: string): string`
pub fn safe_base64_decode(data: &str) -> Result<String, X402Error> {
    let bytes = STANDARD.decode(data)?;
    String::from_utf8(bytes).map_err(|e| X402Error::Other(format!("invalid UTF-8: {}", e)))
}

/// Find implementations by network, supporting wildcard pattern matching.
/// Tries direct match first, then pattern matching (e.g., "eip155:*" matches "eip155:196").
///
/// Mirrors TS: `export const findSchemesByNetwork`
pub fn find_schemes_by_network<'a, T>(
    map: &'a HashMap<String, HashMap<String, T>>,
    network: &str,
) -> Option<&'a HashMap<String, T>> {
    // Direct match first
    if let Some(implementations) = map.get(network) {
        return Some(implementations);
    }

    // Try pattern matching
    for (pattern, implementations) in map.iter() {
        if network_matches_pattern(network, pattern) {
            return Some(implementations);
        }
    }

    None
}

/// Find a specific implementation by network and scheme.
///
/// Mirrors TS: `export const findByNetworkAndScheme`
pub fn find_by_network_and_scheme<'a, T>(
    map: &'a HashMap<String, HashMap<String, T>>,
    scheme: &str,
    network: &str,
) -> Option<&'a T> {
    find_schemes_by_network(map, network)?.get(scheme)
}

/// Check if a network identifier matches a pattern (supports `*` wildcard).
///
/// Examples:
/// - `network_matches_pattern("eip155:196", "eip155:*")` → true
/// - `network_matches_pattern("eip155:196", "eip155:196")` → true
/// - `network_matches_pattern("solana:devnet", "eip155:*")` → false
pub fn network_matches_pattern(network: &str, pattern: &str) -> bool {
    if pattern == network {
        return true;
    }

    // Convert pattern with * wildcards to simple matching
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return false; // No wildcard, already checked exact match
    }

    let mut remaining = network;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            // First part must be a prefix
            if !remaining.starts_with(part) {
                return false;
            }
            remaining = &remaining[part.len()..];
        } else if i == parts.len() - 1 {
            // Last part must be a suffix
            if !remaining.ends_with(part) {
                return false;
            }
            return true;
        } else {
            // Middle parts must be found in order
            match remaining.find(part) {
                Some(pos) => remaining = &remaining[pos + part.len()..],
                None => return false,
            }
        }
    }

    true
}

/// Deep equality comparison for serde_json::Value objects.
/// Uses normalized JSON serialization for consistent comparison.
///
/// Mirrors TS: `export function deepEqual(obj1, obj2): boolean`
pub fn deep_equal(obj1: &serde_json::Value, obj2: &serde_json::Value) -> bool {
    normalize_value(obj1) == normalize_value(obj2)
}

/// Normalize a JSON value by sorting object keys recursively.
fn normalize_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                sorted.insert(key.clone(), normalize_value(&map[key]));
            }
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(normalize_value).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_round_trip() {
        let original = r#"{"x402Version":2,"scheme":"exact"}"#;
        let encoded = safe_base64_encode(original);
        let decoded = safe_base64_decode(&encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_network_matches_pattern() {
        assert!(network_matches_pattern("eip155:196", "eip155:*"));
        assert!(network_matches_pattern("eip155:196", "eip155:196"));
        assert!(!network_matches_pattern("solana:devnet", "eip155:*"));
        assert!(network_matches_pattern("eip155:8453", "eip155:*"));
    }

    #[test]
    fn test_deep_equal() {
        let v1: serde_json::Value = serde_json::json!({"b": 2, "a": 1});
        let v2: serde_json::Value = serde_json::json!({"a": 1, "b": 2});
        assert!(deep_equal(&v1, &v2));

        let v3: serde_json::Value = serde_json::json!({"a": 1, "b": 3});
        assert!(!deep_equal(&v1, &v3));
    }
}
