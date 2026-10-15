import os

import tomli


def main(arguments, context):
    read_only = False
    try:
        with open("/work/should-not-exist", "w", encoding="utf-8") as output:
            output.write("forbidden")
    except OSError as error:
        read_only = error.errno in (13, 30)
    return {
        "dependency": tomli.loads("value = " + arguments["value"])["value"] == 12,
        "secret_injected": os.environ.get("PACKAGE_SECRET") == "journey-scoped-secret",
        "internal_key_absent": "BRAIN_WORKER_KEY" not in os.environ,
        "read_only": read_only,
        "uid": os.getuid(),
        "operation_id": context["operation_id"],
    }
