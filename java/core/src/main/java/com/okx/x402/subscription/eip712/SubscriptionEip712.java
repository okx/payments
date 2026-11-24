// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.eip712;

import com.okx.x402.subscription.model.CancelAuth;
import com.okx.x402.subscription.model.PendingChangeCancelAuth;
import com.okx.x402.subscription.model.PermitSingle;
import com.okx.x402.subscription.model.SubscriptionTerms;
import org.web3j.crypto.Hash;
import org.web3j.utils.Numeric;

import java.math.BigInteger;
import java.nio.charset.StandardCharsets;

public final class SubscriptionEip712 {

    private SubscriptionEip712() {}

    // 17-field terms (calendar-month contract revision): periodMode is the last field.
    // Known-vector typehash: 0xa5223de56e7694cf776c7d4f74c0323f42bf9e65655fe49affefbdfd40ec97ae
    // (the old 16-field hash 0x3fd0…e811 is obsolete; signatures over it are unusable on the new contract).
    public static final String SUBSCRIPTION_TERMS_TYPE =
            "SubscriptionTerms(address payer,address merchant,address facilitator,address token,"
                    + "uint160 amountPerPeriod,uint64 periodSec,uint32 maxPeriods,uint64 startAt,"
                    + "uint32 initialChargePeriods,uint160 initialChargeAmount,uint64 termsDeadline,"
                    + "bytes32 permitHash,bytes32 salt,uint8 planTier,bytes32 changeFromSubId,"
                    + "uint8 changeEffectiveAt,uint8 periodMode)";

    public static final String CANCEL_AUTH_TYPE =
            "CancelAuth(uint8 action,bytes32 subId,uint8 initiator,bytes32 nonce,uint64 deadline)";

    // newSubId binds the cancellation to one specific scheduled downgrade.
    public static final String PENDING_CHANGE_CANCEL_AUTH_TYPE =
            "PendingChangeCancelAuth(bytes32 subId,bytes32 newSubId,bytes32 nonce,uint64 deadline)";

    public static final String PERMIT_SINGLE_TYPE =
            "PermitSingle(PermitDetails details,address spender,uint256 sigDeadline)"
                    + "PermitDetails(address token,uint160 amount,uint48 expiration,uint48 nonce)";

    private static final String PERMIT_DETAILS_TYPE =
            "PermitDetails(address token,uint160 amount,uint48 expiration,uint48 nonce)";

    private static final String EIP712_DOMAIN_TYPE =
            "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";

    public static final byte[] SUBSCRIPTION_TERMS_TYPEHASH =
            Hash.sha3(SUBSCRIPTION_TERMS_TYPE.getBytes(StandardCharsets.UTF_8));

    public static final byte[] CANCEL_AUTH_TYPEHASH =
            Hash.sha3(CANCEL_AUTH_TYPE.getBytes(StandardCharsets.UTF_8));

    public static final byte[] PENDING_CHANGE_CANCEL_AUTH_TYPEHASH =
            Hash.sha3(PENDING_CHANGE_CANCEL_AUTH_TYPE.getBytes(StandardCharsets.UTF_8));

    public static final byte[] PERMIT_SINGLE_TYPEHASH =
            Hash.sha3(PERMIT_SINGLE_TYPE.getBytes(StandardCharsets.UTF_8));

    private static final byte[] PERMIT_DETAILS_TYPEHASH =
            Hash.sha3(PERMIT_DETAILS_TYPE.getBytes(StandardCharsets.UTF_8));

    private static final byte[] EIP712_DOMAIN_TYPEHASH =
            Hash.sha3(EIP712_DOMAIN_TYPE.getBytes(StandardCharsets.UTF_8));

    private static final String DOMAIN_NAME = "A2APaySubscription";
    private static final String DOMAIN_VERSION = "1";

