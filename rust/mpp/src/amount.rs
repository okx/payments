//! Compare route prices and currencies without modifying the authenticated credential.
use alloy_primitives::Address;
use mpp::protocol::core::PaymentCredential;

pub(crate) fn check(
    credential: &PaymentCredential,
    expected_amount: &str,
    expected_currency: &str,
) -> Result<(), String> {
    let request: serde_json::Value = credential
        .challenge
        .request
        .decode()
        .map_err(|e| format!("decode credential request: {e}"))?;
    let actual = request
        .get("amount")
        .and_then(|v| v.as_str())
        .ok_or("credential amount is missing")?;
    check_amount(actual, expected_amount)?;
    let currency = request
        .get("currency")
        .and_then(|v| v.as_str())
        .ok_or("credential currency is missing")?;
    check_currency(currency, expected_currency)
}

/// Compare EVM token addresses, accepting casing differences.
/// This does not authenticate a credential.
pub fn check_currency(actual: &str, expected: &str) -> Result<(), String> {
    let actual_address = actual
        .parse::<Address>()
        .map_err(|_| "credential currency must be an EVM token address".to_string())?;
    let expected_address = expected
        .parse::<Address>()
        .map_err(|_| "route currency must be an EVM token address".to_string())?;
    if actual_address != expected_address {
        return Err(format!(
            "currency mismatch: credential has {actual}, route requires {expected}"
        ));
    }
    Ok(())
}

/// Compare base-unit integer prices. This does not authenticate a credential.
pub fn check_amount(actual: &str, expected: &str) -> Result<(), String> {
    fn canonical(value: &str) -> Result<&str, String> {
        if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
            return Err("amount must be a base-unit integer".into());
        }
        Ok(value.trim_start_matches('0'))
    }
    if canonical(actual)? != canonical(expected)? {
        return Err(format!(
            "amount mismatch: credential has {actual}, route requires {expected}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn credential_requires_a_currency_even_when_the_amount_matches() {
        use mpp::protocol::core::{Base64UrlJson, ChallengeEcho, PaymentCredential};

        for currency in [
            None,
            Some(serde_json::Value::Null),
            Some(serde_json::json!(42)),
        ] {
            let mut request = serde_json::json!({ "amount": "100" });
            if let Some(currency) = currency {
                request["currency"] = currency;
            }
            let credential = PaymentCredential::new(
                ChallengeEcho {
                    id: "fixture".into(),
                    realm: "test".into(),
                    method: "evm".into(),
                    intent: "charge".into(),
                    request: Base64UrlJson::from_value(&request).unwrap(),
                    expires: None,
                    digest: None,
                    opaque: None,
                },
                serde_json::json!({}),
            );
            let error = super::check(
                &credential,
                "100",
                "0x1111111111111111111111111111111111111111",
            )
            .unwrap_err();
            assert!(error.contains("currency is missing"), "{error}");
        }
    }

    #[test]
    fn currencies_match_by_address_not_casing() {
        let currency = "0x74b7F16337b8972027F6196A17a631aC6dE26d22";
        assert!(super::check_currency(currency, &currency.to_ascii_lowercase()).is_ok());
        assert!(
            super::check_currency(currency, "0x1111111111111111111111111111111111111111")
                .unwrap_err()
                .contains("currency mismatch")
        );
        for invalid in ["", "0x", "USDC", "0xnot-a-token"] {
            assert!(super::check_currency(invalid, currency).is_err());
            assert!(super::check_currency(currency, invalid).is_err());
        }
    }

    #[test]
    fn amounts_are_numeric_integers_without_precision_loss() {
        assert!(super::check_amount("000100", "100").is_ok());
        assert!(super::check_amount(
            "340282366920938463463374607431768211456",
            "340282366920938463463374607431768211456"
        )
        .is_ok());
        for bad in ["", "-1", "+1", "1.0", "1e2", " 100"] {
            assert!(super::check_amount(bad, "100").is_err());
        }
        assert!(super::check_amount("100", "50000").is_err());
    }
}
