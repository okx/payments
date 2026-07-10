// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model.v2;

import java.util.Map;

/** A supported payment scheme+network pair (V2). */
public class SupportedKind {
    /** Protocol version. */
    public int x402Version = 2;
    /** Payment scheme identifier. */
    public String scheme;
    /** CAIP-2 network identifier. */
    public String network;
    /** Scheme-specific extra fields. */
    public Map<String, Object> extra;
}
