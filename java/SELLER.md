# x402 Seller SDK — Java Agent Integration Reference

> **Audience:** AI coding agents (Cursor, Claude Code, Copilot, etc.).
> **Scope:** Seller (server) only. No buyer/client code. X Layer network only.
> **Goal:** Generate complete, working x402 payment-protected server code from this document alone.

---

## CONSTRAINTS

- Network: `eip155:196` (X Layer mainnet) — no other networks
- Facilitator client: always use `OKXFacilitatorClient` — never the V1 `HttpFacilitatorClient`
- Default payment token: USDT0 at `0x779ded0c9e1022225f8e0630b35a9b54be713736` (6 decimals, EIP-712 name `USD₮0` U+20AE) — pre-registered in `AssetRegistry`
- Prices expressed as USD strings (e.g. `"$0.01"`) are auto-converted to USDT0 atomic units
- `PaymentFilter` / `PaymentInterceptor` is registered at startup; **no `initialize()` call** is required (the OKX facilitator is invoked lazily on the first protected request)
- Supported schemes: `exact`, `aggr_deferred`
- Java 17+ runtime; Maven 3.8+
- **Pick exactly one** of `x402-java-jakarta` or `x402-java-javax` — the two adapters share package names and will collide on the classpath

---

## PACKAGES

```
com.okx:x402-java-core      — OKXFacilitatorClient, PaymentProcessor, hooks, model, crypto
com.okx:x402-java-jakarta   — Jakarta EE 9+ / Spring Boot 3 (jakarta.servlet.*) PaymentFilter & PaymentInterceptor
com.okx:x402-java-javax     — Java EE 8 / Spring Boot 2 (javax.servlet.*) PaymentFilter & PaymentInterceptor
```

Install (Maven, Spring Boot 3 / Jakarta):

```xml
<dependency>
  <groupId>com.okx</groupId>
  <artifactId>x402-java-jakarta</artifactId>
  <version>1.0.0</version>
</dependency>
```

For Spring Boot 2 / Java EE 8, swap the `artifactId` to `x402-java-javax`. The core artifact is pulled in transitively.

For non-servlet stacks (Vert.x, Play, Micronaut Netty, plain Undertow), depend on `com.okx:x402-java-core` directly and implement the `X402Request` / `X402Response` interfaces against your native types.

---

## IMPORTS

```java
// Core — always needed
import com.okx.x402.facilitator.OKXFacilitatorClient;
import com.okx.x402.server.PaymentProcessor;
import com.okx.x402.server.PaymentHooks;

// Multi-currency
import com.okx.x402.server.AcceptOption;

// Adapter — pick ONE based on chosen artifact
// jakarta (Spring Boot 3 / Jakarta EE 9+):
import com.okx.x402.server.PaymentFilter;        // servlet Filter
import com.okx.x402.server.PaymentInterceptor;   // Spring 6 HandlerInterceptor
// javax (Spring Boot 2 / Java EE 8):
// same FQNs, but resolved from x402-java-javax — do not mix

// Optional facilitator routing
import com.okx.x402.facilitator.FacilitatorRouter;
import com.okx.x402.facilitator.FacilitatorClient;

// Custom asset registration
import com.okx.x402.config.AssetRegistry;
import com.okx.x402.config.AssetConfig;

// Tuning facilitator HTTP behaviour
import com.okx.x402.facilitator.OKXFacilitatorConfig;
```

---

## SETUP PATTERN (all frameworks share this)

