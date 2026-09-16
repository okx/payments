package com.okx.payments.mpp.seller;

import com.okx.payments.mpp.voucher.EvmPaymentChannelDomain;

import java.math.BigInteger;

/**
 * Signs the merchant-side {@code SettleAuthorization} / {@code CloseAuthorization}
 * EIP-712 messages required by the on-chain {@code settleWithAuthorization} /
 * {@code closeWithAuthorization} entry points.
 *
 * <p>Key custody is the implementer's choice — three tiers are supported:
 * env-var (dev), self-held bytes, KMS/Ledger (prod). The default
 * {@link PrivateKeyPayeeAuthSigner} covers the first two.
 */
public interface PayeeAuthSigner {

    /** Sign {@code SettleAuthorization(bytes32 channelId, uint128 cumulativeAmount, uint256 nonce, uint256 deadline)}. */
    byte[] signSettleAuthorization(EvmPaymentChannelDomain domain, AuthData data);

    /** Sign {@code CloseAuthorization(bytes32 channelId, uint128 cumulativeAmount, uint256 nonce, uint256 deadline)}. */
    byte[] signCloseAuthorization(EvmPaymentChannelDomain domain, AuthData data);

    /** Lowercase 0x-prefixed payee address derived from the signing key. */
    String address();

    record AuthData(String channelId, BigInteger cumulativeAmount, BigInteger nonce, BigInteger deadline) {}
}
