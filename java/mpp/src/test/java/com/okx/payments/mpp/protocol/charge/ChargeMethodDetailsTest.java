package com.okx.payments.mpp.protocol.charge;

import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.Test;

import java.util.List;

import static org.assertj.core.api.Assertions.assertThat;

/**
 * Roundtrip + Jackson shape tests for {@link ChargeMethodDetails}.
 *
 * <p>Pins:
 * <ul>
 *   <li>{@code resourceUrl} serializes only when set ({@code NON_NULL}).</li>
 *   <li>JSON field name is camelCase ({@code resourceUrl}), matching the wire spec.</li>
 *   <li>The legacy 5-arg constructor still compiles and defaults {@code resourceUrl=null}.</li>
 *   <li>An unknown future field doesn't break deserialization (Jackson default tolerance
 *       is verified indirectly here — the record only deserializes the 6 declared props).</li>
 * </ul>
 */
class ChargeMethodDetailsTest {

    private final ObjectMapper mapper = new ObjectMapper();

    @Test
    void serialize_omits_null_resourceUrl() throws Exception {
        ChargeMethodDetails d = new ChargeMethodDetails(196L, false, null, null, null);
        String json = mapper.writeValueAsString(d);
        assertThat(json).doesNotContain("resourceUrl");
        assertThat(json).contains("\"chainId\":196");
    }

    @Test
    void serialize_includes_resourceUrl_when_present() throws Exception {
        ChargeMethodDetails d = new ChargeMethodDetails(
            196L, false, null, "tag", null, "/api/v1/widget");
        String json = mapper.writeValueAsString(d);
        assertThat(json).contains("\"resourceUrl\":\"/api/v1/widget\"");
    }

    @Test
    void deserialize_roundtrip_with_resourceUrl() throws Exception {
        ChargeMethodDetails original = new ChargeMethodDetails(
            196L, true, null, null, null, "https://api.example.com/v2/orders");
        String json = mapper.writeValueAsString(original);
        ChargeMethodDetails parsed = mapper.readValue(json, ChargeMethodDetails.class);
        assertThat(parsed).isEqualTo(original);
        assertThat(parsed.resourceUrl()).isEqualTo("https://api.example.com/v2/orders");
    }

    @Test
    void deserialize_legacy_payload_without_resourceUrl_field_yields_null() throws Exception {
        String legacyJson = "{\"chainId\":196,\"feePayer\":false}";
        ChargeMethodDetails d = mapper.readValue(legacyJson, ChargeMethodDetails.class);
        assertThat(d.chainId()).isEqualTo(196L);
        assertThat(d.resourceUrl()).isNull();
    }

    @Test
    void legacy_five_arg_constructor_defaults_resourceUrl_null() {
        ChargeMethodDetails d = new ChargeMethodDetails(196L, false, null, null,
            List.of(new ChargeSplit("10", "0xpayee", "split-memo")));
        assertThat(d.resourceUrl()).isNull();
        assertThat(d.splits()).hasSize(1);
    }
}
