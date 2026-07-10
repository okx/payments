// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.eip712;

import org.junit.jupiter.api.Test;
import org.web3j.crypto.ECDSASignature;
import org.web3j.crypto.Keys;
import org.web3j.crypto.Sign;
import org.web3j.utils.Numeric;

import java.math.BigInteger;
import java.util.Arrays;

import static org.junit.jupiter.api.Assertions.assertEquals;

/**
 * Known-answer test vectors for the canonical AccessProof encoding:
 * {@code inner = keccak256(abi.encodePacked(bytes32 subId, address payer, uint256 timestamp))}
 * — the timestamp is packed as a 32-byte big-endian integer. Frozen constants lock the digest
 * against silent encoding drift (an invisible-0x19 prefix or a narrower timestamp width would
 * both change the digest without failing structurally); the signature is from a fixed local key
 * so recovery is fully deterministic.
 */
class AccessProofEip191GoldenTest {

    private static final String SUB_ID =
            "0x20c4703ac29f837bfad63bb23f9d56407114bf24688b37c51005c6a8dd91e342";
    private static final long TIMESTAMP = 1782991122L;
    private static final BigInteger TEST_KEY = new BigInteger(
            "1111111111111111111111111111111111111111111111111111111111111111", 16);
    private static final String PAYER = "0x19e7e376e7c213b7e7e7e46cc70a5dd086daff2a";
    private static final String EXPECTED_INNER =
            "0xa348c5636515c9e03c80313e0cd0c7b1a189ddc1331a39d4067463b696343ca3";
    private static final String EXPECTED_DIGEST =
            "0x0335212d59f79f6162dea1b494e2a01b772df3b417a267d25c1c8e8f62d9c4e4";
    private static final String SIGNATURE =
            "0xd5e9dc6def9bd8fe319af6dc4a63aa0d24f24838c26d60d78aec1ccc47081391"
                    + "356c7c9d9df49115db27c1c9de9d11b1c9ee1c1faf4cb15e16491436e6b264fd" + "1c";

    @Test
    void innerHashPacksTimestampAsUint256() {
        byte[] inner = AccessProofEip191.innerHash(SUB_ID, PAYER, TIMESTAMP);
        assertEquals(EXPECTED_INNER, Numeric.toHexString(inner));
    }

    @Test
    void personalSignHashIsStandardSinglePrefix() {
        byte[] inner = AccessProofEip191.innerHash(SUB_ID, PAYER, TIMESTAMP);
        assertEquals(EXPECTED_DIGEST, Numeric.toHexString(
                AccessProofEip191.personalSignHash(inner)));
    }

    @Test
    void signatureRecoversPayer() {
        byte[] digest = AccessProofEip191.accessProofDigest(SUB_ID, PAYER, TIMESTAMP);
        byte[] sig = Numeric.hexStringToByteArray(SIGNATURE.substring(2));
        byte v = sig[64];
        if (v < 27) v += 27;
        BigInteger r = new BigInteger(1, Arrays.copyOfRange(sig, 0, 32));
        BigInteger s = new BigInteger(1, Arrays.copyOfRange(sig, 32, 64));
        BigInteger pub = Sign.recoverFromSignature(v - 27, new ECDSASignature(r, s), digest);
        assertEquals(PAYER, "0x" + Keys.getAddress(pub));
    }

    @Test
    void testKeyDerivesExpectedPayer() {
        // Sanity: the frozen PAYER constant really is the fixed test key's address.
        assertEquals(PAYER, "0x" + Keys.getAddress(
                org.web3j.crypto.ECKeyPair.create(TEST_KEY).getPublicKey()));
    }
}
