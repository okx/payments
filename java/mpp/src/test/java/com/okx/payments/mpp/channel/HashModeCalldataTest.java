package com.okx.payments.mpp.channel;

import org.junit.jupiter.api.Test;

import java.math.BigInteger;

import static org.assertj.core.api.Assertions.assertThat;

/**
 * Unit tests + a runnable {@code main} that prints copy-pasteable calldata for the
 * MPP session hash-mode flow. Mirrors the Go reference (mpp-go/demo/hash-calldata).
 *
 * <p>The deterministic vectors here use a fixed salt + zero authorizedSigner so the
 * calldata bytes are stable across runs and can be regression-checked against the Go
 * implementation if needed.
 *
 * <p>Run the demo (prints approve + open + topup calldata for a 5-base-unit deposit):
 * <pre>{@code
 *   mvn -B -pl mpp test-compile exec:java \
 *       -Dexec.mainClass=com.okx.payments.mpp.channel.HashModeCalldataTest \
 *       -Dexec.classpathScope=test
 * }</pre>
 */
public final class HashModeCalldataTest {

    // X Layer mainnet fixtures.
    private static final long   CHAIN_ID  = 196L;
    private static final String ESCROW    = "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b";
    private static final String USDT      = "0x779ded0c9e1022225f8e0630b35a9b54be713736";
    private static final String PAYEE     = "0x238193be9e80e68eace3588b45d8cf4a7eae0fa3";
    // Synthetic test address — clearly a fixture, not a real wallet. The keccak
    // input slot value is irrelevant to the assertions (they check selector +
    // padding + ordering only), so this remains byte-stable across runs.
    private static final String PAYER     = "0xabcdef0123456789abcdef0123456789abcdef01";
    // Deterministic fixture salt — chosen for byte-stable test output.
    private static final String FIXED_SALT =
        "0x6e8a000000000000000000000000000000000000000000000000000000000001";

    // ── Unit tests ──────────────────────────────────────────────────────────

    @Test
    void approve_calldata_encodes_to_0x095ea7b3_with_padded_args() {
        String data = HashModeCalldata.encodeApprove(ESCROW, BigInteger.valueOf(5));
        // 4-byte selector for approve(address,uint256) is 0x095ea7b3
        assertThat(data).startsWith("0x095ea7b3");
        // First arg padded to 32 bytes — escrow address right-aligned in slot
        assertThat(data.toLowerCase()).contains("5e550002e64faf79b41d89fe8439eeb1be66ce3b");
        // Second arg padded to 32 bytes — value 5 right-aligned
        assertThat(data).endsWith(
            "0000000000000000000000000000000000000000000000000000000000000005");
    }

    @Test
    void open_calldata_starts_with_4byte_selector_and_carries_salt() {
        String data = HashModeCalldata.encodeOpen(PAYEE, USDT, BigInteger.valueOf(5),
            FIXED_SALT, ChannelIdCalculator.ZERO_ADDRESS);
        // 0x + 4-byte selector (8 hex) + 7 head slots (5 statics + 2 dyn offsets) + 2 dyn tails (length-prefixed empty arrays)
        // = 2 + 8 + 7*64 + 2*64 = 586
        assertThat(data).startsWith("0x");
        assertThat(data.length()).isEqualTo(2 + 8 + 9 * 64);
        assertThat(data.toLowerCase()).contains(FIXED_SALT.substring(2));
        // Empty splits ⇒ two zero-length dynamic arrays at the tail
        assertThat(data).endsWith(
            "0000000000000000000000000000000000000000000000000000000000000000"
                + "0000000000000000000000000000000000000000000000000000000000000000");
    }

    @Test
    void topup_calldata_encodes_channelId_and_uint128() {
        String channelId = ChannelIdCalculator.compute(
            PAYER, PAYEE, USDT, FIXED_SALT, ChannelIdCalculator.ZERO_ADDRESS, ESCROW, CHAIN_ID);
        String data = HashModeCalldata.encodeTopUp(channelId, BigInteger.valueOf(3));
        assertThat(data).startsWith("0x");
        // amount 3 padded to 32 bytes at the end
        assertThat(data).endsWith(
            "0000000000000000000000000000000000000000000000000000000000000003");
    }

    @Test
    void channelId_is_deterministic_given_payer_payee_token_salt_escrow_chain() {
        String a = ChannelIdCalculator.compute(
            PAYER, PAYEE, USDT, FIXED_SALT, ChannelIdCalculator.ZERO_ADDRESS, ESCROW, CHAIN_ID);
        String b = ChannelIdCalculator.compute(
            PAYER, PAYEE, USDT, FIXED_SALT, ChannelIdCalculator.ZERO_ADDRESS, ESCROW, CHAIN_ID);
        assertThat(a).isEqualTo(b);
        assertThat(a).hasSize(2 + 64); // 0x + 32-byte hex
    }

    // ── Runnable demo — prints copy-pasteable wallet CLI commands ───────────

    public static void main(String[] args) {
        BigInteger deposit = args.length > 0 ? new BigInteger(args[0]) : BigInteger.valueOf(5);
        String salt = HashModeCalldata.generateSalt();
        String channelId = ChannelIdCalculator.compute(
            PAYER, PAYEE, USDT, salt, ChannelIdCalculator.ZERO_ADDRESS, ESCROW, CHAIN_ID);

        System.out.println("════════════════════════════════════════════════════════════════════");
        System.out.println("MPP session hash-mode — buyer calldata bundle");
        System.out.println("════════════════════════════════════════════════════════════════════");
        System.out.println("payer (msg.sender):  " + PAYER);
        System.out.println("payee (seller):      " + PAYEE);
        System.out.println("token (USD₮0):       " + USDT);
        System.out.println("escrow:              " + ESCROW);
        System.out.println("chainId:             " + CHAIN_ID);
        System.out.println("deposit (atomic):    " + deposit);
        System.out.println("authorizedSigner:    " + ChannelIdCalculator.ZERO_ADDRESS + "  (payer signs)");
        System.out.println("salt:                " + salt);
        System.out.println("channelId (precomp): " + channelId);
        System.out.println();
        System.out.println("─── 1. APPROVE — buyer broadcasts ─────────────────────────────────");
        System.out.println("  onchainos wallet contract-call --chain xlayer \\");
        System.out.println("    --to " + USDT + " \\");
        System.out.println("    --input-data " + HashModeCalldata.encodeApprove(ESCROW, deposit));
        System.out.println();
        System.out.println("─── 2. OPEN — buyer broadcasts ────────────────────────────────────");
        System.out.println("  onchainos wallet contract-call --chain xlayer \\");
        System.out.println("    --to " + ESCROW + " \\");
        System.out.println("    --input-data " + HashModeCalldata.encodeOpen(
            PAYEE, USDT, deposit, salt, ChannelIdCalculator.ZERO_ADDRESS));
        System.out.println();
        System.out.println("─── 3. TOPUP (optional, later) — buyer broadcasts ─────────────────");
        System.out.println("  onchainos wallet contract-call --chain xlayer \\");
        System.out.println("    --to " + ESCROW + " \\");
        System.out.println("    --input-data " + HashModeCalldata.encodeTopUp(
            channelId, BigInteger.valueOf(3)));
        System.out.println();
        System.out.println("Pass these to mpp-session-open in step 4:");
        System.out.println("  --tx-hash <openTxHash>");
        System.out.println("  --salt    " + salt);
        System.out.println("  --channel-id " + channelId + "   # if your CLI supports it");
    }
}
