package com.okx.payments.mpp.sa;

import java.time.Duration;
import java.util.Objects;

public final class SaApiConfig {

    public static final String DEFAULT_BASE_URL = "https://web3.okx.com";
    public static final String API_PREFIX = "/api/v6/pay/mpp";

    private final String baseUrl;
    private final String apiKey;
    private final String secretKey;
    private final String passphrase;
    private final Duration connectTimeout;
    private final Duration readTimeout;

    private SaApiConfig(Builder b) {
        this.baseUrl = stripTrailingSlash(Objects.requireNonNull(b.baseUrl, "baseUrl"));
        this.apiKey = Objects.requireNonNull(b.apiKey, "apiKey");
        this.secretKey = Objects.requireNonNull(b.secretKey, "secretKey");
        this.passphrase = Objects.requireNonNull(b.passphrase, "passphrase");
        this.connectTimeout = b.connectTimeout != null ? b.connectTimeout : Duration.ofSeconds(10);
        this.readTimeout = b.readTimeout != null ? b.readTimeout : Duration.ofSeconds(30);
    }

    public String baseUrl() { return baseUrl; }
    public String apiKey() { return apiKey; }
    public String secretKey() { return secretKey; }
    public String passphrase() { return passphrase; }
    public Duration connectTimeout() { return connectTimeout; }
    public Duration readTimeout() { return readTimeout; }

    public static Builder builder() {
        return new Builder();
    }

    private static String stripTrailingSlash(String s) {
        return s.endsWith("/") ? s.substring(0, s.length() - 1) : s;
    }

    public static final class Builder {
        private String baseUrl = DEFAULT_BASE_URL;
        private String apiKey;
        private String secretKey;
        private String passphrase;
        private Duration connectTimeout;
        private Duration readTimeout;

        public Builder baseUrl(String v) { this.baseUrl = v; return this; }
        public Builder apiKey(String v) { this.apiKey = v; return this; }
        public Builder secretKey(String v) { this.secretKey = v; return this; }
        public Builder passphrase(String v) { this.passphrase = v; return this; }
        public Builder connectTimeout(Duration v) { this.connectTimeout = v; return this; }
        public Builder readTimeout(Duration v) { this.readTimeout = v; return this; }
        public SaApiConfig build() { return new SaApiConfig(this); }
    }
}
