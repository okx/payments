# OKX x402 Java SDK — Execution Plan

> Artifact: `com.okx:x402-java`
> Base: Coinbase `com.coinbase:x402` (V1-only, base-sepolia hardcoded)
> Target: Full Phase 1 delivery — X Layer EIP-3009 payment, 28 acceptance tests pass
> Scope: `exact` scheme with EIP-3009 only. `agentic-press` scheme (Doc 4) is explicitly out of Phase 1 scope.

---

## Requirement Traceability Matrix

Every acceptance test mapped to the implementation task that fulfills it.

| Test ID | Test Name                                           | Impl Task                                      |
|---------|-----------------------------------------------------|------------------------------------------------|
| F-1     | X Layer USDT out-of-box                             | Task 2 (asset registry)                        |
| F-2     | X Layer full payment flow                           | Task 1 + 2 + 3 + 4 + 5 + 6                     |
| F-3     | EIP-712 domain name `USD₮0` (Unicode)               | Task 2 + 5                                     |
| F-4     | X Layer Testnet payment                             | Task 2                                         |
| F-5     | Contract address & decimals correct                 | Task 2                                         |
| G-1     | Credentials config-and-go                           | Task 3                                         |
| G-2     | Auth headers auto-attached                          | Task 3                                         |
| G-3     | verify format transparent conversion                | Task 3                                         |
| G-4     | settle format transparent conversion                | Task 3                                         |
| G-5     | getSupported format transparent conversion          | Task 3                                         |
| G-6     | V2→V1 downgrade                                     | **N/A for Java** (Doc 1 explicitly marks this) |
| G-7     | V1→V2 upgrade                                       | **N/A for Java** (Doc 1 explicitly marks this) |
| G-8     | Missing credentials error                           | Task 3                                         |
| G-9     | Error code mapping (50103 → readable)               | Task 3                                         |
| I-1     | PrivateKey signing X Layer tx                       | Task 5                                         |
| I-2     | Signer address correct                              | Task 5                                         |
| I-3     | No config → immediate error                         | Task 5                                         |
| B-1     | Auto 402 handling                                   | Task 6                                         |
| B-2     | 402 response body correct                           | Task 4                                         |
| B-3     | Payment success returns biz data + PAYMENT-RESPONSE | Task 4 + 6                                     |
| C-1     | Route-level pricing                                 | Task 4                                         |
| C-2     | Middleware one-line integration                     | Task 4                                         |
| ERR-1   | Invalid OKX API Key                                 | Task 3                                         |
| ERR-2   | Invalid signature                                   | Task 3 (facilitator returns invalidReason)     |
| ERR-3   | Insufficient balance                                | Task 3 (facilitator returns errorReason)       |
| ERR-4   | Signature expired                                   | Task 5 (validBefore logic) + Task 3            |
| ERR-5   | OKX API timeout                                     | Task 3 (timeout with context error)            |
| ERR-6   | Facilitator not configured                          | Task 3 (constructor validation)                |

---

## Task Breakdown

### Task 0: Project Scaffolding & Package Rename

**Goal:** Fork from Coinbase Java SDK, rebrand to OKX, set up new package structure.

**Changes:**

```
com.coinbase.x402  →  com.okx.x402       (base package rename)

New package layout:
com.okx.x402
├── client/                    # HTTP clients (buyer side)
│   ├── OKXHttpClient.java         ← replaces X402HttpClient
│   └── FacilitatorClient.java     ← kept (interface)
├── facilitator/               # NEW: facilitator client implementations
│   ├── OKXFacilitatorClient.java  ← NEW (core deliverable)
│   ├── HttpFacilitatorClient.java ← kept (Coinbase CDP compat)
│   └── FacilitatorRouter.java     ← NEW (auto-routing)
├── crypto/                    # Signing
│   ├── CryptoSigner.java          ← kept (interface)
│   ├── CryptoSignException.java   ← kept
│   └── OKXEvm3009Signer.java      ← NEW (web3j EIP-3009)
├── model/                     # V2 data types
│   ├── v2/                        ← NEW subpackage
│   │   ├── PaymentPayload.java
│   │   ├── PaymentRequired.java
│   │   ├── PaymentRequirements.java
│   │   ├── ResourceInfo.java
│   │   ├── VerifyRequest.java
│   │   ├── VerifyResponse.java
│   │   ├── SettleRequest.java
│   │   ├── SettleResponse.java
│   │   ├── SupportedKind.java
│   │   └── SupportedResponse.java
│   ├── v1/                        ← move old types here
│   │   └── (existing V1 models)
│   ├── Authorization.java         ← kept (shared by V1 & V2)
│   ├── ExactSchemePayload.java    ← kept (shared)
│   └── SettlementResponseHeader.java ← kept (shared)
├── server/                    # Server middleware
│   └── PaymentFilter.java         ← upgraded to V2
├── config/                    # NEW: asset & chain config
│   ├── AssetRegistry.java
│   ├── AssetConfig.java
│   └── XLayerAssets.java
└── util/
    ├── Json.java                   ← kept
    └── OKXAuth.java                ← NEW (HMAC-SHA256)
```

**pom.xml changes:**

- groupId: `com.okx`
- artifactId: `x402-java`
- Add dependency: `org.web3j:core:4.12.3` (for EIP-712 signing)
- Add dependency: `org.bouncycastle:bcprov-jdk18on` (crypto primitives)

**Fills:** Foundation for all tasks. No test directly, but unblocks everything.

---

### Task 1: V2 Data Models (`model/v2/`)

**Goal:** Implement all x402 V2 protocol types per Doc 3 §1.1.

**New files:**

#### `model/v2/ResourceInfo.java`

```java
public class ResourceInfo {
    public String url;           // required
    public String description;   // optional
    public String mimeType;      // optional
}
```

#### `model/v2/PaymentRequirements.java`

```java
public class PaymentRequirements {
    public String scheme;                    // "exact"
    public String network;                   // CAIP-2: "eip155:196"
    public String amount;                    // atomic units string (was maxAmountRequired in V1)
    public String payTo;                     // 0x address
    public int maxTimeoutSeconds;
    public String asset;                     // token contract address
    public Map<String, Object> extra;        // {name, version, transferMethod}
}
```

> **[Fills B-2]** 402 body uses `amount` (not `maxAmountRequired`), `network` is CAIP-2
> **[Fills F-5]** asset = contract address, extra.name = EIP-712 name

#### `model/v2/PaymentRequired.java`

```java
public class PaymentRequired {
    public int x402Version = 2;
    public String error;
    public ResourceInfo resource;
    public List<PaymentRequirements> accepts = new ArrayList<>();
    public Map<String, Object> extensions;
}
```

> **[Fills B-2]** 402 response body structure matches V2 spec

#### `model/v2/PaymentPayload.java`

```java
public class PaymentPayload {
    public int x402Version = 2;
    public ResourceInfo resource;            // optional echo
    public PaymentRequirements accepted;     // which accept[] item chosen
    public Map<String, Object> payload;      // ExactEIP3009Payload as map
    public Map<String, Object> extensions;   // optional

    public String toHeader() { /* JSON → Base64 */ }

    public static PaymentPayload fromHeader(String h) throws IOException { /* Base64 → JSON */ }
}
```

