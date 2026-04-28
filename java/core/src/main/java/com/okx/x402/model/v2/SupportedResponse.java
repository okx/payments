// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.model.v2;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/** Response from facilitator /supported endpoint (V2). */
public class SupportedResponse {
    /** Supported payment kinds. */
    public List<SupportedKind> kinds = new ArrayList<>();
    /** Supported extensions. */
    public List<String> extensions = new ArrayList<>();
    /** Available signers by type. */
    public Map<String, List<String>> signers = new HashMap<>();
}