```java
// Step 1: Create facilitator client (HMAC-SHA256 auth is automatic)
OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
        System.getenv("OKX_API_KEY"),       // required
        System.getenv("OKX_SECRET_KEY"),    // required
        System.getenv("OKX_PASSPHRASE"));   // required

// Or with full config — tune timeouts, share an HttpClient, plug in OkHttp/Reactor Netty
OKXFacilitatorConfig cfg = new OKXFacilitatorConfig(apiKey, secretKey, passphrase);
cfg.requestTimeout = Duration.ofSeconds(60);
// cfg.baseUrl     = "https://web3.okx.com";   // optional override
// cfg.httpClient  = mySharedJdkClient;        // optional JDK HttpClient
// cfg.httpExecutor = new OkHttpExecutor(myOkHttp);   // optional SPI for OkHttp/Apache/Netty
OKXFacilitatorClient facilitator = new OKXFacilitatorClient(cfg);

// Step 2: Build per-route config
PaymentProcessor.RouteConfig route = new PaymentProcessor.RouteConfig();
route.scheme            = "exact";
route.network           = "eip155:196";
route.payTo             = "0xYourWalletAddress";
route.price             = "$0.01";              // USD string → USDT0 atomic units
route.maxTimeoutSeconds = 300;                  // optional, payment signature validity
// route.syncSettle    = true;                  // see SETTLE MODES
// route.asyncSettle   = true;                  // see ASYNC SETTLE

// Step 3: Create the adapter (servlet Filter or Spring HandlerInterceptor)
PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
        "GET /api/resource", route));           // key format: "METHOD /path"

// Step 4: Register with the framework (see per-framework sections below)
// Step 5: NO initialize() call — adapter is ready as soon as it's wired in
```

---

## ROUTE CONFIG REFERENCE

### Routes map

```java
Map<String, PaymentProcessor.RouteConfig> routes = Map.of(
    "GET  /api/data",      cfgA,
    "POST /api/generate",  cfgB
);
// Key format: "METHOD /path" — METHOD is uppercase HTTP verb
```

### RouteConfig fields

```java
public class RouteConfig {
    public String scheme;                  // REQUIRED unless `accepts` is set: "exact" | "aggr_deferred"
    public String network;                 // REQUIRED: "eip155:196"
    public String payTo;                   // REQUIRED: EVM wallet address
    public Object price;                   // REQUIRED unless `accepts` is set: "$0.01" | 0.01 | AssetAmount
    public Integer maxTimeoutSeconds;      // payment signature validity, default per facilitator
    public Boolean syncSettle;             // wait for chain confirmation before returning 200
    public Boolean asyncSettle;            // run /settle in background, requires settleExecutor
    public List<AcceptOption> accepts;     // multi-currency / multi-scheme alternative to scheme+price
    public String description;             // optional human-readable note returned in 402 envelope
    public Map<String, Object> extra;      // forwarded to PaymentRequirements.extra
}
```

### AcceptOption (multi-currency on a single endpoint)

```java
AcceptOption.builder()
    .scheme("exact")              // REQUIRED
    .network("eip155:196")        // optional, inherits from RouteConfig
    .payTo(payTo)                 // optional, inherits from RouteConfig
    .price("$0.01")               // REQUIRED
    .asset("0x...")               // optional, defaults to USDT0
    .maxTimeoutSeconds(300)       // optional
    .build();
```

### Price formats

| Format         | Example                                    | Behavior                                       |
|----------------|--------------------------------------------|------------------------------------------------|
| USD string     | `"$0.01"`                                  | Converted to USDT0 atomic units (10000)        |
| Number         | `0.01`                                     | Same as USD string                             |
| AssetAmount    | `new AssetAmount("0x779d...", "10000")`    | Direct token + atomic units                    |

---

## FRAMEWORK: SPRING BOOT 3 (Jakarta)

Use the `x402-java-jakarta` adapter. Two registration styles — `PaymentFilter` (recommended) or `PaymentInterceptor`.

### PaymentFilter (recommended — preserves `PAYMENT-RESPONSE` header)

