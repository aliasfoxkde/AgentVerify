# AgentVerify Integrations

**Version:** 1.0
**Created:** 2026-08-11

---

## PostgreSQL Observer

Verify PostgreSQL state changes.

### Contract Example

```yaml
action: create_customer

observer:
  type: postgres
  connection_string: ${POSTGRES_URL}

postconditions:
  - query: |
      SELECT EXISTS(
        SELECT 1 FROM customers WHERE email = $args.email
      ) as exists
    result:
      exists: true

  - query: |
      SELECT status FROM customers WHERE email = $args.email
    result:
      status: "active"
```

### Query Parameters

| Parameter | Description |
|-----------|-------------|
| `$args.<field>` | Action argument value |
| `$result.<field>` | Previous query result |
| `$prev.<field>` | Previous action result |

### Consistency Modes

| Mode | Behavior |
|------|----------|
| `transaction` | Verify within same transaction |
| `read_committed` | Default PostgreSQL isolation |
| `snapshot` | Serializable snapshot |

---

## REST Observer

Verify REST API state.

### Contract Example

```yaml
action: create_order

observer:
  type: rest
  base_url: ${API_BASE_URL}

postconditions:
  - request:
      method: GET
      path: /customers/$args.customer_id
    result:
      status: 200
      body:
        status: "active"

  - request:
      method: GET
      path: /orders?customer_id=$args.customer_id
    result:
      body:
        items:
          $queryCount: 1
```

---

## Redis Observer

Verify Redis state.

### Contract Example

```yaml
action: cache_user_session

observer:
  type: redis
  url: ${REDIS_URL}

postconditions:
  - key: "session:$args.session_id"
    exists: true

  - key: "session:$args.session_id"
    fields:
      user_id: "$args.user_id"
      status: "active"
```

---

## MCP Integration

AgentVerify can intercept MCP tool calls.

### Proxy Mode

```bash
agentverify mcp proxy \
  --server postgres-mcp \
  --contracts ./contracts \
  --port 8081
```

### Tool-to-Contract Mapping

When MCP tool `create_customer` is called:

1. AgentVerify looks for `create_customer.yaml` in contracts dir
2. Executes the tool call
3. Observes the result via configured observer
4. Verifies postconditions
5. Returns result with receipt

### Annotations (Untrusted)

MCP annotations are hints only:

| Annotation | AgentVerify Behavior |
|------------|----------------------|
| `readOnlyHint` | Logged, not trusted |
| `destructiveHint` | Logged, verification still runs |
| `idempotentHint` | Used for retry strategy |
| `openWorldHint` | Ignored |

---

## OpenTelemetry

Export traces and metrics.

### Trace Spans

```
agentverify.action          # Action started
  └── agentverify.execute  # External call
        └── agentverify.observe  # State observation
              └── agentverify.verify  # Postcondition check
```

### Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `agentverify_actions_total` | Counter | Total actions |
| `agentverify_verifications_total` | Counter | Verifications by result |
| `agentverify_verification_duration_seconds` | Histogram | Verification latency |
| `agentverify_recovery_attempts_total` | Counter | Recovery attempts |
| `agentverify_unknown_total` | Counter | UNKNOWN outcomes |

### Configuration

```yaml
telemetry:
  enabled: true
  endpoint: "http://localhost:4317"
  service_name: "agentverify"
  traces:
    enabled: true
    sampling_rate: 1.0
  metrics:
    enabled: true
```

---

## HTTP Gateway

Deploy as a standalone service.

### Docker

```bash
docker run -p 8080:8080 \
  -v ./contracts:/contracts \
  -v ./agentverify.yaml:/etc/agentverify.yaml \
  agentverify:latest serve
```

### Kubernetes

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: agentverify
spec:
  replicas: 3
  template:
    spec:
      containers:
      - name: agentverify
        image: agentverify:latest
        ports:
        - containerPort: 8080
        volumeMounts:
        - name: config
          mountPath: /etc/agentverify
      volumes:
      - name: config
        configMap:
          name: agentverify
```

---

## Language SDKs

### Rust

```rust
use agentverify_runtime::{Executor, Config};

let executor = Executor::new(Config::default())?;

let result = executor.execute(
    action,
    contract,
    observer,
).await?;

match result {
    VerificationResult::Verified => println!("Success!"),
    VerificationResult::Failed => println!("Postconditions not met"),
    VerificationResult::Unknown => println!("Could not verify"),
    _ => println!("Other result: {:?}", result),
}
```

### Python (via HTTP gateway)

```python
import httpx

client = httpx.Client(base_url="http://localhost:8080")

result = client.verify(
    contract="create_customer",
    action={
        "name": "create_customer",
        "args": {"email": "test@example.com"}
    }
)

print(result.status)  # verified, failed, unknown, ...
```

### TypeScript

```typescript
const client = new AgentVerifyClient({
  baseUrl: "http://localhost:8080"
});

const result = await client.verify({
  contract: "create_customer",
  action: {
    name: "create_customer",
    args: { email: "test@example.com" }
  }
});

console.log(result.status);
```