> **[Fills B-1, B-3]** Client sends V2 PaymentPayload via PAYMENT-SIGNATURE header

#### `model/v2/VerifyRequest.java`

```java
public class VerifyRequest {
    public int x402Version = 2;
    public PaymentPayload paymentPayload;
    public PaymentRequirements paymentRequirements;
}
```

#### `model/v2/VerifyResponse.java`

```java
public class VerifyResponse {
    public boolean isValid;
    public String invalidReason;      // machine-readable code
    public String invalidMessage;     // human-readable (V2 new)
    public String payer;
    public Map<String, Object> extensions;
}
```

> **[Fills ERR-2]** invalidReason populated on bad signature

#### `model/v2/SettleRequest.java`

```java
public class SettleRequest {
    public int x402Version = 2;
    public PaymentPayload paymentPayload;
    public PaymentRequirements paymentRequirements;
    public Boolean syncSettle;        // OKX extension: true = wait for on-chain confirm
}
```

#### `model/v2/SettleResponse.java`

```java
public class SettleResponse {
    public boolean success;
    public String errorReason;
    public String errorMessage;
    public String payer;
    public String transaction;        // tx hash (was txHash in V1)
    public String network;            // CAIP-2 (was networkId in V1)
    public String amount;             // actual settled amount (upto scheme)
    public String status;             // OKX extension: "pending"/"success"/"timeout"
    public Map<String, Object> extensions;
}
```

> **[Fills G-4]** Uses `transaction` (not `txHash`), `network` (not `networkId`)

#### `model/v2/SupportedKind.java`

```java
public class SupportedKind {
    public int x402Version = 2;
    public String scheme;
    public String network;            // CAIP-2
    public Map<String, Object> extra;
}
```

#### `model/v2/SupportedResponse.java`

```java
public class SupportedResponse {
    public List<SupportedKind> kinds = new ArrayList<>();
    public List<String> extensions = new ArrayList<>();
    public Map<String, List<String>> signers = new HashMap<>();
}
```

> **[Fills G-5]** Returns CAIP-2 network format, V2 structure

**Tests:**

- `PaymentPayloadV2Test.java` — toHeader()/fromHeader() round-trip with V2 fields
- `PaymentRequiredV2Test.java` — JSON serialization matches spec examples from Doc 3

---

### Task 2: Asset Registry & X Layer Config (`config/`)

**Goal:** Built-in X Layer USDT/USDG asset config. `price: "$0.01"` auto-resolves to correct token.

**New files:**

#### `config/AssetConfig.java`

```java
public class AssetConfig {
    private final String symbol;         // "USDT"
    private final String contractAddress; // "0x779d..."
    private final int decimals;          // 6
    private final String eip712Name;     // "USD₮0" (Unicode ₮ = U+20AE)
    private final String eip712Version;  // "1"
    private final String transferMethod; // "eip3009"

    // Constructor, getters, builder

    /** Convert human amount (e.g. 0.01) to atomic units string */
    public String toAtomicUnits(BigDecimal humanAmount) {
        return humanAmount.movePointRight(decimals)
                .setScale(0, RoundingMode.DOWN)
                .toBigInteger().toString();
    }

    /** Convert USD string "$0.01" to atomic units (assumes 1:1 stablecoin) */
    public String fromUsdPrice(String usdPrice) {
        String cleaned = usdPrice.replace("$", "").trim();
        return toAtomicUnits(new BigDecimal(cleaned));
    }
}
```

> **[Fills F-5]** Contract address and decimals correct

#### `config/AssetRegistry.java`

```java
public class AssetRegistry {
    private static final Map<String, List<AssetConfig>> NETWORK_ASSETS = new ConcurrentHashMap<>();

    static {
        // X Layer Mainnet
        register("eip155:196", AssetConfig.builder()
                .symbol("USDT")
                .contractAddress("0x779ded0c9e1022225f8e0630b35a9b54be713736")
                .decimals(6)
                .eip712Name("USD\u20AE0")  // "USD₮0" — Unicode U+20AE
                .eip712Version("1")
                .transferMethod("eip3009")
                .build());

        // USDG: DO NOT register until EIP-712 domain is confirmed on-chain.
        // eip712Name and eip712Version are both TBD per Doc 2 §5.2.
        // Registering with wrong values will cause all USDG signatures to fail.
        // register("eip155:196", AssetConfig.builder()
        //         .symbol("USDG")
        //         .contractAddress("0x4ae46a509f6b1d9056937ba4500cb143933d2dc8")
        //         .decimals(6)
        //         .eip712Name("TBD")
        //         .eip712Version("TBD")
        //         .transferMethod("eip3009")
        //         .build());

        // X Layer Testnet
        register("eip155:195", AssetConfig.builder()
                .symbol("USDT")
                .contractAddress("TBD")  // Testnet address TBD
                .decimals(6)
                .eip712Name("USD\u20AE0")
                .eip712Version("1")
                .transferMethod("eip3009")
                .build());
    }

    /** Get default asset for network (first registered, typically USDT) */
    public static AssetConfig getDefault(String network) { ...}

    /** Get specific asset by network + symbol */
    public static AssetConfig get(String network, String symbol) { ...}

    /** Resolve "$0.01" for a given network → {amount, asset, extra} */
    public static ResolvedPrice resolvePrice(String price, String network) {
        AssetConfig asset = getDefault(network);
        if (asset == null) throw new IllegalArgumentException(
                "No default asset configured for network " + network);
        return new ResolvedPrice(
                asset.fromUsdPrice(price),
                asset.getContractAddress(),
                Map.of("name", asset.getEip712Name(),
                        "version", asset.getEip712Version(),
                        "transferMethod", asset.getTransferMethod())
        );
    }

    /** Allow user to register custom assets */
    public static void register(String network, AssetConfig config) { ...}
}
```

#### `config/ResolvedPrice.java`

```java
/** Result of price resolution: atomic amount + asset address + EIP-712 extra fields */
public record ResolvedPrice(String amount, String asset, Map<String, Object> extra) {}
```
```

> **[Fills F-1]** `price: "$0.01", network: "eip155:196"` auto-resolves to USDT, no "No default asset" error
> **[Fills F-3]** EIP-712 domain name is `"USD₮0"` (Unicode ₮), not `"USDT0"`
> **[Fills F-4]** X Layer Testnet `eip155:195` registered
> **[Fills F-5]** Address `0x779d...`, decimals 6

**Tests:**

- `AssetRegistryTest.java`:
    - `xlayerUsdtResolvesAutomatically()` → **[F-1]**
    - `eip712NameContainsUnicode()` → **[F-3]** assert name contains `\u20AE`
    - `testnetRegistered()` → **[F-4]**
    - `contractAddressAndDecimalsCorrect()` → **[F-5]**
    - `unknownNetworkThrows()` → assert "No default asset" message

---

### Task 3: OKXFacilitatorClient (`facilitator/`)

**Goal:** Implement FacilitatorClient with OKX API auth, V2 format, error mapping.

**New files:**

#### `util/OKXAuth.java`

```java
public class OKXAuth {
    private final String apiKey;
    private final String secretKey;
    private final String passphrase;

