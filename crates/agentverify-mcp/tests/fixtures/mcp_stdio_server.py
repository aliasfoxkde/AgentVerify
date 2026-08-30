#!/usr/bin/env python3
"""MCP server fixture for the agentverify-mcp integration tests.

This is a real protocol peer, not a canned recorder: it parses each
newline-delimited JSON-RPC 2.0 request from stdin, dispatches on the method and
writes a response to stdout. That is the same framing ``StdioTransport``
implements, and every payload uses the lowerCamelCase field names of the MCP
specification (``inputSchema``, ``protocolVersion``, ``serverInfo``,
``mimeType``, ``isError``), as the specification's own peers do.

Usage::

    mcp_stdio_server.py [MODE]

Modes
-----
ok            answer initialize/tools/resources/prompts (default)
bad-json      write one malformed line, then answer normally
blank         write a blank line, then answer normally
error         answer every request with a JSON-RPC error
unknown-id    write a response for an id nobody is waiting for, then answer
ping-client   write a server-to-client ``ping`` request before answering, and
              announce ``notifications/ack`` once the client answers it
silent        read stdin but never answer (timeout tests)
crash         exit immediately with status 3

Except for ``crash``, every mode reads stdin to EOF, so the process ends as soon
as the transport is dropped and its stdin pipe closes.
"""

import json
import sys

PROTOCOL_VERSION = "2026-07-28"

SERVER_INFO = {"name": "agentverify-test-server", "version": "0.1.0"}

CAPABILITIES = {
    "tools": {},
    "resources": {"subscribe": True, "list": True},
    "prompts": {"list": True},
}

TOOLS = [
    {
        "name": "lookup_order",
        "description": "Look up an order in the system of record.",
        "inputSchema": {
            "type": "object",
            "properties": {"order_id": {"type": "string"}},
            "required": ["order_id"],
        },
        "annotations": {
            "readOnlyHint": True,
            "destructiveHint": False,
            "idempotentHint": True,
        },
    }
]

RESOURCES = [
    {
        "uri": "file:///contracts/order-verify.json",
        "name": "order-verify contract",
        "description": "Verification contract for order mutations.",
        "mimeType": "application/json",
    }
]

PROMPTS = [
    {
        "name": "summarise_order",
        "description": "Summarise an order and its verification state.",
        "arguments": [
            {
                "name": "order_id",
                "description": "Order to summarise.",
                "required": True,
            }
        ],
    }
]

# Id used for the server-to-client request in `ping-client` mode.
CLIENT_REQUEST_ID = 4242


def write(payload):
    """Write one newline-delimited JSON message and flush it."""
    sys.stdout.write(json.dumps(payload) + "\n")
    sys.stdout.flush()


def require_spec_camel_case(params):
    """Reject `initialize` params that do not use the specification's keys.

    A client that emits the Rust field names (`protocol_version`,
    `client_info`) instead of the specification's lowerCamelCase keys is not
    interoperable, so the peer answers with invalid params rather than
    accepting a dialect nothing else speaks.
    """
    if "protocolVersion" not in params or "clientInfo" not in params:
        raise ValueError(
            "initialize params must carry protocolVersion and clientInfo"
        )
    for legacy in ("protocol_version", "client_info"):
        if legacy in params:
            raise ValueError(
                "snake_case %s is not part of the MCP specification" % legacy
            )


def tool_result(params):
    """Build the `tools/call` result for `params`."""
    arguments = (params or {}).get("arguments") or {}
    order_id = arguments.get("order_id")
    if not order_id:
        raise ValueError("lookup_order requires an order_id argument")
    return {
        "content": [
            {"type": "text", "text": "order %s is VERIFIED" % order_id},
            {
                "type": "resource",
                "resource": {
                    "uri": RESOURCES[0]["uri"],
                    "mimeType": "application/json",
                    "text": '{"state": "verified"}',
                },
            },
        ],
        "isError": False,
    }


def result_for(method, params):
    """Dispatch an MCP method to its result payload."""
    if method == "initialize":
        require_spec_camel_case(params or {})
        return {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": CAPABILITIES,
            "serverInfo": SERVER_INFO,
            "instructions": "Verify the order before retrying the write.",
        }
    if method == "tools/list":
        return {"tools": TOOLS}
    if method == "tools/call":
        return tool_result(params)
    if method == "resources/list":
        return {"resources": RESOURCES}
    if method == "prompts/list":
        return {"prompts": PROMPTS}
    if method == "prompts/get":
        return {
            "messages": [
                {
                    "role": "user",
                    "content": {"type": "text", "text": "Summarise order A-1"},
                }
            ]
        }
    raise KeyError("unsupported method %s" % method)


def announce_progress():
    """Emit the notification a server sends while a long tool runs."""
    write(
        {
            "jsonrpc": "2.0",
            "method": "notifications/progress",
            "params": {"progressToken": 7, "progress": 50, "total": 100},
        }
    )


def respond(request, mode):
    """Answer one request according to `mode`."""
    ident = request.get("id")
    if ident is None:
        return  # a notification expects no response

    method = request.get("method")

    if mode == "ping-client" and method == "initialize":
        # Ask the client to do something only a server-side peer may ask for.
        write(
            {
                "jsonrpc": "2.0",
                "id": CLIENT_REQUEST_ID,
                "method": "sampling/createMessage",
                "params": {"messages": []},
            }
        )

    if mode == "error":
        write(
            {
                "jsonrpc": "2.0",
                "id": ident,
                "error": {
                    "code": -32601,
                    "message": "method %s not found" % method,
                    "data": {"method": method},
                },
            }
        )
        return

    try:
        result = result_for(method, request.get("params"))
    except (KeyError, ValueError) as exc:
        write(
            {
                "jsonrpc": "2.0",
                "id": ident,
                "error": {"code": -32602, "message": str(exc.args[0])},
            }
        )
        return

    if mode == "unknown-id":
        # A reply for a request this client never sent.
        write({"jsonrpc": "2.0", "id": ident + 9000, "result": {"tools": []}})

    if method == "tools/call":
        announce_progress()

    write({"jsonrpc": "2.0", "id": ident, "result": result})


def serve(mode):
    """Serve requests on stdin/stdout until stdin reaches EOF."""
    if mode == "bad-json":
        sys.stdout.write("this line is not json\n")
        sys.stdout.flush()
    elif mode == "blank":
        sys.stdout.write("\n")
        sys.stdout.flush()

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            request = json.loads(line)
        except json.JSONDecodeError:
            continue

        if request.get("id") == CLIENT_REQUEST_ID and "error" in request:
            # The client answered our server-to-client request; a conforming
            # client rejects methods it does not implement with -32601.
            if request["error"].get("code") == -32601:
                write(
                    {
                        "jsonrpc": "2.0",
                        "method": "notifications/ack",
                        "params": {"rejected": CLIENT_REQUEST_ID},
                    }
                )
            continue

        if mode == "silent":
            continue
        respond(request, mode)


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "ok"
    if mode == "crash":
        sys.exit(3)
    serve(mode)
    return 0


if __name__ == "__main__":
    sys.exit(main())
