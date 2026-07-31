#! /usr/bin/env python3
"""Minimal GraphQL HTTP fixture for mcp2cli integration tests.

Prints:
  PORT=<n>
  URL=http://127.0.0.1:<n>
"""

from __future__ import annotations

import json
import re
import socket
import sys
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer

GRAPHQL_SCHEMA = {
    "queryType": {"name": "Query"},
    "mutationType": {"name": "Mutation"},
    "types": [
        {
            "kind": "OBJECT",
            "name": "Query",
            "fields": [
                {
                    "name": "users",
                    "description": "List all users",
                    "args": [],
                    "type": {
                        "kind": "NON_NULL",
                        "name": None,
                        "ofType": {
                            "kind": "LIST",
                            "name": None,
                            "ofType": {
                                "kind": "NON_NULL",
                                "name": None,
                                "ofType": {
                                    "kind": "OBJECT",
                                    "name": "User",
                                    "ofType": None,
                                },
                            },
                        },
                    },
                },
                {
                    "name": "user",
                    "description": "Get a user by ID",
                    "args": [
                        {
                            "name": "id",
                            "description": "User ID",
                            "type": {
                                "kind": "NON_NULL",
                                "name": None,
                                "ofType": {
                                    "kind": "SCALAR",
                                    "name": "ID",
                                    "ofType": None,
                                },
                            },
                            "defaultValue": None,
                        }
                    ],
                    "type": {
                        "kind": "OBJECT",
                        "name": "User",
                        "ofType": None,
                    },
                },
                {
                    "name": "usersByIds",
                    "description": "Get users by a list of IDs",
                    "args": [
                        {
                            "name": "ids",
                            "description": "User IDs",
                            "type": {
                                "kind": "NON_NULL",
                                "name": None,
                                "ofType": {
                                    "kind": "LIST",
                                    "name": None,
                                    "ofType": {
                                        "kind": "NON_NULL",
                                        "name": None,
                                        "ofType": {
                                            "kind": "SCALAR",
                                            "name": "ID",
                                            "ofType": None,
                                        },
                                    },
                                },
                            },
                            "defaultValue": None,
                        }
                    ],
                    "type": {
                        "kind": "NON_NULL",
                        "name": None,
                        "ofType": {
                            "kind": "LIST",
                            "name": None,
                            "ofType": {
                                "kind": "NON_NULL",
                                "name": None,
                                "ofType": {
                                    "kind": "OBJECT",
                                    "name": "User",
                                    "ofType": None,
                                },
                            },
                        },
                    },
                },
            ],
            "inputFields": None,
            "enumValues": None,
        },
        {
            "kind": "OBJECT",
            "name": "Mutation",
            "fields": [
                {
                    "name": "createUser",
                    "description": "Create a new user",
                    "args": [
                        {
                            "name": "name",
                            "description": "User name",
                            "type": {
                                "kind": "NON_NULL",
                                "name": None,
                                "ofType": {
                                    "kind": "SCALAR",
                                    "name": "String",
                                    "ofType": None,
                                },
                            },
                            "defaultValue": None,
                        },
                        {
                            "name": "email",
                            "description": "User email",
                            "type": {
                                "kind": "NON_NULL",
                                "name": None,
                                "ofType": {
                                    "kind": "SCALAR",
                                    "name": "String",
                                    "ofType": None,
                                },
                            },
                            "defaultValue": None,
                        },
                        {
                            "name": "age",
                            "description": "User age",
                            "type": {
                                "kind": "SCALAR",
                                "name": "Int",
                                "ofType": None,
                            },
                            "defaultValue": None,
                        },
                    ],
                    "type": {
                        "kind": "OBJECT",
                        "name": "User",
                        "ofType": None,
                    },
                },
                {
                    "name": "deleteUser",
                    "description": "Delete a user by ID",
                    "args": [
                        {
                            "name": "id",
                            "description": "User ID",
                            "type": {
                                "kind": "NON_NULL",
                                "name": None,
                                "ofType": {
                                    "kind": "SCALAR",
                                    "name": "ID",
                                    "ofType": None,
                                },
                            },
                            "defaultValue": None,
                        }
                    ],
                    "type": {
                        "kind": "SCALAR",
                        "name": "Boolean",
                        "ofType": None,
                    },
                },
            ],
            "inputFields": None,
            "enumValues": None,
        },
        {
            "kind": "OBJECT",
            "name": "User",
            "fields": [
                {
                    "name": "id",
                    "description": None,
                    "args": [],
                    "type": {
                        "kind": "NON_NULL",
                        "name": None,
                        "ofType": {
                            "kind": "SCALAR",
                            "name": "ID",
                            "ofType": None,
                        },
                    },
                },
                {
                    "name": "name",
                    "description": None,
                    "args": [],
                    "type": {
                        "kind": "NON_NULL",
                        "name": None,
                        "ofType": {
                            "kind": "SCALAR",
                            "name": "String",
                            "ofType": None,
                        },
                    },
                },
                {
                    "name": "email",
                    "description": None,
                    "args": [],
                    "type": {
                        "kind": "SCALAR",
                        "name": "String",
                        "ofType": None,
                    },
                },
                {
                    "name": "age",
                    "description": None,
                    "args": [],
                    "type": {
                        "kind": "SCALAR",
                        "name": "Int",
                        "ofType": None,
                    },
                },
                {
                    "name": "status",
                    "description": None,
                    "args": [],
                    "type": {
                        "kind": "ENUM",
                        "name": "Status",
                        "ofType": None,
                    },
                },
                {
                    "name": "address",
                    "description": None,
                    "args": [],
                    "type": {
                        "kind": "OBJECT",
                        "name": "Address",
                        "ofType": None,
                    },
                },
            ],
            "inputFields": None,
            "enumValues": None,
        },
        {
            "kind": "OBJECT",
            "name": "Address",
            "fields": [
                {
                    "name": "city",
                    "description": None,
                    "args": [],
                    "type": {
                        "kind": "SCALAR",
                        "name": "String",
                        "ofType": None,
                    },
                },
                {
                    "name": "country",
                    "description": None,
                    "args": [],
                    "type": {
                        "kind": "SCALAR",
                        "name": "String",
                        "ofType": None,
                    },
                },
            ],
            "inputFields": None,
            "enumValues": None,
        },
        {
            "kind": "ENUM",
            "name": "Status",
            "fields": None,
            "inputFields": None,
            "enumValues": [
                {"name": "ACTIVE", "description": None},
                {"name": "INACTIVE", "description": None},
                {"name": "BANNED", "description": None},
            ],
        },
        {"kind": "SCALAR", "name": "ID", "fields": None, "inputFields": None, "enumValues": None},
        {"kind": "SCALAR", "name": "String", "fields": None, "inputFields": None, "enumValues": None},
        {"kind": "SCALAR", "name": "Int", "fields": None, "inputFields": None, "enumValues": None},
        {"kind": "SCALAR", "name": "Float", "fields": None, "inputFields": None, "enumValues": None},
        {"kind": "SCALAR", "name": "Boolean", "fields": None, "inputFields": None, "enumValues": None},
    ],
}