    public static byte[] subDomainSeparator(long chainId, String verifyingContract) {
        byte[] nameHash = Hash.sha3(DOMAIN_NAME.getBytes(StandardCharsets.UTF_8));
        byte[] versionHash = Hash.sha3(DOMAIN_VERSION.getBytes(StandardCharsets.UTF_8));
        byte[] chainIdBytes = Numeric.toBytesPadded(BigInteger.valueOf(chainId), 32);
        byte[] contractBytes = Numeric.toBytesPadded(Numeric.toBigInt(verifyingContract), 32);

        byte[] encoded = new byte[160];
        System.arraycopy(EIP712_DOMAIN_TYPEHASH, 0, encoded, 0, 32);
        System.arraycopy(nameHash, 0, encoded, 32, 32);
        System.arraycopy(versionHash, 0, encoded, 64, 32);
        System.arraycopy(chainIdBytes, 0, encoded, 96, 32);
        System.arraycopy(contractBytes, 0, encoded, 128, 32);
        return Hash.sha3(encoded);
    }

    public static byte[] hashTerms(SubscriptionTerms terms) {
        // 17 ABI fields + typehash = 18 slots × 32 bytes = 576 bytes
        byte[] encoded = new byte[576];
        int offset = 0;

        System.arraycopy(SUBSCRIPTION_TERMS_TYPEHASH, 0, encoded, offset, 32);
        offset += 32;

        // address payer
        System.arraycopy(padAddress(terms.payer), 0, encoded, offset, 32);
        offset += 32;
        // address merchant
        System.arraycopy(padAddress(terms.merchant), 0, encoded, offset, 32);
        offset += 32;
        // address facilitator
        System.arraycopy(padAddress(terms.facilitator), 0, encoded, offset, 32);
        offset += 32;
        // address token
        System.arraycopy(padAddress(terms.token), 0, encoded, offset, 32);
        offset += 32;
        // uint160 amountPerPeriod
        System.arraycopy(padUint(terms.amountPerPeriod), 0, encoded, offset, 32);
        offset += 32;
        // uint64 periodSec
        System.arraycopy(Numeric.toBytesPadded(BigInteger.valueOf(terms.periodSec), 32), 0, encoded, offset, 32);
        offset += 32;
        // uint32 maxPeriods
        System.arraycopy(Numeric.toBytesPadded(BigInteger.valueOf(terms.maxPeriods), 32), 0, encoded, offset, 32);
        offset += 32;
        // uint64 startAt
        System.arraycopy(Numeric.toBytesPadded(BigInteger.valueOf(terms.startAt), 32), 0, encoded, offset, 32);
        offset += 32;
        // uint32 initialChargePeriods
        System.arraycopy(Numeric.toBytesPadded(BigInteger.valueOf(terms.initialChargePeriods), 32), 0, encoded, offset, 32);
        offset += 32;
        // uint160 initialChargeAmount
        System.arraycopy(padUint(terms.initialChargeAmount), 0, encoded, offset, 32);
        offset += 32;
        // uint64 termsDeadline
        System.arraycopy(Numeric.toBytesPadded(BigInteger.valueOf(terms.termsDeadline), 32), 0, encoded, offset, 32);
        offset += 32;
        // bytes32 permitHash
        System.arraycopy(padBytes32(terms.permitHash), 0, encoded, offset, 32);
        offset += 32;
        // bytes32 salt
        System.arraycopy(padBytes32(terms.salt), 0, encoded, offset, 32);
        offset += 32;
        // uint8 planTier
        System.arraycopy(Numeric.toBytesPadded(BigInteger.valueOf(terms.planTier), 32), 0, encoded, offset, 32);
        offset += 32;
        // bytes32 changeFromSubId
        System.arraycopy(padBytes32(terms.changeFromSubId), 0, encoded, offset, 32);
        offset += 32;
        // uint8 changeEffectiveAt
        System.arraycopy(Numeric.toBytesPadded(BigInteger.valueOf(terms.changeEffectiveAt), 32), 0, encoded, offset, 32);
        offset += 32;
        // uint8 periodMode (17th field)
        System.arraycopy(Numeric.toBytesPadded(BigInteger.valueOf(terms.periodMode), 32), 0, encoded, offset, 32);

        return Hash.sha3(encoded);
    }

    public static byte[] termsDigest(SubscriptionTerms terms, long chainId, String contract) {
        byte[] domainSep = subDomainSeparator(chainId, contract);
        byte[] structHash = hashTerms(terms);
        return hashTypedData(domainSep, structHash);
    }

    public static String termsDigestHex(SubscriptionTerms terms, long chainId, String contract) {
        return Numeric.toHexStringNoPrefix(termsDigest(terms, chainId, contract));
    }