```java
import com.okx.x402.facilitator.OKXFacilitatorClient;
import com.okx.x402.server.PaymentFilter;
import com.okx.x402.server.PaymentProcessor;
import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;
import org.springframework.boot.web.servlet.FilterRegistrationBean;
import org.springframework.context.annotation.Bean;

import java.util.Map;

@SpringBootApplication
public class App {

    public static void main(String[] args) {
        SpringApplication.run(App.class, args);
    }

    @Bean
    OKXFacilitatorClient facilitator() {
        return new OKXFacilitatorClient(
                System.getenv("OKX_API_KEY"),
                System.getenv("OKX_SECRET_KEY"),
                System.getenv("OKX_PASSPHRASE"));
    }

    @Bean
    FilterRegistrationBean<PaymentFilter> x402Filter(OKXFacilitatorClient facilitator) {
        PaymentProcessor.RouteConfig route = new PaymentProcessor.RouteConfig();
        route.scheme  = "exact";
        route.network = "eip155:196";
        route.payTo   = System.getenv("PAY_TO_ADDRESS");
        route.price   = "$0.01";
        route.syncSettle = true;

        PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
                "GET /api/premium", route));

        FilterRegistrationBean<PaymentFilter> reg = new FilterRegistrationBean<>(filter);
        reg.addUrlPatterns("/api/*");
        reg.setOrder(20);                   // run after billing/auth filters at lower order
        return reg;
    }
}

// @RestController stays as usual
@RestController
class PremiumController {
    @GetMapping("/api/premium")
    public Map<String, String> premium() {
        return Map.of("data", "premium content");
    }
}
```

### PaymentInterceptor (Spring MVC interceptor)

> ⚠ **Caveat:** when used with `@RestController` (i.e. `@ResponseBody` flows), Spring's
> message converter writes the response body during `RequestMappingHandlerAdapter.handle(...)`
> which commits the response before `postHandle` runs. Settlement still happens, but the
> `PAYMENT-RESPONSE` proof header is silently dropped. **If your buyer needs the proof header,
> use `PaymentFilter` instead.**

```java
import com.okx.x402.server.PaymentInterceptor;
import org.springframework.context.annotation.Configuration;
import org.springframework.web.servlet.config.annotation.InterceptorRegistry;
import org.springframework.web.servlet.config.annotation.WebMvcConfigurer;

import java.util.Map;

@Configuration
class X402Config implements WebMvcConfigurer {
    private final OKXFacilitatorClient facilitator;
    private final Map<String, PaymentProcessor.RouteConfig> routes;

    X402Config(OKXFacilitatorClient facilitator,
               Map<String, PaymentProcessor.RouteConfig> routes) {
        this.facilitator = facilitator;
        this.routes = routes;
    }

    @Override
    public void addInterceptors(InterceptorRegistry r) {
        r.addInterceptor(billingInterceptor).order(10);
        r.addInterceptor(PaymentInterceptor.create(facilitator, routes))
                .order(20)
                .addPathPatterns("/api/**");
    }
}
```

---

## FRAMEWORK: SPRING BOOT 2 (Javax)

Identical wiring to Spring Boot 3 — switch the dependency to `com.okx:x402-java-javax` and continue importing `com.okx.x402.server.PaymentFilter` (same FQN, resolves to the javax-flavoured adapter).

```xml
<dependency>
  <groupId>com.okx</groupId>
  <artifactId>x402-java-javax</artifactId>
  <version>1.0.0</version>
</dependency>
```

```java
// javax.servlet.* under the hood — no source changes from the SB3 example above
```

---

## FRAMEWORK: PLAIN SERVLET (Jetty / Tomcat / Spring Boot without MVC interceptors)

Register `PaymentFilter` directly on the `ServletContext`.

```java
import com.okx.x402.facilitator.OKXFacilitatorClient;
import com.okx.x402.server.PaymentFilter;
import com.okx.x402.server.PaymentProcessor;
import jakarta.servlet.ServletContext;
import org.springframework.boot.web.servlet.ServletContextInitializer;
import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;

import java.util.Map;

@SpringBootApplication
public class App implements ServletContextInitializer {

    public static void main(String[] args) {
        SpringApplication.run(App.class, args);
    }

    @Override
    public void onStartup(ServletContext ctx) {
        OKXFacilitatorClient facilitator = new OKXFacilitatorClient(
                System.getenv("OKX_API_KEY"),
                System.getenv("OKX_SECRET_KEY"),
                System.getenv("OKX_PASSPHRASE"));

        PaymentProcessor.RouteConfig route = new PaymentProcessor.RouteConfig();
        route.scheme  = "exact";
        route.network = "eip155:196";
        route.payTo   = System.getenv("PAY_TO_ADDRESS");
        route.price   = "$0.01";

        ctx.addFilter("x402",
                PaymentFilter.create(facilitator, Map.of(
                        "GET /api/premium", route)))
           .addMappingForUrlPatterns(null, false, "/api/*");
    }
}
```

