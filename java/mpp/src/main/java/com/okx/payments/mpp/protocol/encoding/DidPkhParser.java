package com.okx.payments.mpp.protocol.encoding;

import com.okx.payments.mpp.errors.InvalidPayloadError;

/**
 * Parse a CAIP-10 {@code did:pkh:eip155:<chainId>:<0xAddr>} identifier — mirrors Rust mpp
 * {@code parse_did_pkh_eip155}. Used by hash-mode charge / session paths where {@code source}
 * carries the payer DID.
 *
 * <p>Shared between {@code EvmChargeMethod} (hash payload) and {@code EvmSessionMethod}
 * (hash-mode open / topUp). T1-5 / T1-7 reviewers: extracted from
 * {@code EvmSessionMethod.parseDidPkhEip155} so charge and session use the same parser.
 */
public final class DidPkhParser {

    private static final String PREFIX = "did:pkh:eip155:";

    private DidPkhParser() {}

    /**
     * Parse and return the 0x-prefixed 20-byte address from a {@code did:pkh:eip155} DID.
     *
     * @throws InvalidPayloadError when DID is null/blank, malformed, or its chainId doesn't
     *                             match {@code expectedChainId}
     */
    public static String parseEvmAddress(String didPkh, long expectedChainId) {
        if (didPkh == null || didPkh.isBlank()) {
            throw new InvalidPayloadError("hash mode credential missing source");
        }
        if (!didPkh.startsWith(PREFIX)) {
            throw new InvalidPayloadError(
                "source DID must start with did:pkh:eip155: (" + didPkh + ")");
        }
        String rest = didPkh.substring(PREFIX.length());
        int sep = rest.indexOf(':');
        if (sep < 0) {
            throw new InvalidPayloadError(
                "source DID missing address segment (" + didPkh + ")");
        }
        String chainIdStr = rest.substring(0, sep);
        String addrStr = rest.substring(sep + 1);
        if (chainIdStr.length() > 1 && chainIdStr.startsWith("0")) {
            throw new InvalidPayloadError(
                "source DID chainId has leading zero: " + chainIdStr);
        }
        long parsed;
        try {
            parsed = Long.parseLong(chainIdStr);
        } catch (NumberFormatException e) {
            throw new InvalidPayloadError("invalid chainId in source DID: " + e.getMessage());
        }
        if (parsed != expectedChainId) {
            throw new InvalidPayloadError(
                "source DID chainId " + parsed + " != expected " + expectedChainId);
        }
        if (addrStr.contains(":")) {
            throw new InvalidPayloadError(
                "source DID address segment has invalid chars: " + addrStr);
        }
        if (!addrStr.startsWith("0x") || addrStr.length() != 42) {
            throw new InvalidPayloadError(
                "source DID does not contain a 0x-prefixed 20-byte address: " + didPkh);
        }
        return addrStr;
    }
}
