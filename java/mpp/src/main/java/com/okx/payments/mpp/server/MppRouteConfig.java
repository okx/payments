package com.okx.payments.mpp.server;

import com.okx.payments.mpp.protocol.session.SessionMethodDetails;
import com.okx.payments.mpp.protocol.session.SessionRequest;

import java.math.BigInteger;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.Set;

/**
 * Per-route MPP configuration consumed by {@code MppPaymentInterceptor} /
 * {@code MppAdapter}.
 *
 * <p>Each entry pairs a URI path with one or more {@link Option}s — same-chain
 * accept-variants that differ by ERC-20 token and price. The SDK emits one 402
 * with N {@code WWW-Authenticate: Payment} headers (one per option). The
 * buyer echoes the chosen option's {@link com.okx.payments.mpp.protocol.Challenge}
 * back inside the credential, and the seller derives the canonical
 * {@link SessionRequest} by reading {@code credential.challenge.request} —
 * after HMAC-verifying the echoed challenge.
 *
 * <p>Entry validation:
 * <ul>
 *   <li>≥1 option per entry</li>
 *   <li>all options share the same {@code chainId} (same chain, different tokens)</li>
 *   <li>distinct {@code currency} across options (no duplicates)</li>
 * </ul>
 *
 * <p>Resource-path deduct picks the option by the channel's pinned token at
 * postHandle time — the channel was opened against one specific ERC-20 and
 * every subsequent billed call charges that token at its option's price.
 */
public final class MppRouteConfig {

    public enum Kind { SESSION_MANAGE, RESOURCE }

    private final Map<String, Entry> entries = new LinkedHashMap<>();

    public static MppRouteConfig builder() {
        return new MppRouteConfig();
    }

    /**
     * Add a session-management endpoint (handles open / topUp / voucher / close).
     *
     * @param path exact URI path, e.g. {@code "/session/manage"}
     * @param realm 402 challenge realm
     * @param description optional human-readable description embedded in each option's SessionRequest
     * @param options ≥1 same-chain payment options (token, price, recipient, methodDetails)
     */
    public MppRouteConfig sessionManage(String path, String realm, String description,
                                        List<Option> options) {
        entries.put(normalize(path),
            new Entry(Kind.SESSION_MANAGE, realm, description, options));
        return this;
    }

    public MppRouteConfig sessionManage(String path, String realm, List<Option> options) {
        return sessionManage(path, realm, null, options);
    }

    /**
     * Add a billable resource endpoint. Interceptor will deduct the
     * channel-token-matched option's price per request.
     *
     * @param path exact URI path
     * @param realm 402 challenge realm (same realm as the session-manage endpoint)
     * @param description optional human-readable description
     * @param options ≥1 same-chain payment options
     */
    public MppRouteConfig resource(String path, String realm, String description,
                                   List<Option> options) {
        entries.put(normalize(path),
            new Entry(Kind.RESOURCE, realm, description, options));
        return this;
    }

    public MppRouteConfig resource(String path, String realm, List<Option> options) {
        return resource(path, realm, null, options);
    }

    /** Lookup by request URI; returns {@code null} when path is not configured. */
    public Entry match(String path) {
        return entries.get(normalize(path));
    }

    public Map<String, Entry> entries() {
        return new LinkedHashMap<>(entries);
    }

    private static String normalize(String path) {
        if (path == null || path.isBlank()) {
            throw new IllegalArgumentException("path required");
        }
        return path.endsWith("/") && path.length() > 1
            ? path.substring(0, path.length() - 1) : path;
    }

    /** A single payment accept-variant within an {@link Entry}. */
    public record Option(
        BigInteger price,
        String currency,
        String recipient,
        SessionMethodDetails details
    ) {
        public Option {
            Objects.requireNonNull(price, "price");
            Objects.requireNonNull(currency, "currency");
            Objects.requireNonNull(recipient, "recipient");
            Objects.requireNonNull(details, "details");
            if (price.signum() < 0) {
                throw new IllegalArgumentException("price must be >= 0");
            }
            if (details.chainId() == null) {
                throw new IllegalArgumentException("details.chainId required");
            }
            if (details.escrowContract() == null) {
                throw new IllegalArgumentException("details.escrowContract required");
            }
        }

        public static Option of(BigInteger price, String currency, String recipient,
                                SessionMethodDetails details) {
            return new Option(price, currency, recipient, details);
        }

        /** Build the {@link SessionRequest} envelope for this option. */
        public SessionRequest toSessionRequest(String description) {
            return new SessionRequest(
                price.toString(), currency, recipient,
                "request", null, description, null, details);
        }
    }

    /** Resolved route configuration carrying one or more {@link Option}s. */
    public record Entry(
        Kind kind,
        String realm,
        String description,
        List<Option> options
    ) {
        public Entry {
            Objects.requireNonNull(kind, "kind");
            Objects.requireNonNull(realm, "realm");
            Objects.requireNonNull(options, "options");
            if (options.isEmpty()) {
                throw new IllegalArgumentException("≥1 option required");
            }
            long chainId = options.get(0).details().chainId();
            Set<String> seen = new HashSet<>();
            for (Option o : options) {
                Objects.requireNonNull(o, "option");
                if (o.details().chainId() != chainId) {
                    throw new IllegalArgumentException(
                        "all options must share the same chainId (got "
                            + chainId + " and " + o.details().chainId() + ")");
                }
                if (!seen.add(o.currency().toLowerCase(Locale.ROOT))) {
                    throw new IllegalArgumentException(
                        "duplicate currency in options: " + o.currency());
                }
            }
            options = List.copyOf(options);
        }

        /** Build one {@link SessionRequest} per option (drives N 402 challenges). */
        public List<SessionRequest> buildSessionRequests() {
            List<SessionRequest> out = new ArrayList<>(options.size());
            for (Option o : options) {
                out.add(o.toSessionRequest(description));
            }
            return out;
        }

        /** Locate the option whose currency matches {@code token} (case-insensitive). */
        public Option findOptionByToken(String token) {
            if (token == null) return null;
            for (Option o : options) {
                if (o.currency().equalsIgnoreCase(token)) return o;
            }
            return null;
        }
    }
}
