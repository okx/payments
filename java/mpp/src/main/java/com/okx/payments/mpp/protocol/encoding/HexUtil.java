package com.okx.payments.mpp.protocol.encoding;

import java.util.regex.Pattern;

public final class HexUtil {
    public static final Pattern ADDRESS = Pattern.compile("^0x[0-9a-fA-F]{40}$");
    public static final Pattern BYTES32 = Pattern.compile("^0x[0-9a-fA-F]{64}$");
    public static final Pattern PACKED_SIGNATURE = Pattern.compile("^0x[0-9a-fA-F]{130}$");
    public static final Pattern PACKED_SIGNATURE_OR_EMPTY = Pattern.compile("^(|0x[0-9a-fA-F]{130})$");
    public static final Pattern UINT256_DECIMAL = Pattern.compile("^\\d{1,78}$");

    private HexUtil() {}

    public static byte[] decode(String hex) {
        if (hex == null) {
            throw new IllegalArgumentException("hex must not be null");
        }
        String s = hex.startsWith("0x") || hex.startsWith("0X") ? hex.substring(2) : hex;
        if ((s.length() & 1) != 0) {
            throw new IllegalArgumentException("hex length must be even");
        }
        byte[] out = new byte[s.length() / 2];
        for (int i = 0; i < out.length; i++) {
            int hi = Character.digit(s.charAt(i * 2), 16);
            int lo = Character.digit(s.charAt(i * 2 + 1), 16);
            if (hi < 0 || lo < 0) {
                throw new IllegalArgumentException("Invalid hex digit at position " + (i * 2));
            }
            out[i] = (byte) ((hi << 4) | lo);
        }
        return out;
    }

    public static String encode(byte[] bytes) {
        if (bytes == null) {
            throw new IllegalArgumentException("bytes must not be null");
        }
        StringBuilder sb = new StringBuilder(2 + bytes.length * 2);
        sb.append("0x");
        for (byte b : bytes) {
            sb.append(Character.forDigit((b >>> 4) & 0xf, 16));
            sb.append(Character.forDigit(b & 0xf, 16));
        }
        return sb.toString();
    }

    public static String requireAddress(String s, String field) {
        if (s == null || !ADDRESS.matcher(s).matches()) {
            throw new IllegalArgumentException(field + " must match " + ADDRESS.pattern() + " but was: " + s);
        }
        return s;
    }

    public static String requireBytes32(String s, String field) {
        if (s == null || !BYTES32.matcher(s).matches()) {
            throw new IllegalArgumentException(field + " must match " + BYTES32.pattern() + " but was: " + s);
        }
        return s;
    }

    public static String requirePackedSignature(String s, String field) {
        if (s == null || !PACKED_SIGNATURE.matcher(s).matches()) {
            throw new IllegalArgumentException(field + " must match " + PACKED_SIGNATURE.pattern() + " but was: " + s);
        }
        return s;
    }

    public static String requirePackedSignatureOrEmpty(String s, String field) {
        if (s == null || !PACKED_SIGNATURE_OR_EMPTY.matcher(s).matches()) {
            throw new IllegalArgumentException(field + " must be empty or match " + PACKED_SIGNATURE.pattern());
        }
        return s;
    }

    public static String requireUint256Decimal(String s, String field) {
        if (s == null || !UINT256_DECIMAL.matcher(s).matches()) {
            throw new IllegalArgumentException(field + " must match " + UINT256_DECIMAL.pattern() + " but was: " + s);
        }
        return s;
    }
}
