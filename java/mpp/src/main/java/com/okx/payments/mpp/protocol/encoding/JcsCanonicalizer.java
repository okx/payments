package com.okx.payments.mpp.protocol.encoding;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;

/**
 * RFC 8785 JSON Canonicalization Scheme.
 *
 * <p>Implements only the subset needed for MPP request payloads:
 * objects with sorted keys, arrays, strings, booleans, null, and integer/decimal
 * numbers (we never emit floats — MPP amounts/timestamps are decimal strings).
 *
 * <p>Reference: <a href="https://datatracker.ietf.org/doc/html/rfc8785">RFC 8785</a>.
 */
public final class JcsCanonicalizer {

    private JcsCanonicalizer() {}

    /** Canonicalize a Jackson JsonNode tree to RFC 8785 bytes (UTF-8). */
    public static byte[] canonicalize(JsonNode node) {
        try {
            ByteArrayOutputStream out = new ByteArrayOutputStream(64);
            write(out, node);
            return out.toByteArray();
        } catch (IOException e) {
            throw new IllegalStateException("JCS write failed", e);
        }
    }

    private static void write(ByteArrayOutputStream out, JsonNode node) throws IOException {
        if (node == null || node.isNull()) {
            out.write("null".getBytes(StandardCharsets.UTF_8));
            return;
        }
        if (node.isBoolean()) {
            out.write((node.asBoolean() ? "true" : "false").getBytes(StandardCharsets.UTF_8));
            return;
        }
        if (node.isNumber()) {
            // RFC 8785 §3.2.2.3 — integers verbatim; we don't emit floats.
            // Numeric.toString() already strips a leading "+" / trailing zeros for BigInteger.
            out.write(node.asText().getBytes(StandardCharsets.UTF_8));
            return;
        }
        if (node.isTextual()) {
            writeString(out, node.asText());
            return;
        }
        if (node.isArray()) {
            ArrayNode arr = (ArrayNode) node;
            out.write('[');
            for (int i = 0; i < arr.size(); i++) {
                if (i > 0) {
                    out.write(',');
                }
                write(out, arr.get(i));
            }
            out.write(']');
            return;
        }
        if (node.isObject()) {
            ObjectNode obj = (ObjectNode) node;
            List<String> keys = new ArrayList<>();
            obj.fieldNames().forEachRemaining(keys::add);
            // RFC 8785 §3.2.3 — sort by UTF-16 code units (Java String natural order is UTF-16 code units).
            Collections.sort(keys);
            out.write('{');
            for (int i = 0; i < keys.size(); i++) {
                if (i > 0) {
                    out.write(',');
                }
                writeString(out, keys.get(i));
                out.write(':');
                write(out, obj.get(keys.get(i)));
            }
            out.write('}');
            return;
        }
        throw new IllegalArgumentException("Unsupported JsonNode type: " + node.getNodeType());
    }

    /** RFC 8785 §3.2.2.2 — strings use ECMA-262 escape rules with shortest forms. */
    private static void writeString(ByteArrayOutputStream out, String s) throws IOException {
        out.write('"');
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"' -> out.write("\\\"".getBytes(StandardCharsets.UTF_8));
                case '\\' -> out.write("\\\\".getBytes(StandardCharsets.UTF_8));
                case '\b' -> out.write("\\b".getBytes(StandardCharsets.UTF_8));
                case '\f' -> out.write("\\f".getBytes(StandardCharsets.UTF_8));
                case '\n' -> out.write("\\n".getBytes(StandardCharsets.UTF_8));
                case '\r' -> out.write("\\r".getBytes(StandardCharsets.UTF_8));
                case '\t' -> out.write("\\t".getBytes(StandardCharsets.UTF_8));
                default -> {
                    if (c < 0x20) {
                        out.write(String.format("\\u%04x", (int) c).getBytes(StandardCharsets.UTF_8));
                    } else {
                        // Direct UTF-8 emit (incl surrogates handled as-is by getBytes)
                        out.write(String.valueOf(c).getBytes(StandardCharsets.UTF_8));
                    }
                }
            }
        }
        out.write('"');
    }

    /** Convenience: canonicalize an arbitrary Map/List/value tree (Jackson normalizes). */
    public static byte[] canonicalize(Object value, com.fasterxml.jackson.databind.ObjectMapper mapper) {
        return canonicalize(mapper.valueToTree(value));
    }

    /** Sentinel symbol so Map.copyOf etc. don't collapse — kept for tests. */
    public static final Map<String, Object> EMPTY = Map.of();
}
