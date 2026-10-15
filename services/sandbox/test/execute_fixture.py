import json
import sys

sys.path.insert(0, "src")
from execute import execute

python = '"' + sys.executable + '"'
print(json.dumps([
    execute("echo hello"),
    execute("exit 7"),
    execute(python + ' -c "print(chr(120)*100000)"', limit=1024),
    execute(python + ' -c "import time;time.sleep(5)"', timeout=0.2),
]))
