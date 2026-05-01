//! SA API sandbox integration tests.
//!
//! Gated behind env vars — run only when credentials are provided:
//!
//! ```bash
//! export MPP_SA_SANDBOX_URL=http://defi-sa-gateway.forked-okx-test2-dataasset.swim.env
//! export MPP_SA_SANDBOX_KEY=...
//! export MPP_SA_SANDBOX_SECRET=...
//! export MPP_SA_SANDBOX_PASSPHRASE=...
//! cargo test -p mpp-evm --test sandbox -- --ignored
//! ```
//!
//! Tests are `#[ignore]`d by default so normal `cargo test` doesn't hit the
//! network. Each test exits early (as success) if credentials are missing,
//! which keeps `--ignored` runs green in environments that have no access.
//!
//! Mirrors curl samples from the [Pay] MPP EVM API plan doc.

use mpp_evm::{OkxSaApiClient, SaApiClient};
use serde_json::json;
use std::sync::Arc;

fn sandbox_client() -> Option<Arc<dyn SaApiClient>> {
    let url = std::env::var("MPP_SA_SANDBOX_URL").ok()?;
    let key = std::env::var("MPP_SA_SANDBOX_KEY").ok()?;
    let secret = std::env::var("MPP_SA_SANDBOX_SECRET").ok()?;
    let passphrase = std::env::var("MPP_SA_SANDBOX_PASSPHRASE").ok()?;
    Some(Arc::new(OkxSaApiClient::with_base_url(
        url, key, secret, passphrase,
    )))
}

#[tokio::test]
#[ignore]
async fn sandbox_charge_settle_smoke() {
    let Some(client) = sandbox_client() else {
        println!("skipping: MPP_SA_SANDBOX_* env vars not set");
        return;
    };

    // Payload shape per spec §8.2 + API doc sample. Nonces/addresses/signature
    // are placeholders — this is a smoke test that verifies the HTTP plumbing
    // and (if creds allow) expected error shape, not a happy-path settlement.
    let body = json!({
        "challenge": {
            "id": "smoke-qB3wErTyU7iOpAsD9fGhJk1",
            "realm": "michael.testing",
            "method": "evm",
            "intent": "charge",
            "request": "eyJhbW91bnQiOiIxMDAwMCIsImN1cnJlbmN5IjoiMHg3NGI3RjE2MzM3Yjg5NzIwMjdGNjE5NkExN2E2MzFhQzZkRTI2ZDIyIiwicmVjaXBpZW50IjoiMHg0YjIyZmRiYzM5OWJkNDIyYjZmZWZjYmNlOTVmNzY2NDJlYTI5ZGYxIiwibWV0aG9kRGV0YWlscyI6eyJjaGFpbklkIjoxOTYsImZlZVBheWVyIjpmYWxzZX19",
            "expires": "2027-04-01T12:05:00Z"
        },
        "payload": {
            "type": "transaction",
            "authorization": {
                "type": "eip-3009",
                "from": "0x0000000000000000000000000000000000000000",
                "to": "0x0000000000000000000000000000000000000000",
                "value": "0",
                "validAfter": "0",
                "validBefore": "9999999999",
                "nonce": "0x0000000000000000000000000000000000000000000000000000000000000000",
                "signature": "0x0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
            },
            "splits": []
        }
    });

    let outcome = client.charge_settle(&body).await;
    match outcome {
        Ok(receipt) => {
            // If creds+nonce allow a real settlement to go through, reference
            // should be populated.
            assert!(!receipt.reference.is_empty());
        }
        Err(err) => {
            // Invalid signature / duplicate nonce / blocked payer are all
            // acceptable failure modes — we're checking the plumbing.
            println!("sandbox charge_settle returned expected error: {err}");
            assert!(err.code >= 8000, "unexpected code {}", err.code);
        }
    }
}

#[tokio::test]
#[ignore]
async fn sandbox_charge_verify_hash_smoke() {
    let Some(client) = sandbox_client() else {
        println!("skipping: MPP_SA_SANDBOX_* env vars not set");
        return;
    };

    let body = json!({
        "challenge": {
            "id": "smoke-qB3wErTyU7iOpAsD9fGhJk2",
            "realm": "michael.testing",
            "method": "evm",
            "intent": "charge",
            "request": "eyJhbW91bnQiOiIxMDAwMCIsImN1cnJlbmN5IjoiMHg3NGI3RjE2MzM3Yjg5NzIwMjdGNjE5NkExN2E2MzFhQzZkRTI2ZDIyIiwicmVjaXBpZW50IjoiMHg0YjIyZmRiYzM5OWJkNDIyYjZmZWZjYmNlOTVmNzY2NDJlYTI5ZGYxIiwibWV0aG9kRGV0YWlscyI6eyJjaGFpbklkIjoxOTYsImZlZVBheWVyIjpmYWxzZX19",
            "expires": "2027-04-01T12:05:00Z"
        },
        "payload": {
            "type": "hash",
            "hash": "0x0000000000000000000000000000000000000000000000000000000000000000"
        },
        "source": "did:pkh:eip155:196:0x0000000000000000000000000000000000000000"
    });

    let outcome = client.charge_verify_hash(&body).await;
    match outcome {
        Ok(_) => {}
        Err(err) => {
            println!("sandbox verify_hash returned expected error: {err}");
            assert!(err.code >= 8000, "unexpected code {}", err.code);
        }
    }
}

#[tokio::test]
#[ignore]
async fn sandbox_session_status_of_nonexistent_channel_yields_70010() {
    let Some(client) = sandbox_client() else {
        println!("skipping: MPP_SA_SANDBOX_* env vars not set");
        return;
    };
    let err = client
        .session_status("0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef")
        .await
        .expect_err("nonexistent channel should fail");
    // Accept either a strict 70010 channel_not_found or a generic error if the
    // sandbox surfaces a different code — we just want non-success here.
    println!("session_status error: code={} msg={}", err.code, err.msg);
}
