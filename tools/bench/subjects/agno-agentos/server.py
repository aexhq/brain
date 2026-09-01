"""The AgentOS instance this benchmark measures.

One agent at Agno's defaults, its model pointed at the scripted provider. The launch
environment carries the wiring: AGENT_OS_PORT (which overrides serve() arguments, so it
is the only reliable way to hand AgentOS the per-run port), BENCH_MODEL_BASE_URL, and
BENCH_DATA_DIR for the SQLite file. AGNO_TELEMETRY=false is set in the manifest and
telemetry=False here as well, because the env var alone has not always been honoured.
"""

import os

from agno.agent import Agent
from agno.db.sqlite import SqliteDb
from agno.models.openai.like import OpenAILike
from agno.os import AgentOS

agent = Agent(
    id="bench-agent",
    name="Bench Agent",
    model=OpenAILike(
        # The model the scripted provider advertises; the name travels to the fixture.
        id="gpt-4o-mini",
        base_url=os.environ["BENCH_MODEL_BASE_URL"],
        api_key="bench",
    ),
    db=SqliteDb(db_file=os.path.join(os.environ["BENCH_DATA_DIR"], "agentos.db")),
    add_history_to_context=True,
    telemetry=False,
)

agent_os = AgentOS(agents=[agent], telemetry=False)
app = agent_os.get_app()

if __name__ == "__main__":
    import uvicorn

    uvicorn.run(
        app,
        host="127.0.0.1",
        port=int(os.environ["AGENT_OS_PORT"]),
        access_log=False,
        log_level="warning",
    )
