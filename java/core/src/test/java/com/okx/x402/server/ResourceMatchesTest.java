package com.okx.x402.server;

import org.junit.jupiter.api.Test;

import java.lang.reflect.Method;
import java.util.List;

import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * Unit tests for {@code PaymentProcessor.resourceMatches} — the resource-URL
 * validation that backs {@link PaymentProcessor.RouteConfig#acceptedDomains}.
 *
 * <p>The method is {@code private static}; it is invoked here via reflection so
 * the validation logic can be pinned without standing up a full servlet flow.
 *
 * <p>Scenarios pinned:
 * <ul>
 *   <li>null / empty {@code acceptedDomains} → strict full-URL equality (legacy).</li>
 *   <li>configured domains → payload host must be accepted, payload path must
 *       equal request path, and the request host may differ (proxy rewrite).</li>
 *   <li>host not in the accepted set, path mismatch, and malformed URLs reject.</li>
 * </ul>
 */
class ResourceMatchesTest {

    private static boolean resourceMatches(String payloadUrl, String requestUrl,
                                           List<String> acceptedDomains) throws Exception {
        Method m = PaymentProcessor.class.getDeclaredMethod(
                "resourceMatches", String.class, String.class, List.class);
        m.setAccessible(true);
        return (boolean) m.invoke(null, payloadUrl, requestUrl, acceptedDomains);
    }

    @Test
    void nullAcceptedDomains_strictEquality() throws Exception {
        String url = "https://web3.okx.com/api/foo";
        assertTrue(resourceMatches(url, url, null));
        assertFalse(resourceMatches(url, "https://web3.ouyich.biz/api/foo", null));
    }

    @Test
    void emptyAcceptedDomains_strictEquality() throws Exception {
        String url = "https://web3.okx.com/api/foo";
        assertTrue(resourceMatches(url, url, List.of()));
        assertFalse(resourceMatches(url, "https://web3.ouyich.biz/api/foo", List.of()));
    }

    @Test
    void proxyRewrittenHost_acceptedWhenPayloadHostAllowedAndPathMatches() throws Exception {
        // The real reported case: buyer signs the public domain, the request
        // arrives on the proxy-rewritten internal domain.
        assertTrue(resourceMatches(
                "https://web3.okx.com/api/v6/dex/market/signal/list",
                "https://web3.ouyich.biz/api/v6/dex/market/signal/list",
                List.of("web3.okx.com", "web3.okx.io")));
    }

    @Test
    void payloadHostNotInAcceptedDomains_rejected() throws Exception {
        assertFalse(resourceMatches(
                "https://evil.example.com/api/foo",
                "https://web3.ouyich.biz/api/foo",
                List.of("web3.okx.com")));
    }

    @Test
    void pathMismatch_rejected() throws Exception {
        assertFalse(resourceMatches(
                "https://web3.okx.com/api/foo",
                "https://web3.ouyich.biz/api/bar",
                List.of("web3.okx.com")));
    }

    @Test
    void malformedUrl_rejected() throws Exception {
        assertFalse(resourceMatches(
                "::not a uri::",
                "https://web3.ouyich.biz/api/foo",
                List.of("web3.okx.com")));
    }
}
