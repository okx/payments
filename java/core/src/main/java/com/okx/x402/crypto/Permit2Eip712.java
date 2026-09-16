// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.crypto;

import org.web3j.crypto.Hash;
import org.web3j.utils.Numeric;

import java.math.BigInteger;
import java.nio.charset.StandardCharsets;

/**
 * EIP-712 typed-data hash construction for the Permit2 schemes.
 *
 * <p>Builds the domain separator and {@code PermitWitnessTransferFrom} struct
 * hash for both the {@code exact} and {@code upto} Permit2 schemes. The output
 * 32-byte digest is then passed to the secp256k1 signer (see
 * {@link OKXEvmSigner#signPaymentRequirements}).
 *
 * <p>The Permit2 domain is {@code (name="Permit2", chainId, verifyingContract=Permit2)};
 * note that — unlike the EIP-3009 domain on USDT — there is no {@code version}
 * field.
 */
public final class Permit2Eip712 {

    private Permit2Eip712() {}

    private static final byte[] EIP712_DOMAIN_TYPEHASH = Hash.sha3(
            "EIP712Domain(string name,uint256 chainId,address verifyingContract)"
                    .getBytes(StandardCharsets.UTF_8));

    private static final byte[] TOKEN_PERMISSIONS_TYPEHASH = Hash.sha3(
            Permit2Constants.TOKEN_PERMISSIONS_TYPE.getBytes(StandardCharsets.UTF_8));

    private static final byte[] EXACT_PERMIT_WITNESS_TRANSFER_FROM_TYPEHASH = Hash.sha3(
            Permit2Constants.PERMIT_WITNESS_TRANSFER_FROM_TYPE_EXACT
                    .getBytes(StandardCharsets.UTF_8));

    private static final byte[] UPTO_PERMIT_WITNESS_TRANSFER_FROM_TYPEHASH = Hash.sha3(
            Permit2Constants.PERMIT_WITNESS_TRANSFER_FROM_TYPE_UPTO
                    .getBytes(StandardCharsets.UTF_8));

    private static final byte[] EXACT_WITNESS_TYPEHASH = Hash.sha3(
            Permit2Constants.EXACT_WITNESS_TYPE.getBytes(StandardCharsets.UTF_8));

    private static final byte[] UPTO_WITNESS_TYPEHASH = Hash.sha3(
            Permit2Constants.UPTO_WITNESS_TYPE.getBytes(StandardCharsets.UTF_8));

    /**
     * Build the Permit2 EIP-712 domain separator for the given chain.
     *
     * @param chainId the EVM chain id (e.g. {@code 196} for X Layer)
     * @return 32-byte domain separator
     */
    public static byte[] buildDomainSeparator(long chainId) {
        byte[] nameHash = Hash.sha3(
                Permit2Constants.PERMIT2_DOMAIN_NAME.getBytes(StandardCharsets.UTF_8));
        byte[] chainIdBytes = Numeric.toBytesPadded(BigInteger.valueOf(chainId), 32);
        byte[] verifyingContract = Numeric.toBytesPadded(
                Numeric.toBigInt(Permit2Constants.PERMIT2_ADDRESS), 32);

        byte[] encoded = new byte[128];
        System.arraycopy(EIP712_DOMAIN_TYPEHASH, 0, encoded, 0, 32);
        System.arraycopy(nameHash, 0, encoded, 32, 32);
        System.arraycopy(chainIdBytes, 0, encoded, 64, 32);
        System.arraycopy(verifyingContract, 0, encoded, 96, 32);

        return Hash.sha3(encoded);
    }

    /**
     * Build the full EIP-712 typed-data hash for the {@code exact} Permit2 scheme.
     *
     * @param domainSeparator   32-byte domain separator
     * @param token             token contract address (0x-prefixed)
     * @param amount            permitted amount, uint256 decimal string
     * @param spender           exact-Permit2 proxy address
     * @param nonce             uint256 decimal string
     * @param deadline          uint256 decimal string
     * @param witnessTo         witness recipient (payTo) address
     * @param witnessValidAfter uint256 decimal string
     * @return 32-byte digest ready for secp256k1 signing
     */
    public static byte[] buildExactTypedDataHash(
            byte[] domainSeparator,
            String token, String amount,
            String spender, String nonce, String deadline,
            String witnessTo, String witnessValidAfter) {
        byte[] witnessHash = hashExactWitness(witnessTo, witnessValidAfter);
        byte[] tokenPermissionsHash = hashTokenPermissions(token, amount);
        byte[] structHash = hashPermitWitnessTransferFrom(
                EXACT_PERMIT_WITNESS_TRANSFER_FROM_TYPEHASH,
                tokenPermissionsHash, spender, nonce, deadline, witnessHash);
        return hashTypedData(domainSeparator, structHash);
    }