    public OKXAuth(String apiKey, String secretKey, String passphrase) {
        // [Fills G-8, ERR-6] Validate all fields present
        if (apiKey == null || apiKey.isEmpty())
            throw new IllegalArgumentException("OKX API key is required");
        if (secretKey == null || secretKey.isEmpty())
            throw new IllegalArgumentException("OKX secret key is required");
        if (passphrase == null || passphrase.isEmpty())
            throw new IllegalArgumentException("OKX passphrase is required");
        this.apiKey = apiKey;
        this.secretKey = secretKey;
        this.passphrase = passphrase;
    }

    /** Generate auth headers for an OKX API request */
    /** Generate auth headers for an OKX API request */
    public Map<String, String> createHeaders(String method, String path, String body) {
        // OKX requires millisecond-precision ISO 8601: yyyy-MM-dd'T'HH:mm:ss.SSS'Z'
        // Instant.toString() produces nanoseconds which will cause signature mismatch
        String timestamp = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'")
                .withZone(ZoneOffset.UTC)
                .format(Instant.now());
        String prehash = timestamp + method + path + (body != null ? body : "");
        String signature = Base64.getEncoder().encodeToString(
                hmacSha256(secretKey.getBytes(UTF_8), prehash.getBytes(UTF_8)));

        return Map.of(
                "OK-ACCESS-KEY", apiKey,
                "OK-ACCESS-SIGN", signature,
                "OK-ACCESS-TIMESTAMP", timestamp,
                "OK-ACCESS-PASSPHRASE", passphrase,
                "Content-Type", "application/json"
        );
    }

    private static byte[] hmacSha256(byte[] key, byte[] data) {
        try {
            Mac mac = Mac.getInstance("HmacSHA256");
            mac.init(new SecretKeySpec(key, "HmacSHA256"));
            return mac.doFinal(data);
        } catch (NoSuchAlgorithmException | InvalidKeyException e) {
            throw new RuntimeException("HMAC-SHA256 computation failed", e);
        }
    }
}
```

> **[Fills G-1]** Pass credentials, construct succeeds
> **[Fills G-2]** Auth headers auto-attached
> **[Fills G-8]** Missing apiKey → "OKX API key is required"
> **[Fills ERR-6]** No credentials → clear error at construction

#### `facilitator/OKXFacilitatorClient.java`

```java
public class OKXFacilitatorClient implements FacilitatorClient {
    private static final String DEFAULT_BASE_URL = "https://www.okx.com";
    private static final String VERIFY_PATH = "/api/v6/pay/x402/verify";
    private static final String SETTLE_PATH = "/api/v6/pay/x402/settle";
    private static final String SUPPORTED_PATH = "/api/v6/pay/x402/supported";

    // OKX error code → human-readable message
    private static final Map<String, String> ERROR_MAP = Map.of(
            "50103", "Invalid API key",
            "50104", "Invalid API key or IP",
            "50113", "Invalid passphrase",
            "50001", "Service temporarily unavailable",
            // ... more as documented
    );

    private final OKXAuth auth;
    private final HttpClient http;
    private final String baseUrl;

    public OKXFacilitatorClient(String apiKey, String secretKey, String passphrase) {
        this(apiKey, secretKey, passphrase, DEFAULT_BASE_URL);
    }

    public OKXFacilitatorClient(String apiKey, String secretKey,
                                String passphrase, String baseUrl) {
        this.auth = new OKXAuth(apiKey, secretKey, passphrase);
        this.baseUrl = baseUrl.replaceAll("/+$", "");
        this.http = HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(10))
                .build();
    }

    @Override
    public VerifyResponse verify(PaymentPayload payload,
                                 PaymentRequirements requirements)
            throws IOException, InterruptedException {
        // Build V2 VerifyRequest body
        VerifyRequest req = new VerifyRequest();
        req.x402Version = 2;
        req.paymentPayload = payload;
        req.paymentRequirements = requirements;

        String body = Json.MAPPER.writeValueAsString(req);
        String responseBody = doPost(VERIFY_PATH, body);

        return Json.MAPPER.readValue(responseBody, VerifyResponse.class);
    }

    @Override
    public SettleResponse settle(PaymentPayload payload,
                                 PaymentRequirements requirements)
            throws IOException, InterruptedException {
        // Build V2 SettleRequest body
        SettleRequest req = new SettleRequest();
        req.x402Version = 2;
        req.paymentPayload = payload;
        req.paymentRequirements = requirements;

        String body = Json.MAPPER.writeValueAsString(req);
        String responseBody = doPost(SETTLE_PATH, body);

        return Json.MAPPER.readValue(responseBody, SettleResponse.class);
    }

    @Override
    public SupportedResponse supported() throws IOException, InterruptedException {
        String responseBody = doGet(SUPPORTED_PATH);
        return Json.MAPPER.readValue(responseBody, SupportedResponse.class);
    }

    private String doPost(String path, String body) throws IOException, InterruptedException {
        Map<String, String> headers = auth.createHeaders("POST", path, body);

        HttpRequest.Builder builder = HttpRequest.newBuilder()
                .uri(URI.create(baseUrl + path))
                .POST(HttpRequest.BodyPublishers.ofString(body))
                .timeout(Duration.ofSeconds(30));

        headers.forEach(builder::header);

        HttpResponse<String> resp = http.send(builder.build(),
                HttpResponse.BodyHandlers.ofString());

        return handleResponse(resp, path);
    }

    private String doGet(String path) throws IOException, InterruptedException {
        Map<String, String> headers = auth.createHeaders("GET", path, "");

        HttpRequest.Builder builder = HttpRequest.newBuilder()
                .uri(URI.create(baseUrl + path))
                .GET()
                .timeout(Duration.ofSeconds(30));

        headers.forEach((k, v) -> builder.header(k, v));

        HttpResponse<String> resp = http.send(builder.build(),
                HttpResponse.BodyHandlers.ofString());

        return handleResponse(resp, path);
    }

    private String handleResponse(HttpResponse<String> resp, String path)
            throws IOException {
        if (resp.statusCode() == 200) {
            return resp.body();
        }
        // Try to parse OKX error envelope {code, msg}
        try {
            var node = Json.MAPPER.readTree(resp.body());
            String code = node.has("code") ? node.get("code").asText() : "";
            String msg = ERROR_MAP.getOrDefault(code,
                    node.has("msg") ? node.get("msg").asText() : "Unknown error");
            throw new IOException("OKX API error on " + path +
                    " (code=" + code + "): " + msg);
        } catch (IOException rethrow) {
            throw rethrow;
        } catch (Exception e) {
            throw new IOException("OKX API HTTP " + resp.statusCode() +
                    " on " + path + ": " + resp.body());
        }
    }
}
```

> **[Fills G-1]** Credentials → construct succeeds
> **[Fills G-2]** Every request carries OK-ACCESS-* headers
> **[Fills G-3]** verify sends V2 `amount` directly (OKX /api/v6 is V2-native per Doc 4)
> **[Fills G-4]** settle response uses `transaction` (not `txHash`) — V2 native
> **[Fills G-5]** supported returns CAIP-2 `network` (not `chainIndex`) — V2 native
> **[Fills G-9]** Error code 50103 → "Invalid API key"
> **[Fills ERR-1]** Invalid API key → readable error from OKX + local mapping
> **[Fills ERR-5]** 30s timeout; IOException message includes "which API, which path"

**Important note on V2 vs V1 bridge:**
Per Doc 4, OKX Facilitator `/api/v6` endpoints are **V2-native**. The Java SDK talks V2 directly — no V2→V1 downgrade
needed. This is why Doc 1 marks G-6/G-7 as N/A for Java.

#### `facilitator/FacilitatorRouter.java`

```java
public class FacilitatorRouter implements FacilitatorClient {
    private final Map<String, FacilitatorClient> routeMap = new HashMap<>();
    private final FacilitatorClient defaultClient;

