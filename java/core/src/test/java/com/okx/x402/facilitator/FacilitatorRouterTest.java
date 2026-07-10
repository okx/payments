// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.facilitator;

import com.okx.x402.model.v2.PaymentPayload;
import com.okx.x402.model.v2.PaymentRequirements;
import com.okx.x402.model.v2.SettleResponse;
import com.okx.x402.model.v2.SupportedKind;
import com.okx.x402.model.v2.SupportedResponse;
import com.okx.x402.model.v2.VerifyResponse;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.util.List;

import static org.junit.jupiter.api.Assertions.*;

class FacilitatorRouterTest {

    private static FacilitatorClient stubClient(String payerLabel) {
        return new FacilitatorClient() {
            @Override
            public VerifyResponse verify(PaymentPayload p, PaymentRequirements r) {
                VerifyResponse vr = new VerifyResponse();
                vr.isValid = true;
                vr.payer = payerLabel;
                return vr;
            }
            @Override
            public SettleResponse settle(PaymentPayload p, PaymentRequirements r) {
                SettleResponse sr = new SettleResponse();
                sr.success = true;
                sr.transaction = "0x" + payerLabel;
                return sr;
            }
            @Override
            public SettleResponse settleStatus(String txHash) {
                SettleResponse sr = new SettleResponse();
                sr.success = true;
                sr.transaction = txHash;
                sr.status = "success";
                sr.payer = payerLabel;
                return sr;
            }
            @Override
            public SupportedResponse supported() {
                SupportedResponse sr = new SupportedResponse();
                SupportedKind kind = new SupportedKind();
                kind.scheme = "exact";
                kind.network = payerLabel;
                sr.kinds = List.of(kind);
                return sr;
            }
        };
    }

    @Test
    void customRouteIsUsed() throws Exception {
        FacilitatorRouter router = FacilitatorRouter.builder()
                .route("eip155:8453", stubClient("base"))
                .build();

        PaymentRequirements req = new PaymentRequirements();
        req.network = "eip155:8453";

        VerifyResponse vr = router.verify(new PaymentPayload(), req);
        assertTrue(vr.isValid);
        assertEquals("base", vr.payer);
    }

    @Test
    void settleRoutesToCorrectClient() throws Exception {
        FacilitatorRouter router = FacilitatorRouter.builder()
                .route("eip155:196", stubClient("xlayer"))
                .build();

        PaymentRequirements req = new PaymentRequirements();
        req.network = "eip155:196";

        SettleResponse sr = router.settle(new PaymentPayload(), req);
        assertTrue(sr.success);
        assertEquals("0xxlayer", sr.transaction);
    }

    @Test
    void unknownNetworkWithNoDefaultThrows() {
        FacilitatorRouter router = FacilitatorRouter.builder().build();

        PaymentRequirements req = new PaymentRequirements();
        req.network = "eip155:unknown";

        assertThrows(IllegalStateException.class,
                () -> router.verify(new PaymentPayload(), req));
    }

    @Test
    void defaultFacilitatorUsedForUnknownNetwork() throws Exception {
        FacilitatorRouter router = FacilitatorRouter.builder()
                .defaultFacilitator(stubClient("default"))
                .build();

        PaymentRequirements req = new PaymentRequirements();
        req.network = "eip155:any";

        VerifyResponse vr = router.verify(new PaymentPayload(), req);
        assertEquals("default", vr.payer);
    }

    @Test
    void supportedWithDefaultClient() throws Exception {
        FacilitatorRouter router = FacilitatorRouter.builder()
                .defaultFacilitator(stubClient("default-net"))
                .build();

        SupportedResponse sr = router.supported();
        assertEquals(1, sr.kinds.size());
        assertEquals("default-net", sr.kinds.get(0).network);
    }

    @Test
    void supportedWithNoDefaultUsesFirstRoute() throws Exception {
        FacilitatorRouter router = FacilitatorRouter.builder()
                .route("eip155:196", stubClient("xlayer"))
                .build();

        SupportedResponse sr = router.supported();
        assertNotNull(sr);
        assertFalse(sr.kinds.isEmpty());
    }

    @Test
    void supportedWithEmptyRouterReturnsEmpty() throws Exception {
        FacilitatorRouter router = FacilitatorRouter.builder().build();
        SupportedResponse sr = router.supported();
        assertNotNull(sr);
        assertTrue(sr.kinds.isEmpty());
    }

    @Test
    void supportedAggregatesFromAllClients() throws Exception {
        FacilitatorRouter router = FacilitatorRouter.builder()
                .route("eip155:196", stubClient("xlayer"))
                .route("eip155:8453", stubClient("base"))
                .defaultFacilitator(stubClient("default"))
                .build();

        SupportedResponse sr = router.supported();
        assertEquals(3, sr.kinds.size(),
                "Should aggregate kinds from default + 2 distinct route clients");
    }

    @Test
    void supportedDeduplicatesSameClient() throws Exception {
        FacilitatorClient shared = stubClient("shared");
        FacilitatorRouter router = FacilitatorRouter.builder()
                .route("eip155:196", shared)
                .route("eip155:195", shared)
                .build();

        SupportedResponse sr = router.supported();
        assertEquals(1, sr.kinds.size(),
                "Same client instance routed to 2 networks should only appear once");
    }

    @Test
    void multipleRoutesSelectCorrect() throws Exception {
        FacilitatorRouter router = FacilitatorRouter.builder()
                .route("eip155:196", stubClient("xlayer"))
                .route("eip155:8453", stubClient("base"))
                .defaultFacilitator(stubClient("fallback"))
                .build();

        PaymentRequirements xlayerReq = new PaymentRequirements();
        xlayerReq.network = "eip155:196";
        assertEquals("xlayer", router.verify(new PaymentPayload(), xlayerReq).payer);

        PaymentRequirements baseReq = new PaymentRequirements();
        baseReq.network = "eip155:8453";
        assertEquals("base", router.verify(new PaymentPayload(), baseReq).payer);

        PaymentRequirements otherReq = new PaymentRequirements();
        otherReq.network = "eip155:999";
        assertEquals("fallback", router.verify(new PaymentPayload(), otherReq).payer);
    }
}