    /**
     * Build the full EIP-712 typed-data hash for the {@code upto} Permit2 scheme.
     *
     * @param domainSeparator     32-byte domain separator
     * @param token               token contract address (0x-prefixed)
     * @param amount              upper-cap amount, uint256 decimal string
     * @param spender             upto-Permit2 proxy address
     * @param nonce               uint256 decimal string
     * @param deadline            uint256 decimal string
     * @param witnessTo           witness recipient (payTo) address
     * @param witnessFacilitator  facilitator wallet address bound to the witness
     * @param witnessValidAfter   uint256 decimal string
     * @return 32-byte digest ready for secp256k1 signing
     */
    public static byte[] buildUptoTypedDataHash(
            byte[] domainSeparator,
            String token, String amount,
            String spender, String nonce, String deadline,
            String witnessTo, String witnessFacilitator, String witnessValidAfter) {
        byte[] witnessHash = hashUptoWitness(witnessTo, witnessFacilitator, witnessValidAfter);
        byte[] tokenPermissionsHash = hashTokenPermissions(token, amount);
        byte[] structHash = hashPermitWitnessTransferFrom(
                UPTO_PERMIT_WITNESS_TRANSFER_FROM_TYPEHASH,
                tokenPermissionsHash, spender, nonce, deadline, witnessHash);
        return hashTypedData(domainSeparator, structHash);
    }

    private static byte[] hashExactWitness(String to, String validAfter) {
        byte[] toBytes = Numeric.toBytesPadded(Numeric.toBigInt(to), 32);
        byte[] validAfterBytes = Numeric.toBytesPadded(new BigInteger(validAfter), 32);

        byte[] encoded = new byte[96];
        System.arraycopy(EXACT_WITNESS_TYPEHASH, 0, encoded, 0, 32);
        System.arraycopy(toBytes, 0, encoded, 32, 32);
        System.arraycopy(validAfterBytes, 0, encoded, 64, 32);
        return Hash.sha3(encoded);
    }

    private static byte[] hashUptoWitness(String to, String facilitator, String validAfter) {
        byte[] toBytes = Numeric.toBytesPadded(Numeric.toBigInt(to), 32);
        byte[] facilitatorBytes = Numeric.toBytesPadded(Numeric.toBigInt(facilitator), 32);
        byte[] validAfterBytes = Numeric.toBytesPadded(new BigInteger(validAfter), 32);

        byte[] encoded = new byte[128];
        System.arraycopy(UPTO_WITNESS_TYPEHASH, 0, encoded, 0, 32);
        System.arraycopy(toBytes, 0, encoded, 32, 32);
        System.arraycopy(facilitatorBytes, 0, encoded, 64, 32);
        System.arraycopy(validAfterBytes, 0, encoded, 96, 32);
        return Hash.sha3(encoded);
    }

    private static byte[] hashTokenPermissions(String token, String amount) {
        byte[] tokenBytes = Numeric.toBytesPadded(Numeric.toBigInt(token), 32);
        byte[] amountBytes = Numeric.toBytesPadded(new BigInteger(amount), 32);

        byte[] encoded = new byte[96];
        System.arraycopy(TOKEN_PERMISSIONS_TYPEHASH, 0, encoded, 0, 32);
        System.arraycopy(tokenBytes, 0, encoded, 32, 32);
        System.arraycopy(amountBytes, 0, encoded, 64, 32);
        return Hash.sha3(encoded);
    }

    private static byte[] hashPermitWitnessTransferFrom(
            byte[] typeHash,
            byte[] tokenPermissionsHash,
            String spender,
            String nonce,
            String deadline,
            byte[] witnessHash) {
        byte[] spenderBytes = Numeric.toBytesPadded(Numeric.toBigInt(spender), 32);
        byte[] nonceBytes = Numeric.toBytesPadded(new BigInteger(nonce), 32);
        byte[] deadlineBytes = Numeric.toBytesPadded(new BigInteger(deadline), 32);

        byte[] encoded = new byte[192];
        System.arraycopy(typeHash, 0, encoded, 0, 32);
        System.arraycopy(tokenPermissionsHash, 0, encoded, 32, 32);
        System.arraycopy(spenderBytes, 0, encoded, 64, 32);
        System.arraycopy(nonceBytes, 0, encoded, 96, 32);
        System.arraycopy(deadlineBytes, 0, encoded, 128, 32);
        System.arraycopy(witnessHash, 0, encoded, 160, 32);
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
}
