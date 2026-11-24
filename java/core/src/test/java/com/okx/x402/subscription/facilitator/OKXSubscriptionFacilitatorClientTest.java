// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.facilitator;

import com.github.tomakehurst.wiremock.WireMockServer;
import com.okx.x402.facilitator.HttpExecutor;
import com.okx.x402.facilitator.JdkHttpExecutor;
import com.okx.x402.subscription.error.SubscriptionException;
import com.okx.x402.subscription.model.CancelAuth;
import com.okx.x402.subscription.model.PendingChangeCancelAuth;
import com.okx.x402.subscription.model.PermitDetails;
import com.okx.x402.subscription.model.PermitSingle;
import com.okx.x402.subscription.model.SubscriptionTerms;
import com.okx.x402.subscription.model.resp.*;
import com.okx.x402.util.OKXAuth;
import org.junit.jupiter.api.*;

import java.net.http.HttpClient;
import java.time.Duration;

import static com.github.tomakehurst.wiremock.client.WireMock.*;
import static org.junit.jupiter.api.Assertions.*;

/**
 * Endpoint shapes mirror the OKX facilitator subscription API: write endpoints carry subId in
 * the body; detail is GET /subscriptions/detail?subId=.
 */
class OKXSubscriptionFacilitatorClientTest {

    static WireMockServer wm;
    OKXSubscriptionFacilitatorClient client;

    @BeforeAll
    static void startServer() {
        wm = new WireMockServer(0);
        wm.start();
    }

    @AfterAll
    static void stopServer() {
        wm.stop();
    }