    private FacilitatorRouter(Builder builder) { ...}

    public static Builder builder() {
        return new Builder();
    }

    public static class Builder {
        private OKXFacilitatorClient okxClient;
        private FacilitatorClient defaultClient;
        private final Map<String, FacilitatorClient> routes = new HashMap<>();

        public Builder okx(String apiKey, String secretKey, String passphrase) {
            this.okxClient = new OKXFacilitatorClient(apiKey, secretKey, passphrase);
            // Auto-route X Layer to OKX
            routes.put("eip155:196", okxClient);
            routes.put("eip155:195", okxClient);
            return this;
        }

        public Builder route(String network, FacilitatorClient client) {
            routes.put(network, client);
            return this;
        }

        public Builder defaultFacilitator(FacilitatorClient client) {
            this.defaultClient = client;
            return this;
        }

        public FacilitatorRouter build() {
            return new FacilitatorRouter(this);
        }
    }

    @Override
    public VerifyResponse verify(PaymentPayload payload, PaymentRequirements req)
            throws IOException, InterruptedException {
        return getClient(req.network).verify(payload, req);
    }

    @Override
    public SettleResponse settle(PaymentPayload payload, PaymentRequirements req)
            throws IOException, InterruptedException {
        return getClient(req.network).settle(payload, req);
    }

    @Override
    public SupportedResponse supported() {
        // Merge all facilitators' supported lists
    }

    private FacilitatorClient getClient(String network) {
        FacilitatorClient client = routeMap.get(network);
        if (client != null) return client;
        if (defaultClient != null) return defaultClient;
        throw new IllegalStateException("No facilitator configured for network: " + network);
    }
}
```

**Interface update — FacilitatorClient must change signature to V2:**

```java
public interface FacilitatorClient {
    VerifyResponse verify(PaymentPayload payload, PaymentRequirements requirements)
            throws IOException, InterruptedException;

    SettleResponse settle(PaymentPayload payload, PaymentRequirements requirements)
            throws IOException, InterruptedException;

    SupportedResponse supported()
            throws IOException, InterruptedException;
}
```

> Note: All types here are from `model.v2.*`. The old V1 `HttpFacilitatorClient` moves to `facilitator/` and is updated
> to use V2 types (with V1 wire compat via format adapter if needed for Coinbase CDP).

**Tests:**

- `OKXAuthTest.java`:
    - `hmacSignatureMatchesSpec()` — verify against known test vector from Doc 3 §3
    - `missingApiKeyThrows()` → **[G-8]** "OKX API key is required"
    - `missingSecretKeyThrows()` → **[ERR-6]**
    - `missingPassphraseThrows()` → **[ERR-6]**
- `OKXFacilitatorClientTest.java` (WireMock):
    - `verifyHappyPath()` → **[G-3]**
    - `settleHappyPath()` → **[G-4]**
    - `supportedHappyPath()` → **[G-5]**
    - `authHeadersPresent()` → **[G-2]** assert OK-ACCESS-KEY/SIGN/TIMESTAMP/PASSPHRASE
    - `errorCode50103MapsToReadableMessage()` → **[G-9]** "Invalid API key"
    - `networkTimeout()` → **[ERR-5]** IOException with context
    - `invalidApiKey401()` → **[ERR-1]**
- `FacilitatorRouterTest.java`:
    - `xlayerRoutesToOkx()` — eip155:196 → OKXFacilitatorClient
    - `otherRoutesToDefault()` — eip155:8453 → default

---

### Task 4: PaymentFilter V2 Upgrade (`server/`)

**Goal:** Upgrade PaymentFilter to V2 protocol, multi-chain, dynamic pricing, one-line integration.

**Changes to `PaymentFilter.java`:**

```java
public class PaymentFilter implements Filter {

    // ---- V2 header names ----
    private static final String HEADER_PAYMENT_SIGNATURE = "PAYMENT-SIGNATURE";
    private static final String HEADER_PAYMENT_RESPONSE = "PAYMENT-RESPONSE";
    // V1 compat: also check X-PAYMENT
    private static final String HEADER_X_PAYMENT = "X-PAYMENT";
    private static final String HEADER_X_PAYMENT_RESPONSE = "X-PAYMENT-RESPONSE";

    private final FacilitatorClient facilitator;
    private final Map<String, RouteConfig> routes;  // "GET /path" → config

    // ---- Route configuration ----
    public static class RouteConfig {
        public String scheme = "exact";
        public String network;           // CAIP-2: "eip155:196"
        public String payTo;             // receiver address
        public String price;             // "$0.01" or atomic units
        public String asset;             // optional override (contract address)
        public int maxTimeoutSeconds = 86400;  // default 24h per Doc 3 §7
    }

    public PaymentFilter(FacilitatorClient facilitator, Map<String, RouteConfig> routes) {
        this.facilitator = Objects.requireNonNull(facilitator);
        this.routes = Objects.requireNonNull(routes);
    }

    // ---- Convenience factory for one-line integration ----

    /** [Fills C-2] One-line middleware:
     *  PaymentFilter.create(facilitator, Map.of(
     *      "GET /protected/xlayer-usdt", new RouteConfig() {{
     *          network = "eip155:196"; payTo = "0x..."; price = "$0.01";
     *      }}
     *  ));
     */
    public static PaymentFilter create(FacilitatorClient facilitator,
                                       Map<String, RouteConfig> routes) {
        return new PaymentFilter(facilitator, routes);
    }