For embedded Jetty / Tomcat outside Spring Boot, call `ServletContextHandler.addFilter(...)` (Jetty) or `Context.addFilterDef + addFilterMap` (Tomcat) with the same `PaymentFilter.create(...)` instance.

---

## FRAMEWORK: NON-SERVLET (Vert.x / Play / Micronaut Netty / Undertow)

Depend on `x402-java-core` only and implement the two adapter SPIs against your framework's native request/response types.

```java
import com.okx.x402.server.X402Request;
import com.okx.x402.server.X402Response;
import com.okx.x402.server.PaymentProcessor;

class VertxX402Request implements X402Request { /* ~25 lines */ }
class VertxX402Response implements X402Response { /* ~25 lines */ }

// Then wire PaymentProcessor manually in your route handler:
PaymentProcessor processor = PaymentProcessor.create(facilitator, routes);
PaymentProcessor.PreHandleResult pre = processor.preHandle(new VertxX402Request(rc));
if (pre.shouldStop()) return;            // 402 / 403 already written
// ...invoke business handler...
processor.postHandle(new VertxX402Request(rc), new VertxX402Response(rc));
```

The jakarta adapter is ~50 lines and serves as a reference implementation.

---

## SETTLE MODES

| `route.syncSettle` | Behavior                                                                | Use when                                                              |
|--------------------|-------------------------------------------------------------------------|-----------------------------------------------------------------------|
| `true`             | Facilitator waits for on-chain confirmation, returns `status="success"` | High-value resources, need payment proof before delivery              |
| `false` / unset    | Facilitator returns `status="pending"` immediately                      | Low-value, high-throughput, acceptable delivery-before-confirm        |

| `route.asyncSettle` | Behavior                                                                                  |
|---------------------|-------------------------------------------------------------------------------------------|
| `true`              | `verify` is sync, `settle` runs on a caller-injected `Executor`. Must call `processor.settleExecutor(pool)` — otherwise `IllegalStateException` is thrown at runtime. |
| `false` / unset     | Settlement runs on the request thread (sync or async per `syncSettle`).                  |

```java
ExecutorService settlePool = Executors.newFixedThreadPool(16, r -> {
    Thread t = new Thread(r, "x402-settle"); t.setDaemon(true); return t;
});

route.asyncSettle = true;
PaymentFilter filter = PaymentFilter.create(facilitator, Map.of("GET /api/data", route));
filter.processor()
      .settleExecutor(settlePool)
      .onAsyncSettleComplete((payload, req, result, err) -> {
          if (err != null) log.error("settle failed", err);
          else log.info("settle tx={}", result.transaction);
      });
```

---

## HOOKS

### HTTP-layer hook (whitelist / abort)

```java
filter.processor().onProtectedRequest((request, routeConfig) -> {
    if ("internal".equals(request.getHeader("x-api-key"))) {
        return PaymentHooks.ProtectedRequestResult.grantAccess();        // skip payment, run handler
    }
    if (rateLimiter.isThrottled(request)) {
        return PaymentHooks.ProtectedRequestResult.abort("rate_limited"); // HTTP 403, {"error":"<reason>"}
    }
    return PaymentHooks.ProtectedRequestResult.proceed();                 // normal payment flow
});
```

Multiple hooks run in registration order; the first hook returning `grantAccess()` or `abort(reason)` wins.

### Facilitator lifecycle hooks