    @BeforeEach
    void setUp() {
        wm.resetAll();
        OKXAuth auth = new OKXAuth("test-key", "test-secret", "test-pass");
        HttpExecutor executor = new JdkHttpExecutor(
                HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(5)).build());
        client = new OKXSubscriptionFacilitatorClient(
                auth, executor, "http://localhost:" + wm.port(), Duration.ofSeconds(10));
    }

    @Test
    void subscribeSuccess() throws Exception {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{\"subId\":\"0xabc\",\"txHash\":\"0x123\",\"state\":1},\"msg\":\"\"}")));

        SubscriptionTerms terms = buildTerms();
        PermitSingle permit = buildPermit();
        CreateResp resp = client.subscribe(196, terms, permit, "0xsig1", "0xsig2", true);

        assertEquals("0xabc", resp.subId);
        assertEquals("0x123", resp.txHash);
        assertEquals(1, resp.state);

        wm.verify(postRequestedFor(urlEqualTo("/api/v6/pay/x402/subscriptions"))
                .withHeader("OK-ACCESS-KEY", equalTo("test-key"))
                .withRequestBody(containing("\"chainIndex\":196"))
                .withRequestBody(containing("\"termsSig\":\"0xsig1\""))
                .withRequestBody(containing("\"permitSig\":\"0xsig2\""))
                .withRequestBody(containing("\"periodMode\":0"))
                .withRequestBody(containing("\"syncSettle\":true")));
    }

    @Test
    void chargeSuccess() throws Exception {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions/charge"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{\"subId\":\"0xsub1\",\"period\":3,\"txHash\":\"0xtx\",\"state\":1,\"planChangeTriggered\":false},\"msg\":\"\"}")));

        ChargeResp resp = client.charge("0xsub1", true);
        assertEquals("0xsub1", resp.subId);
        assertEquals(3, resp.period);
        assertFalse(resp.planChangeTriggered);

        wm.verify(postRequestedFor(urlEqualTo("/api/v6/pay/x402/subscriptions/charge"))
                .withRequestBody(containing("\"subId\":\"0xsub1\""))
                .withRequestBody(containing("\"syncSettle\":true")));
    }

    @Test
    void chargeWithPlanChange() throws Exception {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions/charge"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{\"subId\":\"0xsub1\",\"period\":5,\"txHash\":\"0xtx\",\"state\":1,\"planChangeTriggered\":true,\"newSubId\":\"0xnew\"},\"msg\":\"\"}")));

        ChargeResp resp = client.charge("0xsub1", false);
        assertTrue(resp.planChangeTriggered);
        assertEquals("0xnew", resp.newSubId);
    }

    @Test
    void changeUpgrade() throws Exception {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions/change"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{\"newSubId\":\"0xnewsub\",\"txHash\":\"0xtx2\",\"state\":1},\"msg\":\"\"}")));

        ChangeResp resp = client.change(196, "0xoldsub", buildTerms(), buildPermit(),
                "0xsig1", "0xsig2", true);
        assertEquals("0xnewsub", resp.newSubId);
        assertEquals(1, resp.state);

        wm.verify(postRequestedFor(urlEqualTo("/api/v6/pay/x402/subscriptions/change"))
                .withRequestBody(containing("\"oldSubId\":\"0xoldsub\""))
                .withRequestBody(containing("\"newTerms\":")));
    }

    @Test
    void cancelSuccess() throws Exception {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions/cancel"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{\"subId\":\"0xsub1\",\"txHash\":null,\"state\":3},\"msg\":\"\"}")));

        CancelAuth auth = new CancelAuth();
        auth.action = 0;
        auth.subId = "0xsub1";
        auth.initiator = 1;
        auth.nonce = "0x" + "ab".repeat(32);
        auth.deadline = 1700000000L;
        auth.signature = "0xsig";

        CancelResp resp = client.cancel("0xsub1", auth, true);
        assertEquals("0xsub1", resp.subId);
        assertEquals(3, resp.state);

        wm.verify(postRequestedFor(urlEqualTo("/api/v6/pay/x402/subscriptions/cancel"))
                .withRequestBody(containing("\"subId\":\"0xsub1\""))
                .withRequestBody(containing("\"cancelAuth\":")));
    }

    @Test
    void cancelPendingChangeSuccess() throws Exception {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions/cancel-pending-change"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{\"subId\":\"0xsub1\",\"txHash\":null,\"state\":2},\"msg\":\"\"}")));

        PendingChangeCancelAuth auth = new PendingChangeCancelAuth();
        auth.subId = "0xsub1";
        auth.newSubId = "0x" + "ee".repeat(32);
        auth.nonce = "0x" + "cd".repeat(32);
        auth.deadline = 1700000000L;
        auth.signature = "0xsig";

        CancelPendingResp resp = client.cancelPendingChange("0xsub1", auth, true);
        assertEquals("0xsub1", resp.subId);

        // cancelAuth must carry the scheduled downgrade's newSubId — the cancellation binds to
        // one specific pending change.
        wm.verify(postRequestedFor(urlEqualTo("/api/v6/pay/x402/subscriptions/cancel-pending-change"))
                .withRequestBody(containing("\"newSubId\":\"0x" + "ee".repeat(32) + "\"")));
    }

    @Test
    void finalizeExpiredSuccess() throws Exception {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions/finalize-expired"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{\"subId\":\"0xsub1\"},\"msg\":\"\"}")));

        FinalizeExpiredResp resp = client.finalizeExpired("0xsub1");
        assertNotNull(resp);

        wm.verify(postRequestedFor(urlEqualTo("/api/v6/pay/x402/subscriptions/finalize-expired"))
                .withRequestBody(containing("\"subId\":\"0xsub1\"")));
    }

    @Test
    void getSubscription() throws Exception {
        wm.stubFor(get(urlEqualTo("/api/v6/pay/x402/subscriptions/detail?subId=0xsub1"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{\"subId\":\"0xsub1\",\"state\":1,\"isActive\":true,"
                                + "\"serviceEnded\":false,\"currentPeriod\":3,\"elapsedPeriods\":3,"
                                + "\"lastChargedPeriod\":3,\"periodMode\":0,\"billingAnchorAt\":1700000000,"
                                + "\"startAt\":1700000000,\"nextChargeableAt\":1700259200},\"msg\":\"\"}")));

        QueryResp resp = client.getSubscription("0xsub1");
        assertEquals("0xsub1", resp.subId);
        assertTrue(resp.isActive);
        assertEquals(3, resp.currentPeriod);
        assertEquals(3, resp.elapsedPeriods);
        assertEquals(3, resp.lastChargedPeriod);
        assertEquals(1700000000L, resp.billingAnchorAt);
    }

    @Test
    void getCharges() throws Exception {
        wm.stubFor(get(urlEqualTo("/api/v6/pay/x402/subscriptions/charges?subId=0xsub1&limit=10&offset=0"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{\"charges\":[{\"subId\":\"0xsub1\",\"period\":1,\"txHash\":\"0xtx\",\"state\":1,\"chargeType\":1}],\"total\":1},\"msg\":\"\"}")));

        ChargeListResp resp = client.getCharges("0xsub1", 10, 0);
        assertEquals(1, resp.charges.size());
    }

    @Test
    void getPendingChange() throws Exception {
        wm.stubFor(get(urlEqualTo("/api/v6/pay/x402/subscriptions/pending?subId=0xsub1"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{\"subId\":\"0xsub1\",\"newSubId\":\"0xnew\",\"state\":0,\"newPlanTier\":1,\"fromSubId\":\"0xsub1\"},\"msg\":\"\"}")));

        PendingChangeResp resp = client.getPendingChange("0xsub1");
        assertEquals("0xnew", resp.newSubId);
        assertEquals((Integer) 0, resp.state);
    }

    @Test
    void getPendingChangeReturnsNullWhenNoPendingRow() throws Exception {
        // The facilitator answers an all-null body when no change is pending — that must not
        // deserialize into a row whose primitive state reads 0 = PENDING (a legal value).
        wm.stubFor(get(urlEqualTo("/api/v6/pay/x402/subscriptions/pending?subId=0xsub1"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{},\"msg\":\"\"}")));

        assertNull(client.getPendingChange("0xsub1"));
    }

    @Test
    void businessErrorWithoutDataKeyThrows() {
        // A business error whose envelope has no data key must throw — it must not fall past
        // the code+data gate and deserialize into an all-null "pending success".
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions/charge"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":51000,\"msg\":\"unauthorized_caller\"}")));

        SubscriptionException e = assertThrows(SubscriptionException.class,
                () -> client.charge("0xsub1", true));
        assertEquals("unauthorized_caller", e.getCode());
    }

    @Test
    void errorMessageTakesPriorityOverMsg() {
        // Extraction priority is error_message → msg → code; a JSON-null msg must never
        // surface as the literal string "null".
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions/charge"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":51000,\"error_message\":\"charge_in_flight\","
                                + "\"msg\":null,\"data\":null}")));

        SubscriptionException e = assertThrows(SubscriptionException.class,
                () -> client.charge("0xsub1", true));
        assertEquals("charge_in_flight", e.getCode());
    }

    @Test
    void detailSuffixedMsgYieldsBareCode() {
        // The facilitator interpolates a detail suffix into msg
        // ("subscription_not_active: state=3"). The exception's code must be the bare
        // segment before the colon or the self-heal switch / classify never match.
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions/charge"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":30001,\"msg\":\"subscription_not_active: state=3\",\"data\":null}")));

        SubscriptionException e = assertThrows(SubscriptionException.class,
                () -> client.charge("0xsub1", true));
        assertEquals("subscription_not_active", e.getCode());
        assertTrue(e.isSelfHeal());
        // Humans keep the full detail on the message.
        assertTrue(e.getMessage().contains("subscription_not_active: state=3"));
    }

    @Test
    void camelCaseSaCodeClassifiesAsLocalReject() {
        // The facilitator emits mixed-camelCase spellings for eight codes — those spellings
        // are the arbiter, and must classify correctly as-is.
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions/cancel"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":30001,\"msg\":\"cancel_subId_mismatch: auth.subId != subId\",\"data\":null}")));

        CancelAuth auth = new CancelAuth();
        auth.action = 0;
        auth.subId = "0xsub1";
        auth.initiator = 1;
        auth.nonce = "0x" + "ab".repeat(32);
        auth.deadline = 1700000000L;
        auth.signature = "0xsig";

        SubscriptionException e = assertThrows(SubscriptionException.class,
                () -> client.cancel("0xsub1", auth, true));
        assertEquals("cancel_subId_mismatch", e.getCode());
        assertEquals(com.okx.x402.subscription.error.SubscriptionErrorCodes.Category.LOCAL_REJECT,
                e.getCategory());
    }

    @Test
    void inFlightCodeIsRetryable() {
        // The facilitator's in-flight lock codes are transient — the renewal engine must retry.
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":30001,\"msg\":\"create_in_flight\",\"data\":null}")));

        SubscriptionException e = assertThrows(SubscriptionException.class,
                () -> client.subscribe(196, buildTerms(), buildPermit(), "0xs1", "0xs2", true));
        assertEquals("create_in_flight", e.getCode());
        assertTrue(e.isRetryable());
    }

    @Test
    void chargeResponseMissingStateThrows() {
        // state is required on write responses — a partial backend write must not read as a
        // fabricated pending(0) that a scheduler then chases.
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions/charge"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{\"subId\":\"0xsub1\",\"period\":3},\"msg\":\"\"}")));

        SubscriptionException e = assertThrows(SubscriptionException.class,
                () -> client.charge("0xsub1", true));
        assertEquals("system_error", e.getCode());
    }

    @Test
    void http201IsAccepted() throws Exception {
        // Any 2xx counts as HTTP success (gateways may answer 201/202/204).
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions"))
                .willReturn(aResponse()
                        .withStatus(201)
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{\"subId\":\"0xabc\",\"txHash\":\"0x1\",\"state\":1},\"msg\":\"\"}")));

        CreateResp resp = client.subscribe(196, buildTerms(), buildPermit(), "0xs1", "0xs2", true);
        assertEquals("0xabc", resp.subId);
    }

    @Test
    void getAllowanceStatus() throws Exception {
        wm.stubFor(get(urlEqualTo("/api/v6/pay/x402/buyers/0xbuyer/allowance-status?token=0xtoken&chainIndex=196"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{\"nonce\":5,\"reservedAmount\":\"1000\",\"reservedExpiration\":1700000000,\"subscriptionContract\":\"0xcontract\",\"permit2Contract\":\"0xpermit2\",\"permit2Allowance\":\"999999\"},\"msg\":\"\"}")));

        AllowanceStatusResp resp = client.getAllowanceStatus("0xbuyer", "0xtoken", 196);
        assertEquals(5, resp.nonce);
        assertEquals("1000", resp.reservedAmount);
        assertEquals("0xcontract", resp.subscriptionContract);
    }

    @Test
    void getBuyerSubscriptions() throws Exception {
        wm.stubFor(get(urlEqualTo("/api/v6/pay/x402/buyers/0xbuyer/subscriptions?limit=10&offset=0"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{\"subscriptions\":[{\"subId\":\"0xsub1\",\"state\":1,\"token\":\"0xtok\",\"amountPerPeriod\":\"5000000\",\"periodSec\":2592000,\"currentPeriod\":2,\"elapsedPeriods\":2,\"lastChargedPeriod\":2}]},\"msg\":\"\"}")));

        BuyerSubscriptionListResp resp = client.getBuyerSubscriptions("0xbuyer", 10, 0);
        assertEquals(1, resp.subscriptions.size());
        assertEquals("0xsub1", resp.subscriptions.get(0).subId);
        assertEquals(2, resp.subscriptions.get(0).elapsedPeriods);
    }

    @Test
    void featureDisabledError() {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":1,\"data\":null,\"msg\":\"feature_disabled\"}")));

        SubscriptionException ex = assertThrows(SubscriptionException.class,
                () -> client.subscribe(196, buildTerms(), buildPermit(), "0xs1", "0xs2", false));
        assertEquals("feature_disabled", ex.getCode());
    }

    @Test
    void hmacHeadersPresent() throws Exception {
        wm.stubFor(post(urlEqualTo("/api/v6/pay/x402/subscriptions"))
                .willReturn(aResponse()
                        .withHeader("Content-Type", "application/json")
                        .withBody("{\"code\":0,\"data\":{\"subId\":\"0x1\",\"txHash\":\"0x2\",\"state\":1},\"msg\":\"\"}")));

        client.subscribe(196, buildTerms(), buildPermit(), "0xs1", "0xs2", false);

        wm.verify(postRequestedFor(urlEqualTo("/api/v6/pay/x402/subscriptions"))
                .withHeader("OK-ACCESS-KEY", equalTo("test-key"))
                .withHeader("OK-ACCESS-PASSPHRASE", equalTo("test-pass"))
                .withHeader("OK-ACCESS-SIGN", matching(".+"))
                .withHeader("OK-ACCESS-TIMESTAMP", matching(".+")));
    }

    private SubscriptionTerms buildTerms() {
        SubscriptionTerms t = new SubscriptionTerms();
        t.payer = "0x1111111111111111111111111111111111111111";
        t.merchant = "0x2222222222222222222222222222222222222222";
        t.facilitator = "0x3333333333333333333333333333333333333333";
        t.token = "0x4444444444444444444444444444444444444444";
        t.amountPerPeriod = "5000000";
        t.periodSec = 2592000;
        t.maxPeriods = 12;
        t.startAt = 1700000000L;
        t.initialChargePeriods = 1;
        t.initialChargeAmount = "5000000";
        t.termsDeadline = 1700086400L;
        t.permitHash = "0x" + "aa".repeat(32);
        t.salt = "0x" + "bb".repeat(32);
        t.planTier = 2;
        t.changeFromSubId = "0x" + "00".repeat(32);
        t.changeEffectiveAt = 0;
        t.periodMode = 0;
        t.planId = "0x" + "cc".repeat(32);
        return t;
    }

    private PermitSingle buildPermit() {
        PermitSingle p = new PermitSingle();
        p.details = new PermitDetails();
        p.details.token = "0x4444444444444444444444444444444444444444";
        p.details.amount = "60000000";
        p.details.expiration = 1731136000L;
        p.details.nonce = 42;
        p.spender = "0x1234567890abcdef1234567890abcdef12345678";
        p.sigDeadline = "1700086400";
        return p;
    }
}
