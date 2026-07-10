// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.client;

import com.okx.x402.crypto.CryptoSigner;
import com.okx.x402.crypto.CryptoSignException;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.mockito.Mock;
import org.mockito.MockitoAnnotations;

import java.io.IOException;
import java.math.BigInteger;
import java.net.URI;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;

import static org.junit.jupiter.api.Assertions.*;
import static org.mockito.Mockito.*;

class X402HttpClientTest {

    @Mock
    private CryptoSigner mockSigner;

    private X402HttpClient client;

    @BeforeEach
    void setup() throws CryptoSignException {
        MockitoAnnotations.openMocks(this);
        when(mockSigner.sign(any())).thenReturn("0xMockSignature");

        client = new X402HttpClient(mockSigner) {
            @Override
            protected HttpResponse<String> sendRequest(HttpRequest request) {
                @SuppressWarnings("unchecked")
                HttpResponse<String> mockResponse = mock(HttpResponse.class);
                when(mockResponse.statusCode()).thenReturn(200);
                when(mockResponse.body()).thenReturn("{\"ok\":true}");
                return mockResponse;
            }
        };
    }

    @Test
    void testConstructor() {
        X402HttpClient testClient = new X402HttpClient(mockSigner);
        assertNotNull(testClient);
    }

    @Test
    void testGetReturnsResponse() throws Exception {
        URI uri = URI.create("https://example.com/private");
        HttpResponse<String> response = client.get(uri, BigInteger.valueOf(1000), "0xToken", "0xPayTo");

        assertNotNull(response);
        assertEquals(200, response.statusCode());
        assertEquals("{\"ok\":true}", response.body());
    }

    @Test
    void testGetCallsSigner() throws Exception {
        URI uri = URI.create("https://example.com/private");
        client.get(uri, BigInteger.valueOf(1000), "0xToken", "0xPayTo");

        verify(mockSigner).sign(argThat(payload -> {
            assertEquals("1000", payload.get("amount"));
            assertEquals("0xToken", payload.get("asset"));
            assertEquals("0xPayTo", payload.get("payTo"));
            assertEquals("/private", payload.get("resource"));
            assertNotNull(payload.get("nonce"));
            return true;
        }));
    }

    @Test
    void testGetWithSignerException() throws CryptoSignException {
        when(mockSigner.sign(any())).thenThrow(new CryptoSignException("sign failed"));

        X402HttpClient failClient = new X402HttpClient(mockSigner) {
            @Override
            protected HttpResponse<String> sendRequest(HttpRequest request) {
                fail("Should not reach here");
                return null;
            }
        };

        assertThrows(RuntimeException.class, () ->
                failClient.get(URI.create("https://example.com/path"),
                        BigInteger.TEN, "0xToken", "0xPayTo"));
    }
}
