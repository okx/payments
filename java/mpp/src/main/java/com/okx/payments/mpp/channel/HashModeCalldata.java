package com.okx.payments.mpp.channel;

import org.web3j.abi.FunctionEncoder;
import org.web3j.abi.TypeReference;
import org.web3j.abi.datatypes.Address;
import org.web3j.abi.datatypes.DynamicArray;
import org.web3j.abi.datatypes.Function;
import org.web3j.abi.datatypes.Type;
import org.web3j.abi.datatypes.generated.Bytes32;
import org.web3j.abi.datatypes.generated.Uint128;
import org.web3j.abi.datatypes.generated.Uint16;
import org.web3j.abi.datatypes.generated.Uint256;

import java.math.BigInteger;
import java.security.SecureRandom;
import java.util.Collections;
import java.util.List;

/**
 * Buyer-side EVM calldata encoder for MPP <strong>hash mode</strong> session open / topUp.
 *
 * <p>In hash mode the buyer broadcasts the on-chain leg themselves (instead of letting the
 * SA relayer broadcast an EIP-3009 envelope). Two transactions are required at open:
 *
 * <ol>
 *   <li>{@code USD₮0.approve(escrow, deposit)} — allow the escrow to pull the deposit.</li>
 *   <li>{@code escrow.open(payee, token, deposit, salt, authorizedSigner, [], [])} —
 *       creates the channel and pulls the deposit via {@code transferFrom}.</li>
 * </ol>
 *
 * <p>For topUp only one transaction is needed (assuming approve allowance is already
 * sufficient, otherwise re-approve first):
 *
 * <ul>
 *   <li>{@code escrow.topUp(channelId, additionalDeposit)}</li>
 * </ul>
 *
 * <p>This mirrors the Go reference helper authored by the Go SDK team (see
 * {@code go/demo/hash-calldata} in mpp-rs / mpp-go). The Solidity ABI for {@code open}
 * is {@code (address payee, address token, uint128 deposit, bytes32 salt,
 * address authorizedSigner, address[] splitRecipients, uint16[] splitBps)}.
 *
 * <p>Channel-id binding: the escrow derives {@code channelId} on-chain as
 * {@code keccak256(payer, payee, token, salt, authorizedSigner, escrow, chainId)} —
 * exactly what {@link ChannelIdCalculator#compute} reproduces off-chain. So as long as
 * the buyer's wallet broadcasts {@code open(...)} with the same {@code salt} that the
 * SDK / CLI used to compute {@code channelId}, the on-chain and off-chain ids match.
 */
public final class HashModeCalldata {

    private HashModeCalldata() {}

    /** Random 32-byte salt for new {@code open} calls. Hex-encoded with {@code 0x} prefix. */
    public static String generateSalt() {
        byte[] buf = new byte[32];
        new SecureRandom().nextBytes(buf);
        return "0x" + bytesToHex(buf);
    }

    /**
     * ERC-20 {@code approve(address spender, uint256 amount)} calldata.
     *
     * @param spender contract authorized to pull (the MPP escrow address)
     * @param amount  approval cap in atomic units
     * @return {@code 0x}-prefixed hex calldata
     */
    public static String encodeApprove(String spender, BigInteger amount) {
        Function fn = new Function(
            "approve",
            List.of(new Address(spender), new Uint256(amount)),
            Collections.<TypeReference<?>>emptyList());
        return FunctionEncoder.encode(fn);
    }

    /**
     * Escrow {@code open(payee, token, deposit, salt, authorizedSigner, [], [])} calldata.
     * The split arrays are intentionally empty — match the Go reference's no-splits default.
     *
     * @param payee            seller / payee address
     * @param token            ERC-20 token contract used for the channel (e.g. USD₮0)
     * @param deposit          deposit in atomic units (uint128)
     * @param salt32           {@code 0x}-prefixed 32-byte hex salt
     * @param authorizedSigner authorized voucher signer (zero address → payer signs themselves)
     * @return {@code 0x}-prefixed hex calldata
     */
    public static String encodeOpen(String payee,
                                    String token,
                                    BigInteger deposit,
                                    String salt32,
                                    String authorizedSigner) {
        byte[] salt = hexToBytes32(salt32);
        Function fn = new Function(
            "open",
            List.of(
                new Address(payee),
                new Address(token),
                new Uint128(deposit),
                new Bytes32(salt),
                new Address(authorizedSigner == null || authorizedSigner.isBlank()
                    ? ChannelIdCalculator.ZERO_ADDRESS : authorizedSigner),
                new DynamicArray<>(Address.class, Collections.<Address>emptyList()),
                new DynamicArray<>(Uint16.class,  Collections.<Uint16>emptyList())
            ),
            Collections.<TypeReference<?>>emptyList());
        return FunctionEncoder.encode(fn);
    }

    /**
     * Escrow {@code topUp(bytes32 channelId, uint128 additionalDeposit)} calldata.
     *
     * @param channelId32       {@code 0x}-prefixed 32-byte hex channel id (from open output)
     * @param additionalDeposit atomic units to add to existing deposit
     * @return {@code 0x}-prefixed hex calldata
     */
    public static String encodeTopUp(String channelId32, BigInteger additionalDeposit) {
        Function fn = new Function(
            "topUp",
            List.of(new Bytes32(hexToBytes32(channelId32)), new Uint128(additionalDeposit)),
            Collections.<TypeReference<?>>emptyList());
        return FunctionEncoder.encode(fn);
    }

    private static byte[] hexToBytes32(String hex) {
        String body = hex.startsWith("0x") || hex.startsWith("0X") ? hex.substring(2) : hex;
        if (body.length() != 64) {
            throw new IllegalArgumentException(
                "expected 32-byte hex (64 chars excl. 0x), got " + body.length());
        }
        byte[] out = new byte[32];
        for (int i = 0; i < 32; i++) {
            out[i] = (byte) Integer.parseInt(body, i * 2, i * 2 + 2, 16);
        }
        return out;
    }

    private static String bytesToHex(byte[] b) {
        StringBuilder sb = new StringBuilder(b.length * 2);
        for (byte x : b) sb.append(String.format("%02x", x & 0xff));
        return sb.toString();
    }
}
