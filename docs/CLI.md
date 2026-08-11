# AgentVerify CLI

**Version:** 1.0
**Created:** 2026-08-11

---

## Commands

### agentverify init

Initialize AgentVerify in a project.

```bash
agentverify init [OPTIONS]
```

**Options:**
- `--path <PATH>` — Project directory (default: current)
- `--template <TEMPLATE>` — Contract template to use

**Examples:**
```bash
agentverify init
agentverify init --path ./my-project --template rest-api
```

---

### agentverify contract

Manage verification contracts.

#### Validate

Validate a contract file.

```bash
agentverify contract validate <FILE>
```

**Examples:**
```bash
agentverify contract validate contracts/create_customer.yaml
agentverify contract validate contracts/*.yaml
```

#### List

List all contracts in a directory.

```bash
agentverify contract list [DIRECTORY]
```

---

### agentverify verify

Run verification manually.

```bash
agentverify verify [OPTIONS] <CONTRACT>
```

**Options:**
- `--action <ACTION>` — Action name
- `--args <ARGS>` — JSON arguments
- `--observe <SOURCE>` — Observer to use (postgres, rest, redis)
- `--timeout <TIMEOUT>` — Verification timeout

**Examples:**
```bash
agentverify verify contracts/create_customer.yaml \
  --action create_customer \
  --args '{"email": "test@example.com"}' \
  --observe postgres
```

---

### agentverify observe

Query a system of record directly.

```bash
agentverify observe <SOURCE> [OPTIONS]
```

**Examples:**
```bash
# PostgreSQL observation
agentverify observe postgres \
  --query "SELECT * FROM customers WHERE email = 'test@example.com'"

# REST observation
agentverify observe rest \
  --url "https://api.example.com/customers/123" \
  --method GET
```

---

### agentverify serve

Start the AgentVerify HTTP gateway.

```bash
agentverify serve [OPTIONS]
```

**Options:**
- `--config <FILE>` — Configuration file
- `--host <HOST>` — Listen host (default: 127.0.0.1)
- `--port <PORT>` — Listen port (default: 8080)
- `--tls` — Enable TLS

**Examples:**
```bash
agentverify serve --config agentverify.yaml
agentverify serve --port 9000
```

**Endpoints:**
- `POST /verify` — Run verification
- `POST /contracts` — Create contract
- `GET /contracts/<id>` — Get contract
- `GET /receipts/<id>` — Get receipt
- `GET /health` — Health check
- `GET /metrics` — Prometheus metrics

---

### agentverify mcp

Run MCP proxy mode.

```bash
agentverify mcp proxy [OPTIONS]
```

**Options:**
- `--server <SERVER>` — MCP server to proxy (e.g., `filesystem`, `postgres`)
- `--contracts <DIR>` — Directory containing contracts
- `--port <PORT>` — Listen port (default: 8081)

**Examples:**
```bash
# Proxy a filesystem MCP server
agentverify mcp proxy --server filesystem --contracts ./contracts

# Proxy a PostgreSQL MCP server
agentverify mcp proxy --server postgres://localhost/db --contracts ./contracts
```

---

### agentverify receipt

Verify and inspect receipts.

#### Verify

Verify receipt integrity and signature.

```bash
agentverify receipt verify <FILE>
```

#### Inspect

Display receipt details.

```bash
agentverify receipt inspect <FILE>
```

---

### agentverify doctor

Check AgentVerify installation health.

```bash
agentverify doctor
```

**Checks:**
- Configuration file validity
- Observer connectivity
- Storage backend availability
- OpenTelemetry endpoint

---

### agentverify test

Run contract tests with failure injection.

```bash
agentverify test [OPTIONS] [CONTRACT]
```

**Options:**
- `--inject <FAILURE>` — Failure type to inject
- `--iterations <N>` — Number of test iterations

**Failure types:**
- `timeout-before-dispatch`
- `timeout-after-dispatch`
- `connection-reset`
- `http-500`
- `http-503`
- `delayed-response`
- `duplicate-response`
- `partial-write`
- `stale-read`

**Examples:**
```bash
agentverify test contracts/create_customer.yaml
agentverify test --inject timeout-after-dispatch --iterations 100
```

---

## Configuration File

`agentverify.yaml`:

```yaml
version: "1.0"

observers:
  postgres:
    connection_string: "postgres://user:pass@localhost/db"
    pool_size: 10

  rest:
    default_timeout: 5000ms
    retry_attempts: 3

  redis:
    url: "redis://localhost:6379"

storage:
  type: postgres
  connection_string: "postgres://user:pass@localhost/agentverify"

telemetry:
  enabled: true
  endpoint: "http://localhost:4317"
  service_name: "agentverify"

gateway:
  host: "0.0.0.0"
  port: 8080

idempotency:
  ttl: 86400  # 24 hours
```

---

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Verification failed |
| 2 | Contract invalid |
| 3 | Configuration error |
| 4 | Observer error |
| 5 | Timeout |
| 99 | Internal error |
