"""The keychain from the command line, for `ekos setup` (RFC 0185 §3).

    python -m app.keychain_cli put env OPENAI_API_KEY      < value
    python -m app.keychain_cli put console oidc_client_secret < value
    python -m app.keychain_cli status

The value is read from standard input, never from an argument (arguments are visible to other
users of the machine). The same checks as the console's routes apply. Exit codes: 0 done, 2
refused, 3 no keychain.
"""

from __future__ import annotations

import json
import sys

from . import console_file, envs, keychain


def main(argv: list[str]) -> int:
    if argv == ["status"]:
        usable, backend = keychain.available()
        names = keychain._index() if usable else {"env": [], "console": []}
        print(json.dumps({"available": usable, "backend": backend, **names}))
        return 0
    if len(argv) != 3 or argv[0] != "put" or argv[1] not in ("env", "console"):
        print(__doc__, file=sys.stderr)
        return 2
    _, kind, name = argv
    if kind == "env" and not envs.is_valid_name(name):
        print(f"{name!r} is not a variable a project may be given", file=sys.stderr)
        return 2
    if kind == "console" and name not in console_file.SECRETS:
        print(f"{name!r} is not a secret console setting", file=sys.stderr)
        return 2
    value = sys.stdin.read().strip()
    if not value:
        print("no value on standard input", file=sys.stderr)
        return 2
    try:
        (keychain.put_env if kind == "env" else keychain.put_console)(name, value)
    except keychain.Unavailable as exc:
        print(str(exc), file=sys.stderr)
        return 3
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