_USERS = {
    "1": {
        "id": "1",
        "name": "Alice",
        "email": "alice@example.com",
        "age": 30,
        "status": "ACTIVE",
        "address": {"city": "Portland", "country": "US"},
    },
    "2": {
        "id": "2",
        "name": "Bob",
        "email": "bob@example.com",
        "age": 25,
        "status": "INACTIVE",
        "address": {"city": "London", "country": "UK"},
    },
}
_NEXT_USER_ID = 3
_LOCK = threading.Lock()


def _extract_field_name(query: str) -> str | None:
    m = re.search(r"\{\s*(\w+)", query)
    return m.group(1) if m else None


def _select_fields(obj, query: str):
    if not isinstance(obj, dict):
        return obj
    field_name = _extract_field_name(query)
    if not field_name:
        return obj
    pattern = re.escape(field_name) + r"[^{]*\{([^}]+)\}"
    m = re.search(pattern, query)
    if not m:
        pattern2 = re.escape(field_name) + r"[^{]*\{([^}]*\{[^}]*\}[^}]*)\}"
        m = re.search(pattern2, query)
    if not m:
        return obj
    selection_text = m.group(1)
    selected = re.findall(r"\b(\w+)\b", selection_text)
    nested = re.findall(r"(\w+)\s*\{([^}]+)\}", selection_text)
    nested_names = {n[0] for n in nested}
    result = {}
    for key in selected:
        if key in obj and key not in nested_names:
            result[key] = obj[key]
    for name, inner in nested:
        if name in obj:
            if isinstance(obj[name], dict):
                inner_fields = re.findall(r"\b(\w+)\b", inner)
                result[name] = {k: v for k, v in obj[name].items() if k in inner_fields}
            else:
                result[name] = obj[name]
    return result


class GraphQLHandler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):  # noqa: A003
        pass

    def _send_json(self, data, status=200):
        body = json.dumps(data).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _read_body(self) -> dict:
        length = int(self.headers.get("Content-Length", 0))
        if length:
            return json.loads(self.rfile.read(length))
        return {}

    def do_POST(self):  # noqa: N802
        global _NEXT_USER_ID
        body = self._read_body()
        query = body.get("query", "")
        variables = body.get("variables") or {}

        if "__schema" in query:
            self._send_json({"data": {"__schema": GRAPHQL_SCHEMA}})
            return

        field_name = _extract_field_name(query)
        with _LOCK:
            if field_name == "users":
                users = [_select_fields(u, query) for u in _USERS.values()]
                self._send_json({"data": {"users": users}})
                return
            if field_name == "user":
                uid = variables.get("id", "")
                user = _USERS.get(str(uid))
                if user:
                    self._send_json({"data": {"user": _select_fields(user, query)}})
                else:
                    self._send_json({"data": {"user": None}})
                return
            if field_name == "usersByIds":
                ids = variables.get("ids", [])
                users = [_USERS[str(i)] for i in ids if str(i) in _USERS]
                users = [_select_fields(u, query) for u in users]
                self._send_json({"data": {"usersByIds": users}})
                return
            if field_name == "createUser":
                user = {
                    "id": str(_NEXT_USER_ID),
                    "name": variables.get("name", ""),
                    "email": variables.get("email", ""),
                    "age": variables.get("age"),
                    "status": "ACTIVE",
                    "address": None,
                }
                _USERS[str(_NEXT_USER_ID)] = user
                _NEXT_USER_ID += 1
                self._send_json({"data": {"createUser": _select_fields(user, query)}})
                return
            if field_name == "deleteUser":
                uid = variables.get("id", "")
                deleted = str(uid) in _USERS
                if deleted:
                    del _USERS[str(uid)]
                self._send_json({"data": {"deleteUser": deleted}})
                return

        self._send_json({"errors": [{"message": f"Unknown field: {field_name}"}]})


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def main() -> None:
    port = find_free_port()
    server = HTTPServer(("127.0.0.1", port), GraphQLHandler)
    print(f"PORT={port}", flush=True)
    print(f"URL=http://127.0.0.1:{port}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        sys.exit(0)


if __name__ == "__main__":
    main()