```java
filter.processor()
    .onBeforeVerify((payload, req) ->
        AbortResult.proceed())                                  // or AbortResult.abort("reason")
    .onAfterVerify((payload, req, resp) -> { /* metrics */ })
    .onVerifyFailure((payload, req, throwable) ->
        RecoverResult.notRecovered())                           // or RecoverResult.recovered(VerifyResponse)
    .onBeforeSettle((payload, req) ->
        AbortResult.proceed())
    .onAfterSettle((payload, req, resp) -> { /* audit */ })
    .onSettleFailure((payload, req, throwable) ->
        RecoverResult.notRecovered());
```

> The Java SDK does **not** ship an `onSettlementTimeout` hook. If you need on-chain
> verification when `status="timeout"` is returned, do it inside `onAfterSettle`
> by inspecting `resp.status` and querying the chain via web3j (see SETTLEMENT
> TIMEOUT RECOVERY below).

---

## MULTIPLE SCHEMES (exact + aggr_deferred)

```java
PaymentProcessor.RouteConfig exactRoute = new PaymentProcessor.RouteConfig();
exactRoute.scheme  = "exact";
exactRoute.network = "eip155:196";
exactRoute.payTo   = payTo;
exactRoute.price   = "$0.01";

PaymentProcessor.RouteConfig deferredRoute = new PaymentProcessor.RouteConfig();
deferredRoute.scheme  = "aggr_deferred";   // session-key + TEE batch
deferredRoute.network = "eip155:196";
deferredRoute.payTo   = payTo;
deferredRoute.price   = "$0.001";

PaymentFilter filter = PaymentFilter.create(facilitator, Map.of(
        "GET /api/standard", exactRoute,
        "GET /api/agent",    deferredRoute));
```

`aggr_deferred` requires the buyer to use a session key (issued by OKX Wallet TEE)
and an x402 AA wallet. The seller integration is identical to `exact`; only the
`scheme` string differs.

---

## MULTI-CURRENCY ON A SINGLE ENDPOINT (USDT + USDG)

```java
AssetRegistry.register("eip155:196", AssetConfig.builder()
        .symbol("USDG")
        .contractAddress("0x4ae46a509f6b1d9056937ba4500cb143933d2dc8")
        .decimals(6)
        .eip712Name("USDG")
        .eip712Version("1")
        .transferMethod("eip3009")
        .build());

PaymentProcessor.RouteConfig route = new PaymentProcessor.RouteConfig();
route.network = "eip155:196";
route.payTo   = payTo;
route.accepts = List.of(
    AcceptOption.builder().scheme("exact").price("$0.01").build(),                          // USDT0 default
    AcceptOption.builder().scheme("exact").price("$0.01")
        .asset("0x4ae46a509f6b1d9056937ba4500cb143933d2dc8").build()                        // USDG
);
```

Custom assets must be registered **before** `PaymentFilter.create(...)`.

---

## SETTLEMENT TIMEOUT RECOVERY (on-chain verification)

```java
import org.web3j.protocol.Web3j;
import org.web3j.protocol.http.HttpService;
import org.web3j.protocol.core.methods.response.TransactionReceipt;

Web3j web3j = Web3j.build(new HttpService("https://rpc.xlayer.tech"));

filter.processor().onAfterSettle((payload, req, resp) -> {
    if (!"timeout".equals(resp.status) || resp.transaction == null) return;

    try {
        TransactionReceipt receipt = web3j.ethGetTransactionReceipt(resp.transaction)
                .send()
                .getTransactionReceipt()
                .orElse(null);
        if (receipt != null && receipt.isStatusOK()) {
            // Fall back to "success" — log, audit, expose to downstream as confirmed
            confirmedTxAuditor.record(payload, resp.transaction);
        }
    } catch (Exception e) {
        log.warn("on-chain fallback failed for tx={}", resp.transaction, e);
    }
});
```

---

## SERVER-SIDE LIMITATIONS (read before going to prod)

`PaymentFilter` wraps the response in an in-memory `BufferedHttpServletResponse`
so settlement can attach the `PAYMENT-RESPONSE` header **after** the handler
returns. Three constraints follow:

