// SPDX-License-Identifier: Apache-2.0
// OKX x402 Java SDK — Demo Client
// Shows how to make paid API calls with automatic 402 handling.
//
// Environment variables:
//   PRIVATE_KEY  - 0x-prefixed hex private key for signing payments
//   SERVER_URL   - (optional) base URL of the demo server (default: http://localhost:8080)

package com.okx.x402.examples;

import com.okx.x402.client.OKXHttpClient;
import com.okx.x402.crypto.OKXEvmSigner;
import com.okx.x402.crypto.OKXSignerFactory;
import com.okx.x402.crypto.OKXSignerFactory.OKXSignerConfig;

import java.net.URI;
import java.net.http.HttpResponse;
import java.util.Base64;

/**
 * Demo client showing automatic x402 payment flow.
 *
 * <p>Flow: GET protected endpoint → auto-handles 402 → sign → retry → 200 + data</p>
 */
public class DemoClient {

    public static void main(String[] args) {
        try {
            String privateKey = System.getenv("PRIVATE_KEY");
            if (privateKey == null || privateKey.isEmpty()) {
                System.err.println("ERROR: PRIVATE_KEY environment variable is required.");
                System.err.println("Set: export PRIVATE_KEY=0x...");
                System.exit(1);
            }

            String baseUrl = System.getenv("SERVER_URL") != null
                    ? System.getenv("SERVER_URL") : "http://localhost:8080";

            // Step 1: Create signer from private key
            OKXEvmSigner signer = OKXSignerFactory.createOKXSigner(
                    new OKXSignerConfig().privateKey(privateKey));
            System.out.println("Signer address: " + signer.getAddress());

            // Step 2: Create auto-402 handling client
            OKXHttpClient client = new OKXHttpClient(signer, "eip155:196");

            // Step 3: GET protected endpoint
            URI uri = URI.create(baseUrl + "/protected/xlayer-usdt");
            System.out.println("Requesting: " + uri);

            HttpResponse<String> resp = client.get(uri);

            System.out.println("Status: " + resp.statusCode());
            System.out.println("Body: " + resp.body());

            // Step 4: Print settlement proof
            String paymentResponse = resp.headers()
                    .firstValue("PAYMENT-RESPONSE").orElse(null);
            if (paymentResponse != null) {
                String json = new String(Base64.getDecoder().decode(paymentResponse));
                System.out.println("Settlement: " + json);
            }
        } catch (Exception e) {
            System.err.println("Payment failed: " + e.getMessage());
            e.printStackTrace();
            System.exit(1);
        }
    }
}
