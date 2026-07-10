// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.server.store;

import java.util.List;

public interface SubscriptionStore {
    StoredSubscription get(String subId);
    void put(StoredSubscription sub);
    void delete(String subId);
    default List<StoredSubscription> findByPayer(String payer) {
        throw new UnsupportedOperationException("findByPayer not implemented");
    }

    /**
     * All cached records — backs {@code SubscriptionService.dueSubscriptions} (a scan filtered
     * by the caller). A durable backend with a large subscription set may add its own indexed
     * due-query instead.
     */
    default List<StoredSubscription> list() {
        throw new UnsupportedOperationException("list not implemented");
    }
}
