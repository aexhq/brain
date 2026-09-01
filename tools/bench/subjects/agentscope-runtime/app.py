"""The AgentScope Runtime AgentApp this benchmark measures.

One ReActAgent per request at the cookbook's defaults, its model pointed at the scripted
provider (OpenAIChatModel forwards client_kwargs to openai.AsyncClient, so base_url lands
there), session state in JSONSession files under the run's data directory — the offline,
restart-surviving option among the session backends the 1.x line ships.

Pinned to agentscope-runtime==1.1.6.post2 with agentscope==1.0.21: the runtime's last
release before the repository was archived into AgentScope 2.0, and the last 1.x
agentscope its documented API imports from. Later agentscope releases delete
agentscope.session and agentscope.pipeline outright.
"""

import os
from contextlib import asynccontextmanager

from agentscope.agent import ReActAgent
from agentscope.formatter import OpenAIChatFormatter
from agentscope.memory import InMemoryMemory
from agentscope.model import OpenAIChatModel
from agentscope.pipeline import stream_printing_messages
from agentscope.session import JSONSession
from fastapi import FastAPI

from agentscope_runtime.engine import AgentApp
from agentscope_runtime.engine.schemas.agent_schemas import AgentRequest


@asynccontextmanager
async def lifespan(app: FastAPI):
    app.state.session = JSONSession(save_dir=os.environ["BENCH_DATA_DIR"])
    yield


agent_app = AgentApp(app_name="Bench", app_description="bench agent", lifespan=lifespan)


@agent_app.query(framework="agentscope")
async def query_func(self, msgs, request: AgentRequest = None, **kwargs):
    agent = ReActAgent(
        name="Bench",
        model=OpenAIChatModel(
            # The model the scripted provider advertises; the name travels to the fixture.
            model_name="gpt-4o-mini",
            api_key="bench",
            stream=True,
            client_kwargs={"base_url": os.environ["BENCH_MODEL_BASE_URL"]},
        ),
        sys_prompt="You are a benchmark assistant.",
        memory=InMemoryMemory(),
        formatter=OpenAIChatFormatter(),
    )
    agent.set_console_output_enabled(enabled=False)
    await agent_app.state.session.load_session_state(
        session_id=request.session_id, user_id=request.user_id, agent=agent
    )
    async for msg, last in stream_printing_messages(agents=[agent], coroutine_task=agent(msgs)):
        yield msg, last
    await agent_app.state.session.save_session_state(
        session_id=request.session_id, user_id=request.user_id, agent=agent
    )


if __name__ == "__main__":
    agent_app.run(host="127.0.0.1", port=int(os.environ["BENCH_PORT"]))
