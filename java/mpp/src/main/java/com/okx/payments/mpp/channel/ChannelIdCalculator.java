package com.okx.payments.mpp.channel;

import com.okx.payments.mpp.voucher.Eip712Hashing;
import org.web3j.crypto.Hash;
import org.web3j.utils.Numeric;

/**
 * Mirror of the on-chain {@code computeChannelId(payer, payee, token, salt, authorizedSigner)} —
 * but the actual contract additionally binds {@code address(this)} (the escrow) and
 * {@code block.chainid}, so we compute the full 7-field keccak.
 *
 * <p>Sentinel: when payer signs vouchers themselves, the client passes
 * {@code authorizedSigner = 0x000…0000}. The {@code payer == authorizedSigner} case is
 * rejected by SA gateway with code 70000 — caller must enforce.
 */
public final class ChannelIdCalculator {

    public static final String ZERO_ADDRESS = "0x0000000000000000000000000000000000000000";

    private ChannelIdCalculator() {}

    public static String compute(String payer,
                                 String payee,
                                 String token,
                                 String saltBytes32,
                                 String authorizedSigner,
                                 String escrow,
                                 long chainId) {
        byte[] encoded = Eip712Hashing.concat(
            Eip712Hashing.address(payer),
            Eip712Hashing.address(payee),
            Eip712Hashing.address(token),
            Eip712Hashing.bytes32(saltBytes32),
            Eip712Hashing.address(authorizedSigner),
            Eip712Hashing.address(escrow),
            Eip712Hashing.uint256(java.math.BigInteger.valueOf(chainId)));
        return Numeric.toHexString(Hash.sha3(encoded));
    }
}
