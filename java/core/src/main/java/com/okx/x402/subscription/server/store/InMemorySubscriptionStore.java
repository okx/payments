// SPDX-License-Identifier: Apache-2.0
package com.okx.x402.subscription.server.store;

import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.stream.Collectors;

/**
 * In-memory store with clone-on-get / clone-on-put: callers always receive and hand over
 * private copies, so a request thread mutating its row can never tear a concurrent reader of
 * the same subscription.
 */
public class InMemorySubscriptionStore implements SubscriptionStore {

    private final Map<String, StoredSubscription> store = new ConcurrentHashMap<>();

    @Override
    public StoredSubscription get(String subId) {
        StoredSubscription s = store.get(subId);
        return s == null ? null : s.copy();
    }

    @Override
    public void put(StoredSubscription sub) {
        store.put(sub.subId, sub.copy());
    }

    @Override
    public void delete(String subId) {
        store.remove(subId);
    }

    @Override
    public List<StoredSubscription> findByPayer(String payer) {
        return store.values().stream()
                .filter(s -> payer.equalsIgnoreCase(s.payer))
                .map(StoredSubscription::copy)
                .collect(Collectors.toList());
    }

    @Override
    public List<StoredSubscription> list() {
        return store.values().stream()
                .map(StoredSubscription::copy)
                .collect(Collectors.toList());
    }
}
