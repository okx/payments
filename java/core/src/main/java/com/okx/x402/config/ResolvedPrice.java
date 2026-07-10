// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.config;

import java.util.Map;

/**
 * Result of price resolution: atomic amount + asset address + EIP-712 extra fields.
 *
 * @param amount atomic units string
 * @param asset token contract address
 * @param extra EIP-712 extra fields (name, version, transferMethod)
 */
public record ResolvedPrice(String amount, String asset, Map<String, Object> extra) {
}
