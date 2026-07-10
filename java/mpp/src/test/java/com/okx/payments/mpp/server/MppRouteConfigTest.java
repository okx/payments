package com.okx.payments.mpp.server;

import com.okx.payments.mpp.protocol.session.SessionMethodDetails;
import com.okx.payments.mpp.protocol.session.SessionRequest;
import org.junit.jupiter.api.Test;

import java.math.BigInteger;
import java.util.List;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

/**
 * Validates the multi-option contract on {@link MppRouteConfig.Entry}:
 *  - ≥1 option required
 *  - all options share the same chainId
 *  - distinct currency across options
 *  - buildSessionRequests emits one SessionRequest per option
 */
class MppRouteConfigTest {

    private static final String PAYEE = "0x0000000000000000000000000000000000000001";
    private static final String USDC  = "0x000000000000000000000000000000000000ABCD";
    private static final String USDT  = "0x000000000000000000000000000000000000EF01";
    private static final String OKB   = "0x000000000000000000000000000000000000BEEF";
    private static final String ESCROW_XLAYER  = "0x000000000000000000000000000000000000ESC0";
    private static final String ESCROW_MAINNET = "0x000000000000000000000000000000000000ESC1";

    private static SessionMethodDetails xlayer() {
        return new SessionMethodDetails(196L, ESCROW_XLAYER, null, "0", null);
    }

    private static SessionMethodDetails mainnet() {
        return new SessionMethodDetails(1L, ESCROW_MAINNET, null, "0", null);
    }

    @Test
    void single_option_resource_routes() {
        MppRouteConfig routes = MppRouteConfig.builder()
            .resource("/api/data", "demo", List.of(
                MppRouteConfig.Option.of(BigInteger.ONE, USDC, PAYEE, xlayer())));
        MppRouteConfig.Entry e = routes.match("/api/data");
        assertThat(e).isNotNull();
        assertThat(e.options()).hasSize(1);
        assertThat(e.options().get(0).currency()).isEqualToIgnoringCase(USDC);
        assertThat(e.options().get(0).price()).isEqualTo(BigInteger.ONE);
    }

    @Test
    void multi_token_same_chain_is_accepted() {
        MppRouteConfig routes = MppRouteConfig.builder()
            .resource("/api/data", "demo", "spot price", List.of(
                MppRouteConfig.Option.of(BigInteger.ONE,             USDC, PAYEE, xlayer()),
                MppRouteConfig.Option.of(BigInteger.valueOf(2),      USDT, PAYEE, xlayer()),
                MppRouteConfig.Option.of(BigInteger.valueOf(50_000), OKB,  PAYEE, xlayer())));
        MppRouteConfig.Entry e = routes.match("/api/data");
        assertThat(e.options()).hasSize(3);

        // findOptionByToken picks the right option by ERC-20 contract.
        assertThat(e.findOptionByToken(USDT).price()).isEqualTo(BigInteger.valueOf(2));
        assertThat(e.findOptionByToken(OKB).price()).isEqualTo(BigInteger.valueOf(50_000));
        assertThat(e.findOptionByToken("0xdead")).isNull();

        // buildSessionRequests emits one per option in declaration order.
        List<SessionRequest> srs = e.buildSessionRequests();
        assertThat(srs).hasSize(3);
        assertThat(srs.get(0).currency()).isEqualToIgnoringCase(USDC);
        assertThat(srs.get(1).currency()).isEqualToIgnoringCase(USDT);
        assertThat(srs.get(2).currency()).isEqualToIgnoringCase(OKB);
    }

    @Test
    void mixed_chain_options_are_rejected() {
        assertThatThrownBy(() -> MppRouteConfig.builder()
            .resource("/api/data", "demo", List.of(
                MppRouteConfig.Option.of(BigInteger.ONE, USDC, PAYEE, xlayer()),
                MppRouteConfig.Option.of(BigInteger.ONE, USDT, PAYEE, mainnet()))))
            .isInstanceOf(IllegalArgumentException.class)
            .hasMessageContaining("same chainId");
    }

    @Test
    void duplicate_currency_options_are_rejected() {
        assertThatThrownBy(() -> MppRouteConfig.builder()
            .resource("/api/data", "demo", List.of(
                MppRouteConfig.Option.of(BigInteger.ONE, USDC, PAYEE, xlayer()),
                MppRouteConfig.Option.of(BigInteger.TWO, USDC, PAYEE, xlayer()))))
            .isInstanceOf(IllegalArgumentException.class)
            .hasMessageContaining("duplicate currency");
    }

    @Test
    void empty_options_rejected() {
        assertThatThrownBy(() -> MppRouteConfig.builder()
            .resource("/api/data", "demo", List.of()))
            .isInstanceOf(IllegalArgumentException.class)
            .hasMessageContaining("≥1 option required");
    }

    @Test
    void negative_price_rejected() {
        assertThatThrownBy(() -> MppRouteConfig.Option.of(
            BigInteger.valueOf(-1), USDC, PAYEE, xlayer()))
            .isInstanceOf(IllegalArgumentException.class)
            .hasMessageContaining("price must be >= 0");
    }

    @Test
    void option_requires_escrowContract_and_chainId() {
        assertThatThrownBy(() -> MppRouteConfig.Option.of(
            BigInteger.ONE, USDC, PAYEE,
            new SessionMethodDetails(196L, null, null, "0", null)))
            .isInstanceOf(IllegalArgumentException.class)
            .hasMessageContaining("escrowContract required");

        assertThatThrownBy(() -> MppRouteConfig.Option.of(
            BigInteger.ONE, USDC, PAYEE,
            new SessionMethodDetails(null, ESCROW_XLAYER, null, "0", null)))
            .isInstanceOf(IllegalArgumentException.class)
            .hasMessageContaining("chainId required");
    }
}
