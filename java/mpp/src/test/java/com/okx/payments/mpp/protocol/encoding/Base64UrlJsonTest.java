package com.okx.payments.mpp.protocol.encoding;

import com.fasterxml.jackson.databind.JsonNode;
import org.junit.jupiter.api.Test;

import java.util.LinkedHashMap;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;

class Base64UrlJsonTest {

    private final Base64UrlJson codec = new Base64UrlJson();

    @Test
    void encode_then_decode_roundtrip() {
        Map<String, Object> in = new LinkedHashMap<>();
        in.put("amount", "10000");
        in.put("currency", "0xab");
        in.put("methodDetails", Map.of("chainId", 196));

        String b64 = codec.encode(in);
        JsonNode tree = codec.decodeTree(b64);
        assertThat(tree.get("amount").asText()).isEqualTo("10000");
        assertThat(tree.get("currency").asText()).isEqualTo("0xab");
        assertThat(tree.get("methodDetails").get("chainId").asInt()).isEqualTo(196);
    }

    @Test
    void encode_strips_padding() {
        // RFC 4648 §5 base64url WITHOUT padding ('=')
        String b64 = codec.encode(Map.of("a", 1));
        assertThat(b64).doesNotContain("=");
    }

    @Test
    void encode_uses_url_safe_alphabet() {
        // No '+' or '/' in url-safe base64
        String b64 = codec.encode(Map.of("k", "test data with potential / and + chars"));
        assertThat(b64).doesNotContain("+");
        assertThat(b64).doesNotContain("/");
    }

    @Test
    void encode_is_jcs_stable_regardless_of_insertion_order() {
        Map<String, Object> a = new LinkedHashMap<>();
        a.put("x", 1);
        a.put("y", 2);

        Map<String, Object> b = new LinkedHashMap<>();
        b.put("y", 2);
        b.put("x", 1);

        assertThat(codec.encode(a)).isEqualTo(codec.encode(b));
    }
}
