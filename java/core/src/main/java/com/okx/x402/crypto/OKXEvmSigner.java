// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.crypto;

import com.okx.x402.model.v2.PaymentRequirements;

import org.web3j.crypto.ECKeyPair;
import org.web3j.crypto.Hash;
import org.web3j.crypto.Keys;
import org.web3j.crypto.Sign;
import org.web3j.utils.Numeric;

import java.math.BigInteger;
import java.nio.charset.StandardCharsets;
import java.security.SecureRandom;
import java.time.Instant;
import java.util.LinkedHashMap;
import java.util.Map;

/**
 * EVM signer that produces EIP-3009 TransferWithAuthorization signatures.
 * Uses web3j for EIP-712 typed data signing on X Layer.
 */
public class OKXEvmSigner implements EvmSigner {

    private static final byte[] EIP712_DOMAIN_TYPEHASH = Hash.sha3(
            "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"
                    .getBytes(StandardCharsets.UTF_8));

    private static final byte[] TRANSFER_WITH_AUTHORIZATION_TYPEHASH = Hash.sha3(
            ("TransferWithAuthorization(address from,address to,uint256 value,"
                    + "uint256 validAfter,uint256 validBefore,bytes32 nonce)")
                    .getBytes(StandardCharsets.UTF_8));

    private static final SecureRandom SECURE_RANDOM = new SecureRandom();

    private final ECKeyPair keyPair;
    private final String address;

    /**
     * Creates an OKXEvmSigner from a hex private key.
     *
     * @param privateKeyHex hex private key, optionally 0x-prefixed
     */
    public OKXEvmSigner(String privateKeyHex) {
        String cleaned = privateKeyHex.startsWith("0x")
                ? privateKeyHex.substring(2) : privateKeyHex;
        this.keyPair = ECKeyPair.create(new BigInteger(cleaned, 16));
        this.address = Keys.toChecksumAddress("0x" + Keys.getAddress(keyPair));
    }

    @Override
    public String getAddress() {
        return address;
    }

    @Override
    public Map<String, Object> signPaymentRequirements(PaymentRequirements requirements)
            throws CryptoSignException {
        if (requirements == null) {
            throw new CryptoSignException("PaymentRequirements must not be null");
        }
        if (requirements.payTo == null || requirements.amount == null
                || requirements.network == null) {
            throw new CryptoSignException(
                    "PaymentRequirements.payTo, amount, and network are required");
        }

        // Dispatch by extra.assetTransferMethod — default is "eip3009" (EIP-3009
        // TransferWithAuthorization). "permit2" routes to one of two Permit2
        // variants depending on the scheme ("exact" or "upto").
        String transferMethod = requirements.extra == null ? null
                : (String) requirements.extra.get("assetTransferMethod");
        if (transferMethod == null) {
            transferMethod = (String) (requirements.extra == null
                    ? null : requirements.extra.get("transferMethod"));
        }

        if ("permit2".equalsIgnoreCase(transferMethod)) {
            if ("upto".equalsIgnoreCase(requirements.scheme)) {
                return signUptoPermit2(requirements);
            }
            return signExactPermit2(requirements);
        }

        return signEip3009(requirements);
    }

    private Map<String, Object> signEip3009(PaymentRequirements requirements)
            throws CryptoSignException {
        if (requirements.extra == null || !requirements.extra.containsKey("name")
                || !requirements.extra.containsKey("version")) {
            throw new CryptoSignException(
                    "PaymentRequirements.extra must contain 'name' and 'version'"
                            + " for EIP-3009 signing");
        }
        try {
            long now = Instant.now().getEpochSecond();
            String validAfter = String.valueOf(now - 5);
            String validBefore = String.valueOf(now + requirements.maxTimeoutSeconds);

            byte[] nonceBytes = new byte[32];
            SECURE_RANDOM.nextBytes(nonceBytes);
            String nonce = Numeric.toHexStringWithPrefix(new BigInteger(1, nonceBytes));

            String from = address;
            String to = requirements.payTo;
            String value = requirements.amount;

            String domainName = (String) requirements.extra.get("name");
            String domainVersion = (String) requirements.extra.get("version");
            long chainId = extractChainId(requirements.network);

            byte[] digest = buildEIP712Hash(
                    domainName, domainVersion, chainId, requirements.asset,
                    from, to, value, validAfter, validBefore, nonce);

            Sign.SignatureData sig = Sign.signMessage(digest, keyPair, false);
            String signature = toHexSignature(sig);

            Map<String, Object> payload = new LinkedHashMap<>();
            payload.put("signature", signature);
            payload.put("authorization", Map.of(
                    "from", from,
                    "to", to,
                    "value", value,
                    "validAfter", validAfter,
                    "validBefore", validBefore,
                    "nonce", nonce
            ));
            return payload;

        } catch (Exception e) {
            throw new CryptoSignException("EIP-3009 signing failed", e);
        }
    }

