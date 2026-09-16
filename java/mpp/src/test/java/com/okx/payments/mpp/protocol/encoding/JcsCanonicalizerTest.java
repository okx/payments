package com.okx.payments.mpp.protocol.encoding;

import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.Test;

import java.nio.charset.StandardCharsets;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import static org.assertj.core.api.Assertions.assertThat;

class JcsCanonicalizerTest {

    private final ObjectMapper mapper = new ObjectMapper();

    @Test
    void sorts_object_keys_lexicographically() {
        // Insertion order: b, a, c — JCS must sort to a, b, c.
        Map<String, Object> in = new LinkedHashMap<>();
        in.put("b", 2);
        in.put("a", 1);
        in.put("c", 3);
        String out = utf8(JcsCanonicalizer.canonicalize(in, mapper));
        assertThat(out).isEqualTo("{\"a\":1,\"b\":2,\"c\":3}");
    }

    @Test
    void escapes_control_characters_and_quotes() {
        String out = utf8(JcsCanonicalizer.canonicalize(Map.of("k", "a\nb\"c"), mapper));
        assertThat(out).isEqualTo("{\"k\":\"a\\nb\\\"c\"}");
    }

    @Test
    void preserves_array_order() {
        String out = utf8(JcsCanonicalizer.canonicalize(List.of(3, 1, 2), mapper));
        assertThat(out).isEqualTo("[3,1,2]");
    }

    @Test
    void nested_objects_sort_at_each_level() {
        Map<String, Object> inner = new LinkedHashMap<>();
        inner.put("z", 1);
        inner.put("a", 2);

        Map<String, Object> outer = new LinkedHashMap<>();
        outer.put("y", inner);
        outer.put("x", List.of(inner));

        String out = utf8(JcsCanonicalizer.canonicalize(outer, mapper));
        assertThat(out).isEqualTo("{\"x\":[{\"a\":2,\"z\":1}],\"y\":{\"a\":2,\"z\":1}}");
    }

    @Test
    void null_and_boolean() {
        Map<String, Object> in = new LinkedHashMap<>();
        in.put("n", null);
        in.put("t", true);
        in.put("f", false);
        String out = utf8(JcsCanonicalizer.canonicalize(in, mapper));
        assertThat(out).isEqualTo("{\"f\":false,\"n\":null,\"t\":true}");
    }

    @Test
    void empty_object() {
        assertThat(utf8(JcsCanonicalizer.canonicalize(Map.of(), mapper))).isEqualTo("{}");
    }

    @Test
    void empty_array() {
        assertThat(utf8(JcsCanonicalizer.canonicalize(List.of(), mapper))).isEqualTo("[]");
    }

    @Test
    void rfc_8785_appendix_b_sample_keys_sort_correctly() {
        // RFC 8785 §A.2 example — verify lexicographic UTF-16 ordering on Latin1 keys.
        Map<String, Object> in = new LinkedHashMap<>();
        in.put("€", 1);     // €
        in.put("$", 2);
        String out = utf8(JcsCanonicalizer.canonicalize(in, mapper));
        // '$' (0x24) sorts before '€' (0x20AC)
        assertThat(out).isEqualTo("{\"$\":2,\"€\":1}");
    }

    private static String utf8(byte[] bytes) {
        return new String(bytes, StandardCharsets.UTF_8);
    }
}
