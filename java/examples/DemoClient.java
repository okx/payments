// OKX x402 Java SDK — Demo Client
// Shows how to make paid API calls with automatic 402 handling
//
// Environment variables required:
//   PRIVATE_KEY - 0x-prefixed hex private key for signing payments

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
 * <p>Usage:</p>
 * <pre>
 *   export PRIVATE_KEY=0x...
 *   java DemoClient
 * </pre>
 *
 * <p>Flow:</p>
 * <ol>
 *   <li>Create signer from private key</li>
 *   <li>Create OKXHttpClient with signer</li>
 *   <li>GET protected endpoint → auto-handles 402</li>
 *   <li>Receive 200 + business data + settlement proof</li>
 * </ol>
 */
public class DemoClient {
    /**
     * Main entry point for the demo client.
     *
     * @param args command line arguments (unused)
     * @throws Exception if payment flow fails
     */
    public static void main(String[] args) throws Exception {
        // Step 1: Create signer from private key
        OKXEvmSigner signer = OKXSignerFactory.createOKXSigner(
                new OKXSignerConfig().privateKey(System.getenv("PRIVATE_KEY")));

        System.out.println("Signer address: " + signer.getAddress());

        // Step 2: Create auto-402 handling client
        OKXHttpClient client = new OKXHttpClient(signer, "eip155:196");

        // Step 3: GET protected endpoint
        URI uri = URI.create("http://localhost:8080/protected/xlayer-usdt");
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
    }
}
