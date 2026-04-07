//! Quick test: can Rust reqwest reach the OKX Facilitator?
//! Run: cargo run --example test_connectivity

use reqwest::Client;

#[tokio::main]
async fn main() {
    let url = std::env::var("FACILITATOR_URL")
        .unwrap_or_else(|_| "https://web3.okx.com".to_string());
    let full_url = format!("{}/api/v6/pay/x402/supported", url);

    println!("Testing connectivity to: {}", full_url);

    // Test 1: default client (rustls)
    println!("\n--- Test 1: default reqwest client ---");
    let client = Client::new();
    match client.get(&full_url).send().await {
        Ok(resp) => println!("OK! Status: {}, Body: {}", resp.status(), resp.text().await.unwrap_or_default()),
        Err(e) => {
            println!("FAILED: {}", e);
            if let Some(source) = std::error::Error::source(&e) {
                println!("  Caused by: {}", source);
                if let Some(source2) = std::error::Error::source(source) {
                    println!("  Caused by: {}", source2);
                    if let Some(source3) = std::error::Error::source(source2) {
                        println!("  Caused by: {}", source3);
                    }
                }
            }
        }
    }

    // Test 2: native-tls client
    println!("\n--- Test 2: native-tls client ---");
    let client = Client::builder()
        .use_native_tls()
        .build()
        .expect("failed to build native-tls client");
    match client.get(&full_url).send().await {
        Ok(resp) => println!("OK! Status: {}, Body: {}", resp.status(), resp.text().await.unwrap_or_default()),
        Err(e) => {
            println!("FAILED: {}", e);
            if let Some(source) = std::error::Error::source(&e) {
                println!("  Caused by: {}", source);
                if let Some(source2) = std::error::Error::source(source) {
                    println!("  Caused by: {}", source2);
                    if let Some(source3) = std::error::Error::source(source2) {
                        println!("  Caused by: {}", source3);
                    }
                }
            }
        }
    }

    // Test 3: with danger_accept_invalid_certs
    println!("\n--- Test 3: accept invalid certs ---");
    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("failed to build client");
    match client.get(&full_url).send().await {
        Ok(resp) => println!("OK! Status: {}, Body: {}", resp.status(), resp.text().await.unwrap_or_default()),
        Err(e) => {
            println!("FAILED: {}", e);
            if let Some(source) = std::error::Error::source(&e) {
                println!("  Caused by: {}", source);
            }
        }
    }

    // Test 4: public HTTPS site (sanity check)
    println!("\n--- Test 4: public HTTPS (httpbin.org) ---");
    let client = Client::new();
    match client.get("https://httpbin.org/get").send().await {
        Ok(resp) => println!("OK! Status: {}", resp.status()),
        Err(e) => println!("FAILED: {} ", e),
    }

    // Test 5: HTTP (no TLS) to the facilitator
    let http_url = url.replace("https://", "http://");
    let http_full = format!("{}/api/v6/pay/x402/supported", http_url);
    println!("\n--- Test 5: HTTP (no TLS): {} ---", http_full);
    let client = Client::new();
    match client.get(&http_full).send().await {
        Ok(resp) => println!("OK! Status: {}, Body: {}", resp.status(), resp.text().await.unwrap_or_default()),
        Err(e) => println!("FAILED: {}", e),
    }

    // Test 6: native-tls with native root certs + no hostname verification
    println!("\n--- Test 6: native-tls + no hostname verify + accept invalid ---");
    let client = Client::builder()
        .use_native_tls()
        .danger_accept_invalid_certs(true)
        .no_proxy()
        .build()
        .expect("failed to build client");
    match client.get(&full_url).send().await {
        Ok(resp) => println!("OK! Status: {}, Body: {}", resp.status(), resp.text().await.unwrap_or_default()),
        Err(e) => {
            println!("FAILED: {}", e);
            if let Some(source) = std::error::Error::source(&e) {
                println!("  Caused by: {}", source);
                if let Some(source2) = std::error::Error::source(source) {
                    println!("  Caused by: {}", source2);
                }
            }
        }
    }
}
