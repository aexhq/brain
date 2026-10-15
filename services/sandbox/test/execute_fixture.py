import json
import os
import sys

sys.path.insert(0, "src")
from execute import execute

python = '"' + sys.executable + '"'
if os.name == "posix":
    assert execute("exec 1>&- 2>&-; sleep 5", timeout=0.2)["timed_out"]
print(json.dumps([
    execute("echo hello"),
    execute("exit 7"),
    execute(python + ' -c "print(chr(120)*100000)"', limit=1024),
    execute(python + ' -c "import time;time.sleep(5)"', timeout=0.2),
]))