1. **Do not call `response.sendError(...)` on a paid route.** It commits the
   underlying response before settlement can run; the buyer client will not see
   the `PAYMENT-RESPONSE` header even if the path was already paid for. Use
   `setStatus(...) + getWriter().write(...)` instead.
2. **Async / non-blocking I/O is not supported on paid routes.** The buffered
   `ServletOutputStream` is synchronous; handlers using
   `setWriteListener(...)` will not receive callbacks. Streaming responses
   (`StreamingResponseBody`, server-sent events, oversized chunked transfers)
   should not be marked as paid routes.
3. **`@RestController` + `PaymentInterceptor` drops the proof header.** See the
   warning in the Spring MVC section above. Use `PaymentFilter` for
   `@ResponseBody` / `@RestController` flows.

---

## ENV VARS

```bash
export OKX_API_KEY=your-api-key
export OKX_SECRET_KEY=your-secret-key
export OKX_PASSPHRASE=your-passphrase
export PAY_TO_ADDRESS=0xYourWalletAddress
```

---

## DECISION TREE

```
Need to protect an HTTP endpoint with payment?
│
├─ Which servlet API version is the host running?
│  ├─ Jakarta (Spring Boot 3, Jetty 11+, Tomcat 10+)  → com.okx:x402-java-jakarta
│  └─ Javax   (Spring Boot 2, Jetty 9-10, Tomcat 8-9) → com.okx:x402-java-javax
│
├─ Which adapter style?
│  ├─ Already use Spring MVC interceptors                → PaymentInterceptor
│  │  (⚠ DOES NOT preserve PAYMENT-RESPONSE on @RestController flows)
│  ├─ Need PAYMENT-RESPONSE proof header                 → PaymentFilter
│  ├─ Plain servlet container (Jetty / Tomcat)           → PaymentFilter via ServletContext
│  └─ Non-servlet (Vert.x / Play / Netty)                → core artifact + implement X402Request/Response
│
├─ Payment confirmation before delivery?
│  ├─ Yes → route.syncSettle = true
│  └─ No  → omit syncSettle
│
├─ Want to keep request thread free during settle?
│  ├─ Yes → route.asyncSettle = true + processor.settleExecutor(pool)
│  └─ No  → omit (default = settle on request thread)
│
└─ Which scheme(s)?
   ├─ Standard EOA payment        → "exact"          (default)
   ├─ AI-agent high-frequency     → "aggr_deferred"  (requires buyer-side session key + AA wallet)
   └─ Both                        → use route.accepts = List.of(exact, aggr_deferred)
```

---

## COMMON MISTAKES

| Mistake                                                         | Fix                                                                                                          |
|-----------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------|
| Depended on both `x402-java-jakarta` and `x402-java-javax`      | Pick one; they share package names and collide                                                               |
| Used `HttpFacilitatorClient` (V1)                                | Always use `OKXFacilitatorClient`                                                                            |
| `route.asyncSettle = true` without `processor.settleExecutor()` | Inject your own thread pool — SDK throws `IllegalStateException` rather than silently spawn threads          |
| Called `response.sendError(...)` on a paid route                | Use `setStatus + getWriter().write(...)` — `sendError` commits the response and drops `PAYMENT-RESPONSE`     |
| `@RestController` + `PaymentInterceptor` and need proof header   | Switch to `PaymentFilter`; Spring's converter commits the response before `postHandle` runs                  |
| Hardcoded token amount without understanding decimals            | Use USD string `"$0.01"` — SDK converts to USDT0 atomic units automatically                                  |
| Used a network other than `eip155:196`                           | Only X Layer mainnet is supported                                                                            |
| Registered a custom asset **after** `PaymentFilter.create(...)`  | Register via `AssetRegistry` first, then create the filter                                                   |
| Looked for an `initialize()` call like the TS SDK                | Java adapter is ready as soon as it's wired in — there is no `initialize()`                                  |
| Put a streaming endpoint behind `PaymentFilter`                  | Streaming / SSE / non-blocking I/O is not supported; expose those endpoints unprotected or pre-charge upstream |