    @Override
    public void doFilter(ServletRequest req, ServletResponse res, FilterChain chain)
            throws IOException, ServletException {

        if (!(req instanceof HttpServletRequest request)
                || !(res instanceof HttpServletResponse response)) {
            chain.doFilter(req, res);
            return;
        }

        String routeKey = request.getMethod() + " " + request.getRequestURI();
        RouteConfig config = routes.get(routeKey);
        if (config == null) {
            // Fallback: try path-only match (backward compat)
            config = routes.get(request.getRequestURI());
        }
        if (config == null) {
            chain.doFilter(req, res);
            return;
        }

        // Read PAYMENT-SIGNATURE (V2) or X-PAYMENT (V1 fallback)
        String header = request.getHeader(HEADER_PAYMENT_SIGNATURE);
        if (header == null || header.isEmpty()) {
            header = request.getHeader(HEADER_X_PAYMENT);
        }
        if (header == null || header.isEmpty()) {
            respond402(response, request, config);
            return;
        }

        // Decode & verify
        PaymentPayload payload;
        try {
            payload = PaymentPayload.fromHeader(header);
        } catch (Exception e) {
            respond402(response, request, config, "malformed payment header");
            return;
        }

        // Validate resource URL matches request (prevent cross-route payment replay)
        if (payload.resource != null && payload.resource.url != null) {
            String requestUrl = request.getRequestURL().toString();
            if (!payload.resource.url.equals(requestUrl)) {
                respond402(response, request, config, "resource mismatch");
                return;
            }
        }

        PaymentRequirements requirements = buildRequirements(request, config);
        VerifyResponse vr;
        try {
            vr = facilitator.verify(payload, requirements);
        } catch (IOException e) {
            response.setStatus(500);
            response.setContentType("application/json");
            response.getWriter().write(
                    "{\"error\":\"Payment verification failed: " + e.getMessage() + "\"}");
            return;
        }

        if (!vr.isValid) {
            respond402(response, request, config, vr.invalidReason);
            return;
        }

        // Execute business logic
        chain.doFilter(req, res);

        if (response.getStatus() >= 400) return;

        // Settle
        try {
            SettleResponse sr = facilitator.settle(payload, requirements);
            if (!sr.success) {
                if (!response.isCommitted()) {
                    respond402(response, request, config,
                            sr.errorReason != null ? sr.errorReason : "settlement failed");
                }
                return;
            }
            // Set V2 response header
            SettlementResponseHeader srh = new SettlementResponseHeader(
                    true, sr.transaction, sr.network, sr.payer);
            String b64 = Base64.getEncoder().encodeToString(
                    Json.MAPPER.writeValueAsBytes(srh));
            response.setHeader(HEADER_PAYMENT_RESPONSE, b64);
            response.setHeader("Access-Control-Expose-Headers", HEADER_PAYMENT_RESPONSE);
        } catch (Exception e) {
            if (!response.isCommitted()) {
                respond402(response, request, config, "settlement error: " + e.getMessage());
            }
        }
    }

    private PaymentRequirements buildRequirements(HttpServletRequest request,
                                                  RouteConfig config) {
        // [Fills C-1] Route-level pricing
        ResolvedPrice resolved = AssetRegistry.resolvePrice(config.price, config.network);

        PaymentRequirements pr = new PaymentRequirements();
        pr.scheme = config.scheme;
        pr.network = config.network;
        pr.amount = resolved.amount();
        pr.payTo = config.payTo;
        pr.maxTimeoutSeconds = config.maxTimeoutSeconds;
        pr.asset = config.asset != null ? config.asset : resolved.asset();
        pr.extra = resolved.extra();
        return pr;
    }

    private void respond402(HttpServletResponse resp, HttpServletRequest req,
                            RouteConfig config) throws IOException {
        respond402(resp, req, config, null);
    }

    private void respond402(HttpServletResponse resp, HttpServletRequest req,
                            RouteConfig config, String error) throws IOException {
        PaymentRequired pr = new PaymentRequired();
        pr.x402Version = 2;
        pr.error = error;
        pr.resource = new ResourceInfo();
        pr.resource.url = req.getRequestURL().toString();
        pr.resource.mimeType = "application/json";
        pr.accepts = List.of(buildRequirements(req, config));

        resp.setStatus(402);
        resp.setContentType("application/json");
        resp.getWriter().write(Json.MAPPER.writeValueAsString(pr));
    }
}
```

> **[Fills C-1]** Route-level pricing: each route has its own network + price config
> **[Fills C-2]** One-line: `PaymentFilter.create(facilitator, routes)` as servlet filter
> **[Fills B-2]** 402 body: V2 format with `network: "eip155:196"`, `asset`, `amount`, `extra: {name: "USD₮0"}`
> **[Fills B-3]** 200 + business JSON + `PAYMENT-RESPONSE` header (V2 name)

**Tests:**

- `PaymentFilterV2Test.java`:
    - `freeEndpointPassesThrough()`
    - `missingHeaderReturns402WithV2Body()` → **[B-2]** verify body structure
    - `v2HeaderVerifyAndSettle()` → **[B-3]** PAYMENT-RESPONSE header present
    - `v1FallbackHeaderAlsoWorks()` — X-PAYMENT header backward compat
    - `routeLevelPricing()` → **[C-1]** different routes → different amounts
    - `xlayerUsdt402ResponseCorrect()` → **[B-2]** network=eip155:196, extra.name=USD₮0
    - `settlementFailureReturns402()`
    - `handlerErrorSkipsSettlement()`
    - `verificationExceptionReturns500()`

---

### Task 5: createOKXSigner — EIP-3009 Private Key Signing (`crypto/`)

**Goal:** `createOKXSigner({privateKey})` produces a signer that creates valid EIP-3009 signatures for X Layer.

**New file: `crypto/OKXSignerFactory.java`**

```java
public final class OKXSignerFactory {

    public static class OKXSignerConfig {
        private String privateKey;       // 0x-prefixed hex
        // TEE fields reserved for Phase 2:
        // private String teeEndpoint;
        // private String teeAccessKey;
    }

    /**
     * Creates an EVM EIP-3009 signer.
     *
     * [Fills I-3] Throws immediately if no privateKey provided.
     */
    public static OKXEvmSigner createOKXSigner(OKXSignerConfig config) {
        if (config == null || config.getPrivateKey() == null
                || config.getPrivateKey().isEmpty()) {
            throw new IllegalArgumentException(
                    "Must provide either tee or privateKey");
        }
        return new OKXEvmSigner(config.getPrivateKey());
    }
}
```

**New file: `crypto/OKXEvmSigner.java`**

```java
public class OKXEvmSigner implements CryptoSigner {
    private final ECKeyPair keyPair;
    private final String address;         // checksummed 0x address

    public OKXEvmSigner(String privateKeyHex) {
        String cleaned = privateKeyHex.startsWith("0x")
                ? privateKeyHex.substring(2) : privateKeyHex;
        this.keyPair = ECKeyPair.create(new BigInteger(cleaned, 16));
        this.address = Keys.toChecksumAddress("0x" + Keys.getAddress(keyPair));
    }

    /** [Fills I-2] Returns address derived from private key */
    public String getAddress() {
        return address;
    }