    public static byte[] hashPermitSingle(PermitSingle permit) {
        byte[] detailsHash = hashPermitDetails(permit);

        byte[] encoded = new byte[128];
        System.arraycopy(PERMIT_SINGLE_TYPEHASH, 0, encoded, 0, 32);
        System.arraycopy(detailsHash, 0, encoded, 32, 32);
        System.arraycopy(padAddress(permit.spender), 0, encoded, 64, 32);
        System.arraycopy(Numeric.toBytesPadded(new BigInteger(permit.sigDeadline), 32), 0, encoded, 96, 32);

        return Hash.sha3(encoded);
    }

    public static String hashPermitSingleHex(PermitSingle permit) {
        return Numeric.toHexString(hashPermitSingle(permit));
    }

    public static byte[] hashCancelAuth(CancelAuth auth, long chainId, String contract) {
        byte[] domainSep = subDomainSeparator(chainId, contract);

        byte[] encoded = new byte[192];
        System.arraycopy(CANCEL_AUTH_TYPEHASH, 0, encoded, 0, 32);
        System.arraycopy(Numeric.toBytesPadded(BigInteger.valueOf(auth.action), 32), 0, encoded, 32, 32);
        System.arraycopy(padBytes32(auth.subId), 0, encoded, 64, 32);
        System.arraycopy(Numeric.toBytesPadded(BigInteger.valueOf(auth.initiator), 32), 0, encoded, 96, 32);
        System.arraycopy(padBytes32(auth.nonce), 0, encoded, 128, 32);
        System.arraycopy(Numeric.toBytesPadded(BigInteger.valueOf(auth.deadline), 32), 0, encoded, 160, 32);

        byte[] structHash = Hash.sha3(encoded);
        return hashTypedData(domainSep, structHash);
    }

    public static byte[] hashPendingChangeCancelAuth(PendingChangeCancelAuth auth,
                                                     long chainId, String contract) {
        byte[] domainSep = subDomainSeparator(chainId, contract);

        byte[] encoded = new byte[160];
        System.arraycopy(PENDING_CHANGE_CANCEL_AUTH_TYPEHASH, 0, encoded, 0, 32);
        System.arraycopy(padBytes32(auth.subId), 0, encoded, 32, 32);
        System.arraycopy(padBytes32(auth.newSubId), 0, encoded, 64, 32);
        System.arraycopy(padBytes32(auth.nonce), 0, encoded, 96, 32);
        System.arraycopy(Numeric.toBytesPadded(BigInteger.valueOf(auth.deadline), 32), 0, encoded, 128, 32);

        byte[] structHash = Hash.sha3(encoded);
        return hashTypedData(domainSep, structHash);
    }

    private static byte[] hashPermitDetails(PermitSingle permit) {
        byte[] encoded = new byte[160];
        System.arraycopy(PERMIT_DETAILS_TYPEHASH, 0, encoded, 0, 32);
        System.arraycopy(padAddress(permit.details.token), 0, encoded, 32, 32);
        System.arraycopy(padUint(permit.details.amount), 0, encoded, 64, 32);
        System.arraycopy(Numeric.toBytesPadded(BigInteger.valueOf(permit.details.expiration), 32), 0, encoded, 96, 32);
        System.arraycopy(Numeric.toBytesPadded(BigInteger.valueOf(permit.details.nonce), 32), 0, encoded, 128, 32);
        return Hash.sha3(encoded);
    }

    private static byte[] hashTypedData(byte[] domainSeparator, byte[] structHash) {
        byte[] encoded = new byte[66];
        encoded[0] = 0x19;
        encoded[1] = 0x01;
        System.arraycopy(domainSeparator, 0, encoded, 2, 32);
        System.arraycopy(structHash, 0, encoded, 34, 32);
        return Hash.sha3(encoded);
    }

    private static byte[] padAddress(String address) {
        return Numeric.toBytesPadded(Numeric.toBigInt(address), 32);
    }

    private static byte[] padUint(String decimalValue) {
        return Numeric.toBytesPadded(new BigInteger(decimalValue), 32);
    }

    private static byte[] padBytes32(String hexValue) {
        return Numeric.hexStringToByteArray(
                hexValue.startsWith("0x") ? hexValue.substring(2) : hexValue);
    }
}
