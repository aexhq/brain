from pathlib import Path
import six

with Path(".venv/prepared").open("x") as marker:
    marker.write(six.__version__)