    /**
     * Sign EIP-3009 TransferWithAuthorization via EIP-712.
     *
     * Input payload map keys (from PaymentRequirements + generated):
     *   amount, asset, payTo, network, extra{name, version}
     *
     * Output: ExactEIP3009Payload as Map with:
     *   signature: "0x..." (65 bytes hex)
     *   authorization: {from, to, value, validAfter, validBefore, nonce}
     *
     * [Fills I-1] Signature correct, OKX verify returns isValid: true
     */
    @Override
    public Map<String, Object> sign(PaymentRequirements requirements)
            throws CryptoSignException {
        try {
            long now = Instant.now().getEpochSecond();
            // Per Doc 3 §4.2: OKX validAfter = now - 5
            String validAfter = String.valueOf(now - 5);
            // Per Doc 3 §4.2: validBefore = now + maxTimeoutSeconds
            String validBefore = String.valueOf(now + requirements.maxTimeoutSeconds);

            // Random 32-byte nonce
            byte[] nonceBytes = new byte[32];
            new SecureRandom().nextBytes(nonceBytes);
            String nonce = "0x" + Hex.toHexString(nonceBytes);

            // Build authorization
            Authorization auth = new Authorization();
            auth.from = address;
            auth.to = requirements.payTo;
            auth.value = requirements.amount;
            auth.validAfter = validAfter;
            auth.validBefore = validBefore;
            auth.nonce = nonce;

            // Extract EIP-712 domain from requirements.extra
            String domainName = (String) requirements.extra.get("name");
            String domainVersion = (String) requirements.extra.get("version");
            int chainId = extractChainId(requirements.network);  // "eip155:196" → 196

            // Build EIP-712 typed data and sign
            byte[] structHash = buildEIP712Hash(
                    domainName, domainVersion, chainId, requirements.asset, auth);

            Sign.SignatureData sig = Sign.signMessage(structHash, keyPair, false);
            String signature = toHexSignature(sig);  // → "0x" + r + s + v

            // Return as ExactEIP3009Payload map
            Map<String, Object> payload = new LinkedHashMap<>();
            payload.put("signature", signature);
            payload.put("authorization", Map.of(
                    "from", auth.from,
                    "to", auth.to,
                    "value", auth.value,
                    "validAfter", auth.validAfter,
                    "validBefore", auth.validBefore,
                    "nonce", auth.nonce
            ));
            return payload;

        } catch (Exception e) {
            throw new CryptoSignException("EIP-3009 signing failed", e);
        }
    }

    // ---- EIP-712 internals ----

    private byte[] buildEIP712Hash(String name, String version, int chainId,
                                   String verifyingContract, Authorization auth) {
        // domainSeparator = keccak256(EIP712Domain(name,version,chainId,verifyingContract))
        // structHash = keccak256(TransferWithAuthorization(from,to,value,validAfter,validBefore,nonce))
        // return keccak256("\x19\x01" + domainSeparator + structHash)
        // Full implementation using web3j Hash.sha3() / Structured data
    }

    private static int extractChainId(String network) {
        // "eip155:196" → 196
        String[] parts = network.split(":");
        return Integer.parseInt(parts[1]);
    }

    private static String toHexSignature(Sign.SignatureData sig) {
        byte[] r = sig.getR();
        byte[] s = sig.getS();
        byte v = sig.getV()[0];
        byte[] full = new byte[65];
        System.arraycopy(r, 0, full, 0, 32);
        System.arraycopy(s, 0, full, 32, 32);
        full[64] = v;
        return "0x" + Hex.toHexString(full);
    }
}
```

> **[Fills I-1]** privateKey signing X Layer tx — signature correct, OKX verify returns isValid: true
> **[Fills I-2]** `signer.getAddress()` equals privateKey-derived address
> **[Fills I-3]** `createOKXSigner({})` → immediate error "Must provide either tee or privateKey"
> **[Fills ERR-4]** validBefore = now + maxTimeoutSeconds; if expired, facilitator rejects
> **[Fills F-3]** domain.name read from extra → "USD₮0"

**CryptoSigner interface update:**
Keep the old interface for backward compat, add new V2-aware interface alongside:

```java
// Kept for backward compat — existing CryptoSigner implementors won't break
public interface CryptoSigner {
    String sign(Map<String, Object> payload) throws CryptoSignException;
}

// New V2-aware signer interface extends CryptoSigner
public interface EvmSigner extends CryptoSigner {
    /** Sign EIP-3009 payload from V2 PaymentRequirements */
    Map<String, Object> signPaymentRequirements(PaymentRequirements requirements)
            throws CryptoSignException;

    /** Get signer's checksummed wallet address */
    String getAddress();

    /** Legacy bridge — OKXEvmSigner implements this to delegate to signPaymentRequirements */
    @Override
    default String sign(Map<String, Object> payload) throws CryptoSignException {
        throw new CryptoSignException("Use signPaymentRequirements() for V2 signing");
    }
}
```

`OKXEvmSigner` implements `EvmSigner`. `OKXHttpClient` and `PaymentFilter` use `EvmSigner`.
Old code using `CryptoSigner` still compiles against the interface.

**Tests:**

- `OKXSignerFactoryTest.java`:
    - `noConfigThrows()` → **[I-3]** "Must provide either tee or privateKey"
    - `emptyPrivateKeyThrows()` → **[I-3]**
    - `validPrivateKeyCreates()` → signer not null
- `OKXEvmSignerTest.java`:
    - `addressMatchesPrivateKey()` → **[I-2]** known private key → known address
    - `signatureValid()` → **[I-1]** sign then ecrecover → matches signer address
    - `eip712DomainUsesUnicode()` → **[F-3]** domain name with ₮ encoded correctly
    - `validAfterIsNowMinus5()` → per Doc 3 OKX-specific rule
    - `validBeforeUsesMaxTimeout()` → **[ERR-4]** proper expiration
    - `nonceIs32BytesHex()` → 0x-prefixed, 66 chars total

---

### Task 6: OKXHttpClient — Auto 402 Handling (`client/`)

**Goal:** Client that auto-intercepts 402, signs, retries with V2 headers.

**New file: `client/OKXHttpClient.java`**

```java
public class OKXHttpClient {
    private final HttpClient http;
    private final CryptoSigner signer;
    private final String network;       // default "eip155:196"

    public OKXHttpClient(CryptoSigner signer) {
        this(signer, "eip155:196");
    }

    public OKXHttpClient(CryptoSigner signer, String network) {
        this.signer = Objects.requireNonNull(signer);
        this.network = network;
        this.http = HttpClient.newHttpClient();
    }

    /**
     * GET with automatic 402 payment handling.
     *
     * Flow:
     * 1. Send GET to uri (no payment header)
     * 2. If 200 → return response
     * 3. If 402 → parse PaymentRequired body
     * 4. Select matching PaymentRequirements from accepts[]
     * 5. Sign payload via CryptoSigner
     * 6. Build PaymentPayload V2
     * 7. Retry with PAYMENT-SIGNATURE header
     * 8. Return final response
     *
     * [Fills B-1] Auto 402 → sign → retry → 200
     */
    /** Convenience GET wrapper */
    public HttpResponse<String> get(URI uri) throws IOException, InterruptedException {
        HttpRequest initial = HttpRequest.newBuilder().uri(uri).GET().build();
        return request(initial);
    }

    /** Generic request with auto 402 handling — supports GET, POST, etc. */
    public HttpResponse<String> request(HttpRequest initial) throws IOException, InterruptedException {
        URI uri = initial.uri();
        HttpResponse<String> resp = http.send(initial, HttpResponse.BodyHandlers.ofString());

        // Step 2: Not 402 → return as-is
        if (resp.statusCode() != 402) return resp;

        // Step 3: Parse 402 body
        PaymentRequired paymentRequired = Json.MAPPER.readValue(
                resp.body(), PaymentRequired.class);

        // Step 4: Select matching requirement (prefer configured network)
        PaymentRequirements selected = selectRequirement(paymentRequired.accepts);

        // Step 5: Sign
        Map<String, Object> signedPayload = signer.sign(selected);

        // Step 6: Build V2 PaymentPayload
        PaymentPayload pp = new PaymentPayload();
        pp.x402Version = 2;
        pp.accepted = selected;
        pp.resource = paymentRequired.resource;
        pp.payload = signedPayload;

        // Step 7: Retry with PAYMENT-SIGNATURE
        String header = pp.toHeader();
        HttpRequest retry = HttpRequest.newBuilder()
                .uri(uri)
                .header("PAYMENT-SIGNATURE", header)
                .GET()
                .build();

        return http.send(retry, HttpResponse.BodyHandlers.ofString());
    }

