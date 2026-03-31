package com.okx.x402.server;

import com.okx.x402.facilitator.FacilitatorClient;
import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.ResourceInfo;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.SupportedResponse;
import com.okx.x402.model.v2.VerifyResponse;
import jakarta.servlet.FilterChain;
import jakarta.servlet.ServletRequest;
import jakarta.servlet.ServletResponse;
import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.mockito.Mock;
import org.mockito.MockitoAnnotations;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.PrintWriter;
import java.util.Map;

import static org.mockito.Mockito.*;

class PaymentFilterV2Test {

    @Mock HttpServletRequest req;
    @Mock HttpServletResponse resp;
    @Mock FilterChain chain;
    @Mock FacilitatorClient fac;

    private PaymentFilter filter;

    @BeforeEach
    void init() throws Exception {
        MockitoAnnotations.openMocks(this);
        when(resp.getWriter()).thenReturn(new PrintWriter(new ByteArrayOutputStream(), true));

        PaymentFilter.RouteConfig xlayerConfig = new PaymentFilter.RouteConfig();
        xlayerConfig.network = "eip155:196";
        xlayerConfig.payTo = "0xReceiver";
        xlayerConfig.price = "$0.01";

        filter = PaymentFilter.create(fac, Map.of(
                "GET /protected/xlayer-usdt", xlayerConfig
        ));
    }

    @Test
    void freeEndpointPassesThrough() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/public");

        filter.doFilter(req, resp, chain);

        verify(chain).doFilter(req, resp);
        verify(resp, never()).setStatus(anyInt());
    }

    @Test
    void missingHeaderReturns402() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(null);
        when(req.getHeader("X-PAYMENT")).thenReturn(null);

        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        verify(chain, never()).doFilter(any(), any());
    }

    @Test
    void validV2HeaderVerifyAndSettle() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        // Build V2 payment payload
        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.resource = new ResourceInfo();
        p.resource.url = "http://localhost/protected/xlayer-usdt";
        p.payload = Map.of("signature", "0xabc");
        String header = p.toHeader();
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(header);

        // Facilitator verify succeeds
        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);

        // Handler returns 200
        when(resp.getStatus()).thenReturn(HttpServletResponse.SC_OK);

        // Settlement succeeds
        SettleResponse sr = new SettleResponse();
        sr.success = true;
        sr.transaction = "0xdef";
        sr.network = "eip155:196";
        sr.payer = "0xPayer";
        when(fac.settle(any(), any())).thenReturn(sr);

        filter.doFilter(req, resp, chain);

        verify(chain).doFilter(req, resp);
        verify(resp, never()).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        verify(resp).setHeader(eq("PAYMENT-RESPONSE"), any());
        verify(resp).setHeader(eq("Access-Control-Expose-Headers"), eq("PAYMENT-RESPONSE"));
    }

    @Test
    void v1FallbackHeaderWorks() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(null);

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.payload = Map.of("signature", "0xabc");
        String header = p.toHeader();
        when(req.getHeader("X-PAYMENT")).thenReturn(header);

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);
        when(resp.getStatus()).thenReturn(200);

        SettleResponse sr = new SettleResponse();
        sr.success = true;
        sr.transaction = "0x123";
        sr.network = "eip155:196";
        when(fac.settle(any(), any())).thenReturn(sr);

        filter.doFilter(req, resp, chain);
        verify(chain).doFilter(req, resp);
    }

    @Test
    void verificationExceptionReturns500() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.payload = Map.of("test", "data");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(p.toHeader());

        when(fac.verify(any(), any())).thenThrow(new IOException("Network error"));

        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(HttpServletResponse.SC_INTERNAL_SERVER_ERROR);
        verify(chain, never()).doFilter(any(), any());
    }

    @Test
    void settlementFailureReturns402() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.payload = Map.of("test", "data");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(p.toHeader());

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);
        when(resp.getStatus()).thenReturn(200);

        SettleResponse sr = new SettleResponse();
        sr.success = false;
        sr.errorReason = "insufficient_funds";
        when(fac.settle(any(), any())).thenReturn(sr);

        filter.doFilter(req, resp, chain);

        verify(chain).doFilter(req, resp);
        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
    }

    @Test
    void nonHttpRequestPassesThrough() throws Exception {
        ServletRequest nonHttpReq = mock(ServletRequest.class);
        ServletResponse nonHttpRes = mock(ServletResponse.class);

        filter.doFilter(nonHttpReq, nonHttpRes, chain);

        verify(chain).doFilter(nonHttpReq, nonHttpRes);
        verifyNoInteractions(fac);
    }

    @Test
    void malformedHeaderReturns402() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn("not-valid-base64!!!");

        filter.doFilter(req, resp, chain);

        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
        verify(chain, never()).doFilter(any(), any());
    }

    @Test
    void errorResponseSkipsSettlement() throws Exception {
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/protected/xlayer-usdt");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/protected/xlayer-usdt"));

        PaymentPayload p = new PaymentPayload();
        p.x402Version = 2;
        p.payload = Map.of("test", "data");
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(p.toHeader());

        VerifyResponse vr = new VerifyResponse();
        vr.isValid = true;
        when(fac.verify(any(), any())).thenReturn(vr);

        // Handler returns 500
        when(resp.getStatus()).thenReturn(500);

        filter.doFilter(req, resp, chain);

        verify(chain).doFilter(req, resp);
        verify(fac, never()).settle(any(), any());
    }

    @Test
    void routeLevelPricing() throws Exception {
        PaymentFilter.RouteConfig cheapConfig = new PaymentFilter.RouteConfig();
        cheapConfig.network = "eip155:196";
        cheapConfig.payTo = "0xReceiver";
        cheapConfig.price = "$0.01";

        PaymentFilter.RouteConfig expensiveConfig = new PaymentFilter.RouteConfig();
        expensiveConfig.network = "eip155:196";
        expensiveConfig.payTo = "0xReceiver";
        expensiveConfig.price = "$1.00";

        PaymentFilter multiFilter = PaymentFilter.create(fac, Map.of(
                "GET /cheap", cheapConfig,
                "GET /expensive", expensiveConfig
        ));

        // Just verify both routes are accepted (no exception)
        when(req.getMethod()).thenReturn("GET");
        when(req.getRequestURI()).thenReturn("/cheap");
        when(req.getRequestURL()).thenReturn(new StringBuffer("http://localhost/cheap"));
        when(req.getHeader("PAYMENT-SIGNATURE")).thenReturn(null);
        when(req.getHeader("X-PAYMENT")).thenReturn(null);

        multiFilter.doFilter(req, resp, chain);
        verify(resp).setStatus(HttpServletResponse.SC_PAYMENT_REQUIRED);
    }
}
