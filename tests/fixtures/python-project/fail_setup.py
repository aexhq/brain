from pathlib import Path
import six

with Path(".venv/setup_attempts").open("a") as attempts:
    attempts.write("attempt\n")
raise RuntimeError("fixture setup failure")