    private PaymentRequirements selectRequirement(List<PaymentRequirements> accepts) {
        // Prefer matching network, then first available
        return accepts.stream()
                .filter(r -> r.network.equals(network))
                .findFirst()
                .orElse(accepts.get(0));
    }
}
```

> **[Fills B-1]** Auto 402 handling: intercept → sign → retry → 200
> **[Fills B-3]** After successful flow, client receives 200 + business JSON + PAYMENT-RESPONSE header

**Tests:**

- `OKXHttpClientTest.java`:
    - `auto402Handling()` → **[B-1]** mock server returns 402 then 200
    - `non402PassesThrough()` → 200 returned directly
    - `paymentSignatureHeaderPresent()` → V2 header name used
    - `paymentPayloadIsV2Format()` → has accepted, resource fields

---

### Task 7: Demo Server + Client

**Goal:** Working Spring Boot server + standalone client, matching Doc 1 §3.

**New files in `demo/` or `examples/` directory:**

#### `demo/server/DemoServer.java` (Spring Boot)

```java

@SpringBootApplication
public class DemoServer implements ServletContextInitializer {

    @Override
    public void onStartup(ServletContext ctx) {
        // Build facilitator
        FacilitatorClient facilitator = FacilitatorRouter.builder()
                .okx(System.getenv("OKX_API_KEY"),
                        System.getenv("OKX_SECRET_KEY"),
                        System.getenv("OKX_PASSPHRASE"))
                .build();

        // Route config — avoid double-brace init (creates anonymous inner classes,
        // holds enclosing reference, triggers SpotBugs warnings)
        String payTo = System.getenv("PAY_TO_ADDRESS");

        PaymentFilter.RouteConfig xlayerUsdt = new PaymentFilter.RouteConfig();
        xlayerUsdt.network = "eip155:196";
        xlayerUsdt.payTo = payTo;
        xlayerUsdt.price = "$0.01";

        PaymentFilter.RouteConfig xlayerTestnet = new PaymentFilter.RouteConfig();
        xlayerTestnet.network = "eip155:195";
        xlayerTestnet.payTo = payTo;
        xlayerTestnet.price = "$0.01";

        Map<String, PaymentFilter.RouteConfig> routes = Map.of(
                "GET /protected/xlayer-usdt", xlayerUsdt,
                "GET /protected/xlayer-testnet", xlayerTestnet
        );

        // [Fills C-2] One-line middleware
        FilterRegistration.Dynamic filter = ctx.addFilter(
                "x402", PaymentFilter.create(facilitator, routes));
        filter.addMappingForUrlPatterns(null, false, "/protected/*");
    }

    @RestController
    static class Routes {
        @GetMapping("/health")
        String health() {
            return "{\"status\":\"ok\"}";
        }

        @GetMapping("/protected/xlayer-usdt")
        String xlayerUsdt() {
            return "{\"data\":\"premium X Layer content\"}";
        }

        @GetMapping("/protected/xlayer-testnet")
        String xlayerTestnet() {
            return "{\"data\":\"testnet content\"}";
        }
    }
}
```

> **[Fills C-1]** Route-level pricing: /protected/xlayer-usdt = $0.01 on eip155:196
> **[Fills C-2]** One-line filter registration

#### `demo/client/DemoClient.java`

```java
public class DemoClient {
    public static void main(String[] args) throws Exception {
        // [Fills I-1] Create signer from private key
        OKXEvmSigner signer = OKXSignerFactory.createOKXSigner(
                new OKXSignerConfig().privateKey(System.getenv("PRIVATE_KEY")));

        System.out.println("Signer address: " + signer.getAddress());

        // [Fills B-1] Auto 402 handling client
        OKXHttpClient client = new OKXHttpClient(signer, "eip155:196");

        URI uri = URI.create("http://localhost:8080/protected/xlayer-usdt");
        HttpResponse<String> resp = client.get(uri);

        System.out.println("Status: " + resp.statusCode());
        System.out.println("Body: " + resp.body());

        // [Fills B-3] Print settlement proof
        String paymentResponse = resp.headers()
                .firstValue("PAYMENT-RESPONSE").orElse(null);
        if (paymentResponse != null) {
            String json = new String(Base64.getDecoder().decode(paymentResponse));
            System.out.println("Settlement: " + json);
            // Prints tx hash, payer, network
        }
    }
}
```

---

### Task 8: End-to-End Integration Tests

**Goal:** Automated tests that validate the full payment flow matches all 28 acceptance criteria.

**Test strategy:** Pure unit assertions (G-1, G-8, I-2, I-3, F-5, ERR-6) live in their respective
unit test classes from Tasks 2-6. E2E tests here focus only on **integration flows spanning multiple
components**. This avoids duplicate test maintenance.

**New file: `test/.../integration/E2EPaymentFlowTest.java`**

Uses embedded Jetty + WireMock (mock OKX Facilitator) to test the complete chain.

```java

@ExtendWith(WireMockExtension.class)
class E2EPaymentFlowTest {

    // ===== F. X Layer Chain Support =====

    @Test
        // F-1: X Layer USDT out-of-box
    void xlayerUsdtAutoResolves() {
        // Configure route with price: "$0.01", network: "eip155:196"
        // Assert: no "No default asset" exception
        // Assert: amount = "10000" (0.01 * 10^6)
    }

    @Test
        // F-2: X Layer full payment flow
    void xlayerFullPaymentFlow() {
        // 1. Client GET /protected/xlayer-usdt → 402
        // 2. Parse 402 body → PaymentRequired V2
        // 3. Sign EIP-3009 → PAYMENT-SIGNATURE header
        // 4. Retry → mock facilitator verify(isValid:true) → settle(success:true, transaction:"0xabc")
        // 5. Assert 200 + business JSON + PAYMENT-RESPONSE header with tx hash
    }

    @Test
        // F-3: EIP-712 domain name correct
    void eip712DomainNameIsUnicode() {
        // Trigger 402 → check accepts[0].extra.name == "USD₮0"
        // Verify ₮ is U+20AE, not ASCII T
        String name = (String) paymentRequired.accepts.get(0).extra.get("name");
        assertEquals("USD\u20AE0", name);
    }

    @Test
        // F-4: X Layer Testnet payment
    void xlayerTestnetPayment() {
        // Same as F-2 but network = "eip155:195"
    }

    @Test
        // F-5: Contract address and decimals
    void contractAddressAndDecimalsCorrect() {
        AssetConfig usdt = AssetRegistry.get("eip155:196", "USDT");
        assertEquals("0x779ded0c9e1022225f8e0630b35a9b54be713736", usdt.getContractAddress());
        assertEquals(6, usdt.getDecimals());
    }