    /**
     * Sign an {@code exact + permit2} payload. Returns
     * {@code {signature, permit2Authorization:{from, permitted, spender, nonce,
     * deadline, witness:{to, validAfter}}}}.
     */
    private Map<String, Object> signExactPermit2(PaymentRequirements requirements)
            throws CryptoSignException {
        if (requirements.asset == null) {
            throw new CryptoSignException("PaymentRequirements.asset is required for Permit2");
        }
        try {
            long chainId = extractChainId(requirements.network);
            long now = Instant.now().getEpochSecond();
            String validAfter = "0";
            String deadline = Long.toString(now + requirements.maxTimeoutSeconds
                    + Permit2Constants.PERMIT2_DEADLINE_BUFFER_SECONDS);

            byte[] nonceBytes = new byte[32];
            SECURE_RANDOM.nextBytes(nonceBytes);
            String nonce = new BigInteger(1, nonceBytes).toString();

            byte[] domainSeparator = Permit2Eip712.buildDomainSeparator(chainId);
            byte[] digest = Permit2Eip712.buildExactTypedDataHash(
                    domainSeparator,
                    requirements.asset, requirements.amount,
                    Permit2Constants.EXACT_PERMIT2_PROXY_ADDRESS,
                    nonce, deadline,
                    requirements.payTo, validAfter);

            String signature = toHexSignature(Sign.signMessage(digest, keyPair, false));

            Map<String, Object> permit2Auth = new LinkedHashMap<>();
            permit2Auth.put("from", address);
            permit2Auth.put("permitted", Map.of(
                    "token", requirements.asset,
                    "amount", requirements.amount));
            permit2Auth.put("spender", Permit2Constants.EXACT_PERMIT2_PROXY_ADDRESS);
            permit2Auth.put("nonce", nonce);
            permit2Auth.put("deadline", deadline);
            permit2Auth.put("witness", Map.of(
                    "to", requirements.payTo,
                    "validAfter", validAfter));

            Map<String, Object> payload = new LinkedHashMap<>();
            payload.put("signature", signature);
            payload.put("permit2Authorization", permit2Auth);
            return payload;

        } catch (Exception e) {
            throw new CryptoSignException("exact+permit2 signing failed", e);
        }
    }

    /**
     * Sign an {@code upto + permit2} payload. Requires
     * {@code extra.facilitatorAddress} to be present so the witness binds the
     * settling facilitator into the EIP-712 hash.
     */
    private Map<String, Object> signUptoPermit2(PaymentRequirements requirements)
            throws CryptoSignException {
        if (requirements.asset == null) {
            throw new CryptoSignException("PaymentRequirements.asset is required for Permit2");
        }
        if (requirements.extra == null
                || !requirements.extra.containsKey("facilitatorAddress")) {
            throw new CryptoSignException(
                    "upto+permit2 requires extra.facilitatorAddress in PaymentRequirements");
        }
        String facilitatorAddress = (String) requirements.extra.get("facilitatorAddress");
        if (facilitatorAddress == null || facilitatorAddress.isBlank()) {
            throw new CryptoSignException(
                    "extra.facilitatorAddress must not be null or blank for upto+permit2");
        }
        try {
            long chainId = extractChainId(requirements.network);
            long now = Instant.now().getEpochSecond();
            String validAfter = "0";
            String deadline = Long.toString(now + requirements.maxTimeoutSeconds
                    + Permit2Constants.PERMIT2_DEADLINE_BUFFER_SECONDS);

            byte[] nonceBytes = new byte[32];
            SECURE_RANDOM.nextBytes(nonceBytes);
            String nonce = new BigInteger(1, nonceBytes).toString();

            byte[] domainSeparator = Permit2Eip712.buildDomainSeparator(chainId);
            byte[] digest = Permit2Eip712.buildUptoTypedDataHash(
                    domainSeparator,
                    requirements.asset, requirements.amount,
                    Permit2Constants.UPTO_PERMIT2_PROXY_ADDRESS,
                    nonce, deadline,
                    requirements.payTo, facilitatorAddress, validAfter);

            String signature = toHexSignature(Sign.signMessage(digest, keyPair, false));

            Map<String, Object> permit2Auth = new LinkedHashMap<>();
            permit2Auth.put("from", address);
            permit2Auth.put("permitted", Map.of(
                    "token", requirements.asset,
                    "amount", requirements.amount));
            permit2Auth.put("spender", Permit2Constants.UPTO_PERMIT2_PROXY_ADDRESS);
            permit2Auth.put("nonce", nonce);
            permit2Auth.put("deadline", deadline);
            permit2Auth.put("witness", Map.of(
                    "to", requirements.payTo,
                    "facilitator", facilitatorAddress,
                    "validAfter", validAfter));

            Map<String, Object> payload = new LinkedHashMap<>();
            payload.put("signature", signature);
            payload.put("permit2Authorization", permit2Auth);
            return payload;

        } catch (Exception e) {
            throw new CryptoSignException("upto+permit2 signing failed", e);
        }
    }

