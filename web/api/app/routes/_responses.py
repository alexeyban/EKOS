"""OpenAPI descriptions for the error statuses the console's routes raise.

Each route lists the statuses it can return in its decorator's `responses=`, so the generated
OpenAPI schema (and the typed client `npm run gen:api` builds from it) documents the failures a
caller has to handle, not only the success shape.
"""

from __future__ import annotations

BAD_REQUEST = {"description": "The request is not valid in the current configuration"}
UNAUTHORIZED = {"description": "Not signed in, or the token is invalid"}
NOT_FOUND = {"description": "No such workspace, object, run, schedule or report"}
CONFLICT = {"description": "Conflicts with existing state"}
UNPROCESSABLE = {"description": "Parameters, configuration or trigger failed validation"}
TOO_MANY = {"description": "The run queue is full"}
SERVER_ERROR = {"description": "A stored file is corrupt"}
BAD_GATEWAY = {"description": "The ekos CLI or MCP server call failed"}
