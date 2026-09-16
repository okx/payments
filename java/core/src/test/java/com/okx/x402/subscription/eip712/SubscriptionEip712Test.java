// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.eip712;

import com.okx.x402.subscription.model.CancelAuth;
import com.okx.x402.subscription.model.PendingChangeCancelAuth;
import com.okx.x402.subscription.model.PermitDetails;
import com.okx.x402.subscription.model.PermitSingle;
import com.okx.x402.subscription.model.SubscriptionTerms;
import org.junit.jupiter.api.Test;
import org.web3j.crypto.Hash;
import org.web3j.utils.Numeric;

import java.nio.charset.StandardCharsets;

import static org.junit.jupiter.api.Assertions.*;

class SubscriptionEip712Test {

    private static final long CHAIN_ID = 196;
    private static final String CONTRACT = "0x1234567890abcdef1234567890abcdef12345678";

    @Test
    void subscriptionTermsTypehashMatchesTypestring() {
        byte[] expected = Hash.sha3(SubscriptionEip712.SUBSCRIPTION_TERMS_TYPE.getBytes(StandardCharsets.UTF_8));
        assertArrayEquals(expected, SubscriptionEip712.SUBSCRIPTION_TERMS_TYPEHASH);
    }

    @Test
    void termsTypehashMatchesContractKnownVector() {
        // Known-answer test vector: the 17-field typehash published in the facilitator API
        // reference; final authority is the contract's SUBSCRIPTION_TERMS_TYPEHASH() getter.
        assertEquals("0xa5223de56e7694cf776c7d4f74c0323f42bf9e65655fe49affefbdfd40ec97ae",
                org.web3j.utils.Numeric.toHexString(SubscriptionEip712.SUBSCRIPTION_TERMS_TYPEHASH));
    }

    @Test
    void cancelAuthTypehashMatchesTypestring() {
        byte[] expected = Hash.sha3(SubscriptionEip712.CANCEL_AUTH_TYPE.getBytes(StandardCharsets.UTF_8));
        assertArrayEquals(expected, SubscriptionEip712.CANCEL_AUTH_TYPEHASH);
    }

    @Test
    void pendingChangeCancelAuthTypehashMatchesTypestring() {
        byte[] expected = Hash.sha3(SubscriptionEip712.PENDING_CHANGE_CANCEL_AUTH_TYPE.getBytes(StandardCharsets.UTF_8));
        assertArrayEquals(expected, SubscriptionEip712.PENDING_CHANGE_CANCEL_AUTH_TYPEHASH);
    }

    @Test
    void permitSingleTypehashMatchesTypestring() {
        byte[] expected = Hash.sha3(SubscriptionEip712.PERMIT_SINGLE_TYPE.getBytes(StandardCharsets.UTF_8));
        assertArrayEquals(expected, SubscriptionEip712.PERMIT_SINGLE_TYPEHASH);
    }

    @Test
    void subDomainSeparatorDeterministic() {
        byte[] sep1 = SubscriptionEip712.subDomainSeparator(CHAIN_ID, CONTRACT);
        byte[] sep2 = SubscriptionEip712.subDomainSeparator(CHAIN_ID, CONTRACT);
        assertArrayEquals(sep1, sep2);
        assertEquals(32, sep1.length);
    }

    @Test
    void subDomainSeparatorDifferentChainIds() {
        byte[] sep196 = SubscriptionEip712.subDomainSeparator(196, CONTRACT);
        byte[] sep1952 = SubscriptionEip712.subDomainSeparator(1952, CONTRACT);
        assertFalse(java.util.Arrays.equals(sep196, sep1952));
    }

    @Test
    void termsDigestIsDeterministicAndProducesSubId() {
        SubscriptionTerms terms = buildSampleTerms();
        byte[] digest1 = SubscriptionEip712.termsDigest(terms, CHAIN_ID, CONTRACT);
        byte[] digest2 = SubscriptionEip712.termsDigest(terms, CHAIN_ID, CONTRACT);
        assertArrayEquals(digest1, digest2);
        assertEquals(32, digest1.length);
    }

    @Test
    void termsDigestChangesWhenFieldChanges() {
        SubscriptionTerms terms1 = buildSampleTerms();
        SubscriptionTerms terms2 = buildSampleTerms();
        terms2.amountPerPeriod = "9999999";

        byte[] d1 = SubscriptionEip712.termsDigest(terms1, CHAIN_ID, CONTRACT);
        byte[] d2 = SubscriptionEip712.termsDigest(terms2, CHAIN_ID, CONTRACT);
        assertFalse(java.util.Arrays.equals(d1, d2));
    }

    @Test
    void hashPermitSingleProduces32Bytes() {
        PermitSingle permit = buildSamplePermit();
        byte[] hash = SubscriptionEip712.hashPermitSingle(permit);
        assertEquals(32, hash.length);
    }

    @Test
    void hashPermitSingleIsDeterministic() {
        PermitSingle permit = buildSamplePermit();
        byte[] h1 = SubscriptionEip712.hashPermitSingle(permit);
        byte[] h2 = SubscriptionEip712.hashPermitSingle(permit);
        assertArrayEquals(h1, h2);
    }

    @Test
    void permitHashDoesNotIncludeDomainSeparator() {
        PermitSingle permit = buildSamplePermit();
        byte[] hash = SubscriptionEip712.hashPermitSingle(permit);
        // permitHash is struct hash only (no 0x1901 prefix)
        String hex = Numeric.toHexStringNoPrefix(hash);
        assertEquals(64, hex.length());
        // Verify it's not prefixed with 1901
        assertNotEquals((byte) 0x19, hash[0]);
    }

    @Test
    void cancelAuthDigestProduces32Bytes() {
        CancelAuth auth = new CancelAuth();
        auth.action = 0;
        auth.subId = "0x" + "ab".repeat(32);
        auth.initiator = 0;
        auth.nonce = "0x" + "cd".repeat(32);
        auth.deadline = 1700000000L;

        byte[] digest = SubscriptionEip712.hashCancelAuth(auth, CHAIN_ID, CONTRACT);
        assertEquals(32, digest.length);
    }

    @Test
    void pendingChangeCancelAuthDigestProduces32Bytes() {
        PendingChangeCancelAuth auth = new PendingChangeCancelAuth();
        auth.subId = "0x" + "ab".repeat(32);
        auth.newSubId = "0x" + "ee".repeat(32);
        auth.nonce = "0x" + "cd".repeat(32);
        auth.deadline = 1700000000L;

        byte[] digest = SubscriptionEip712.hashPendingChangeCancelAuth(auth, CHAIN_ID, CONTRACT);
        assertEquals(32, digest.length);
    }

    @Test
    void termsDigestHexReturnsWithoutPrefix() {
        SubscriptionTerms terms = buildSampleTerms();
        String hex = SubscriptionEip712.termsDigestHex(terms, CHAIN_ID, CONTRACT);
        assertEquals(64, hex.length());
        assertFalse(hex.startsWith("0x"));
    }

    private SubscriptionTerms buildSampleTerms() {
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
        return t;
    }

    private PermitSingle buildSamplePermit() {
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