    /**
     * Build the EIP-712 hash for TransferWithAuthorization.
     *
     * @param name domain name
     * @param version domain version
     * @param chainId chain ID
     * @param verifyingContract verifying contract address
     * @param from sender address
     * @param to recipient address
     * @param value transfer value
     * @param validAfter valid-after timestamp
     * @param validBefore valid-before timestamp
     * @param nonce authorization nonce
     * @return EIP-712 digest bytes
     */
    byte[] buildEIP712Hash(String name, String version, long chainId,
                           String verifyingContract,
                           String from, String to, String value,
                           String validAfter, String validBefore, String nonce) {
        // Domain separator
        byte[] domainSeparator = Hash.sha3(concat(
                EIP712_DOMAIN_TYPEHASH,
                Hash.sha3(name.getBytes(StandardCharsets.UTF_8)),
                Hash.sha3(version.getBytes(StandardCharsets.UTF_8)),
                padLeft32(BigInteger.valueOf(chainId)),
                padLeft32(new BigInteger(verifyingContract.substring(2), 16))
        ));

        // Struct hash
        byte[] structHash = Hash.sha3(concat(
                TRANSFER_WITH_AUTHORIZATION_TYPEHASH,
                padLeft32(new BigInteger(from.substring(2), 16)),
                padLeft32(new BigInteger(to.substring(2), 16)),
                padLeft32(new BigInteger(value)),
                padLeft32(new BigInteger(validAfter)),
                padLeft32(new BigInteger(validBefore)),
                Numeric.hexStringToByteArray(nonce.startsWith("0x") ? nonce.substring(2) : nonce)
        ));

        // Final hash: keccak256("\x19\x01" + domainSeparator + structHash)
        byte[] prefix = new byte[]{0x19, 0x01};
        return Hash.sha3(concat(prefix, domainSeparator, structHash));
    }

    /**
     * Extract chain ID from an EVM CAIP-2 network string (e.g. "eip155:196").
     *
     * <p>Strict about the {@code eip155:} namespace: any other CAIP-2 prefix
     * (e.g. {@code solana:…}) would indicate the caller is misusing an EVM
     * signer for a non-EVM chain, so we fail loudly instead of returning a
     * plausible but meaningless chain id.
     *
     * @param network network identifier
     * @return chain ID
     */
    static long extractChainId(String network) {
        if (network == null || !network.startsWith("eip155:")) {
            throw new IllegalArgumentException(
                    "Invalid CAIP-2 network: " + network
                            + " (expected 'eip155:<chainId>')");
        }
        String idStr = network.substring("eip155:".length());
        try {
            // EIP-155 chainId is uint256; a 64-bit long covers every real EVM
            // chain id (largest known are far below 2^53) without the overflow
            // that Integer.parseInt would hit above 2^31-1.
            return Long.parseLong(idStr);
        } catch (NumberFormatException e) {
            throw new IllegalArgumentException(
                    "Invalid CAIP-2 chain id: " + network, e);
        }
    }

    private static String toHexSignature(Sign.SignatureData sig) {
        byte[] r = sig.getR();
        byte[] s = sig.getS();
        byte v = sig.getV()[0];
        byte[] full = new byte[65];
        System.arraycopy(r, 0, full, 0, 32);
        System.arraycopy(s, 0, full, 32, 32);
        full[64] = v;
        return "0x" + Numeric.toHexStringNoPrefixZeroPadded(new BigInteger(1, full), 130);
    }

    private static byte[] padLeft32(BigInteger val) {
        byte[] raw = val.toByteArray();
        byte[] padded = new byte[32];
        if (raw.length <= 32) {
            System.arraycopy(raw, 0, padded, 32 - raw.length, raw.length);
        } else {
            // Remove leading zero byte from BigInteger two's complement
            System.arraycopy(raw, raw.length - 32, padded, 0, 32);
        }
        return padded;
    }

    private static byte[] concat(byte[]... arrays) {
        int totalLen = 0;
        for (byte[] a : arrays) {
            totalLen += a.length;
        }
        byte[] result = new byte[totalLen];
        int pos = 0;
        for (byte[] a : arrays) {
            System.arraycopy(a, 0, result, pos, a.length);
            pos += a.length;
        }
        return result;
    }
}
