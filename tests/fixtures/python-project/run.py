import json
from pathlib import Path
import sys
import six

assert Path(".venv/prepared").read_text() == six.__version__
print(json.dumps({"echo": json.load(sys.stdin), "dependency": six.__version__}))