    // ===== G. OKX Facilitator =====

    @Test
        // G-1: Credentials config-and-go
    void credentialsConfigAndGo() {
        OKXFacilitatorClient client = new OKXFacilitatorClient("key", "secret", "pass");
        assertNotNull(client);
    }

    @Test
        // G-2: Auth headers auto-attached
    void authHeadersAutoAttached() {
        // WireMock: verify request has OK-ACCESS-KEY, OK-ACCESS-SIGN,
        //   OK-ACCESS-TIMESTAMP, OK-ACCESS-PASSPHRASE
    }

    @Test
        // G-3: verify format transparent conversion
    void verifyFormatTransparent() {
        // Call verify → WireMock receives V2 body with "amount" (not "maxAmountRequired")
    }

    @Test
        // G-4: settle format transparent conversion
    void settleFormatTransparent() {
        // WireMock returns {transaction: "0xabc"} → SettleResponse.transaction = "0xabc"
    }

    @Test
        // G-5: getSupported format transparent conversion
    void getSupportedFormat() {
        // WireMock returns {kinds:[{network:"eip155:196"}]}
        // → SupportedKind.network = "eip155:196"
    }

    @Test
        // G-8: Missing credentials error
    void missingCredentialsError() {
        assertThrows(IllegalArgumentException.class,
                () -> new OKXFacilitatorClient(null, "secret", "pass"),
                "OKX API key is required");
    }

    @Test
        // G-9: Error code mapping
    void errorCodeMapping() {
        // WireMock returns 401 with {code:"50103", msg:"..."}
        // → IOException message contains "Invalid API key"
    }

    // ===== I. Client Signing =====

    @Test
        // I-1: PrivateKey signing X Layer tx
    void privateKeySigningXLayer() {
        OKXEvmSigner signer = OKXSignerFactory.createOKXSigner(
                new OKXSignerConfig().privateKey(TEST_PRIVATE_KEY));
        // Sign requirements for eip155:196
        // Ecrecover from signature → matches signer.getAddress()
    }

    @Test
        // I-2: Signer address correct
    void signerAddressCorrect() {
        OKXEvmSigner signer = OKXSignerFactory.createOKXSigner(
                new OKXSignerConfig().privateKey(KNOWN_PRIVATE_KEY));
        assertEquals(KNOWN_ADDRESS, signer.getAddress());
    }

    @Test
        // I-3: No config → error
    void noConfigThrows() {
        assertThrows(IllegalArgumentException.class,
                () -> OKXSignerFactory.createOKXSigner(new OKXSignerConfig()),
                "Must provide either tee or privateKey");
    }

    // ===== B. Basic Client =====

    @Test
        // B-1: Auto 402 handling
    void auto402Handling() {
        // Start embedded server with PaymentFilter
        // OKXHttpClient.get(uri) → auto handles 402 → returns 200
    }

    @Test
        // B-2: 402 response body correct
    void response402BodyCorrect() {
        // GET /protected/xlayer-usdt without payment
        // Assert body: network="eip155:196", asset=contract, amount="10000",
        //   extra.name="USD₮0"
    }

    @Test
        // B-3: Payment success returns biz data + header
    void paymentSuccessReturnsBizData() {
        // Full flow → assert 200 body contains business JSON
        // Assert PAYMENT-RESPONSE header present and decodable
    }

    // ===== C. Basic Server =====

    @Test
        // C-1: Route-level pricing
    void routeLevelPricing() {
        // Two routes with different prices
        // Assert each returns correct amount in 402
    }

    @Test
        // C-2: Middleware one-line integration
    void middlewareOneLineIntegration() {
        // PaymentFilter.create(facilitator, routes) compiles and registers
    }

    // ===== ERR. Error Scenarios =====

    @Test
        // ERR-1: OKX API Key invalid
    void invalidApiKey() {
        // WireMock returns {code:"50103"} → IOException "Invalid API key"
    }

    @Test
        // ERR-2: Invalid signature
    void invalidSignature() {
        // WireMock verify returns {isValid:false, invalidReason:"invalid_exact_evm_payload_signature"}
        // → 402 with invalidReason in error
    }

    @Test
        // ERR-3: Insufficient balance
    void insufficientBalance() {
        // WireMock settle returns {success:false, errorReason:"insufficient_funds"}
        // → 402 with errorReason
    }

    @Test
        // ERR-4: Signature expired
    void signatureExpired() {
        // WireMock verify returns {isValid:false,
        //   invalidReason:"invalid_exact_evm_payload_authorization_valid_before"}
    }

    @Test
        // ERR-5: OKX API timeout
    void okxApiTimeout() {
        // WireMock delay > timeout → IOException with context
        // Assert message contains API path info
    }

    @Test
        // ERR-6: Facilitator not configured
    void facilitatorNotConfigured() {
        assertThrows(IllegalArgumentException.class,
                () -> new OKXFacilitatorClient(null, null, null));
    }
}
```

---

## Execution Order & Dependencies

```
Task 0: Project Scaffolding (package rename, pom.xml, deps)
  │
  ├──► Task 1: V2 Data Models (no external deps)
  │       │
  │       ├──► Task 2: Asset Registry (depends on Task 1 for PaymentRequirements)
  │       │
  │       ├──► Task 3: OKXFacilitatorClient (depends on Task 1 for V2 types)
  │       │       │
  │       │       └──► Task 4: PaymentFilter V2 (depends on Task 1, 2, 3)
  │       │
  │       └──► Task 5: OKXEvmSigner (depends on Task 1 for PaymentRequirements)
  │               │
  │               └──► Task 6: OKXHttpClient (depends on Task 1, 5)
  │
  └──► Task 7: Demo (depends on Task 3, 4, 5, 6)
       │
       └──► Task 8: E2E Tests (depends on ALL tasks)
```

**Parallelizable:**

- Task 1 → then Task 2, 3, 5 can run in parallel
- Task 4 depends on 1+2+3
- Task 6 depends on 1+5
- Task 7 & 8 are last

---

## Deliverables Checklist (per Doc 1 §6)

- [ ] SDK package (`com.okx:x402-java`, installable via Maven)
- [ ] Server Demo (`/health`, `/protected/xlayer-usdt`, `/protected/xlayer-testnet`)
- [ ] Client Demo (end-to-end payment flow runnable)
- [ ] 28 acceptance tests all passing (26 active + 2 N/A)

---

## Risk Items

| Risk                                         | Mitigation                                                                |
|----------------------------------------------|---------------------------------------------------------------------------|
| X Layer Testnet USDT contract address TBD    | Use mainnet for dev, swap when available                                  |
| USDG EIP-712 name TBD                        | Placeholder in registry, update when confirmed                            |
| web3j EIP-712 structured data API complexity | Consider manual keccak256 if web3j's structured data util is insufficient |
| JaCoCo 85% coverage with new code            | Write tests alongside each task, not after                                |
| OKX /api/v6 might not be live yet            | WireMock all facilitator tests; E2E against real API gated behind env var |
| `agentic-press` scheme out of scope          | Explicitly Phase 2. Only `exact` EIP-3009 in Phase 1. Doc 4 lists it but no SDK work needed now |
