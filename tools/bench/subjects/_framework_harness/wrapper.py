"""A minimal HTTP surface over a framework that has none.

The `framework` subjects are libraries you build an agent *with*, not servers that hold
sessions, so they answer no probe until someone writes a harness around them. This is that
harness, and any number published from it has to say we wrote it — the benchmark's own
rule.

It is deliberately the smallest thing that can be measured: create a unit, run a turn,
and let the framework persist however it persists. What is compared is the *storage
model* — bytes written per turn against turn index — not anything about the code here.
Every framework gets the same two endpoints and the same input, built in one place here,
so the only thing that differs between them is what they choose to write.

    POST /units              -> {"id": "..."}
    POST /units/{id}/turns   -> {"ok": true}
    GET  /ok                 -> readiness

Which framework is loaded comes from BENCH_FRAMEWORK. The data directory it must write
into comes from BENCH_DATA_DIR, so the runner can watch it grow.
"""

import asyncio
import collections
import inspect
import json
import os
import threading
import uuid

from fastapi import FastAPI, HTTPException

FRAMEWORK = os.environ.get("BENCH_FRAMEWORK", "langgraph")
DATA_DIR = os.environ.get("BENCH_DATA_DIR", "/tmp/bench-framework")
MODEL_BASE_URL = os.environ["OPENAI_BASE_URL"]
MODEL = os.environ.get("BENCH_MODEL", "gpt-4o-mini")
TURN_INPUT = "benchmark"

os.makedirs(DATA_DIR, exist_ok=True)
app = FastAPI()

# Some of these frameworks are async-only. One loop on one background thread runs their
# turns, because a fresh loop per turn would strand the HTTP clients they hold open
# between calls and measure connection setup instead of what they wrote.
_loop = asyncio.new_event_loop()
threading.Thread(target=_loop.run_forever, daemon=True).start()


def settle(result):
    """Runs a backend's call to completion, whether that backend is sync or async."""
    if inspect.isawaitable(result):
        return asyncio.run_coroutine_threadsafe(result, _loop).result()
    return result


_turns = collections.Counter()


def turn_input(unit: str) -> str:
    """What every backend is asked on this turn — the same string for all of them.

    Carrying the turn index is not decoration. A conversation whose every turn is
    byte-identical is not a conversation, and at least one of these frameworks hashes
    messages and drops a turn that repeats what it already holds: with a constant input
    Microsoft Agent Framework stored two messages and nothing after them, which measures
    its de-duplication rather than its storage model. Building the string here rather than
    in each backend is what keeps the frameworks comparable.
    """
    _turns[unit] += 1
    return f"{TURN_INPUT} {_turns[unit]}"


class LangGraphBackend:
    """LangGraph with the SQLite checkpointer: a full checkpoint per super-step."""

    def __init__(self):
        from langchain_openai import ChatOpenAI
        from langgraph.checkpoint.sqlite import SqliteSaver
        from langgraph.graph import END, START, MessagesState, StateGraph

        self._saver_cm = SqliteSaver.from_conn_string(os.path.join(DATA_DIR, "checkpoints.sqlite"))
        self.saver = self._saver_cm.__enter__()
        model = ChatOpenAI(model=MODEL, base_url=MODEL_BASE_URL, api_key="bench", max_retries=0)

        def call_model(state: MessagesState):
            return {"messages": [model.invoke(state["messages"])]}

        builder = StateGraph(MessagesState)
        builder.add_node("agent", call_model)
        builder.add_edge(START, "agent")
        builder.add_edge("agent", END)
        self.graph = builder.compile(checkpointer=self.saver)

    def create(self) -> str:
        return str(uuid.uuid4())

    def turn(self, unit: str, message: str) -> None:
        self.graph.invoke(
            {"messages": [{"role": "user", "content": message}]},
            config={"configurable": {"thread_id": unit}},
        )


class CrewAIBackend:
    """CrewAI with memory on: its own LanceDB store, wherever it decides to put it.

    The harness sets CREWAI_STORAGE_DIR and nothing else about storage, so the layout
    below that root — table files, fragments, compaction — is entirely CrewAI's.
    """

    def __init__(self):
        from crewai import LLM, Agent, Crew, Task

        os.environ["CREWAI_STORAGE_DIR"] = DATA_DIR
        llm = LLM(model=f"openai/{MODEL}", base_url=MODEL_BASE_URL, api_key="bench")
        agent = Agent(role="bench", goal=TURN_INPUT, backstory=TURN_INPUT, llm=llm)
        # CrewAI interpolates `{...}` in a task description from the kickoff inputs, so
        # the task *is* the turn's message and nothing else.
        task = Task(description="{message}", expected_output=TURN_INPUT, agent=agent)
        self.crew = Crew(
            agents=[agent],
            tasks=[task],
            memory=True,
            # Pointed at the same scripted endpoint as the chat model. Left at its default
            # it would embed against api.openai.com, which is neither reachable from the
            # benchmark host nor the same input every other subject gets.
            embedder={
                "provider": "openai",
                "config": {
                    "model_name": "text-embedding-3-small",
                    "api_base": MODEL_BASE_URL,
                    "api_key": "bench",
                },
            },
        )

    def create(self) -> str:
        # CrewAI's unit of work is the crew itself, kicked off again per turn; the id is
        # what the driver tracks, not something the framework needs.
        return str(uuid.uuid4())

    def turn(self, unit: str, message: str) -> None:
        self.crew.kickoff(inputs={"message": message})


class AutoGenBackend:
    """AutoGen AgentChat, which ships no durable store at all.

    `save_state()` hands back the whole conversation as a dict and AutoGen's own state
    documentation says to write that to a file or a database — so that is what happens
    here, one `json.dump` per turn over one file. The shape of the curve is AutoGen's
    serialization; the write itself is the harness's, and the manifest says so.
    """

    def __init__(self):
        from autogen_ext.models.openai import OpenAIChatCompletionClient

        self.client = OpenAIChatCompletionClient(
            model=MODEL, base_url=MODEL_BASE_URL, api_key="bench"
        )
        self.agents = {}

    def create(self) -> str:
        from autogen_agentchat.agents import AssistantAgent

        unit = str(uuid.uuid4())
        self.agents[unit] = AssistantAgent(name="bench", model_client=self.client)
        return unit

    async def turn(self, unit: str, message: str) -> None:
        agent = self.agents[unit]
        await agent.run(task=message)
        state = await agent.save_state()
        with open(os.path.join(DATA_DIR, f"{unit}.json"), "w") as handle:
            json.dump(state, handle)


class AgentFrameworkBackend:
    """Microsoft Agent Framework behind FileHistoryProvider.

    The one durable conversation store shipped in `agent-framework-core`: one
    append-only JSONL file per session. Its sibling `FileSessionStore` writes a whole
    session snapshot per turn instead, which is a different storage model and would give
    a different curve; the manifest records that this is the appending one.
    """

    def __init__(self):
        from agent_framework import FileHistoryProvider
        from agent_framework.openai import OpenAIChatCompletionClient

        # Not `OpenAIChatClient`, which speaks the Responses API: the scripted provider
        # every other subject is wired to serves `/chat/completions`, and this is the
        # framework's own client for that endpoint. Same fixture, same fixed input.
        client = OpenAIChatCompletionClient(
            model=MODEL, base_url=MODEL_BASE_URL, api_key="bench"
        )
        self.agent = client.as_agent(
            name="bench", context_providers=[FileHistoryProvider(DATA_DIR)]
        )
        self.sessions = {}

    def create(self) -> str:
        from agent_framework import AgentSession

        unit = str(uuid.uuid4())
        self.sessions[unit] = AgentSession(session_id=unit)
        return unit

    async def turn(self, unit: str, message: str) -> None:
        await self.agent.run(message, session=self.sessions[unit])


BACKENDS = {
    "langgraph": LangGraphBackend,
    "crewai": CrewAIBackend,
    "autogen": AutoGenBackend,
    "microsoft-agent-framework": AgentFrameworkBackend,
}


def load():
    factory = BACKENDS.get(FRAMEWORK)
    if factory is None:
        raise SystemExit(
            f"no backend for {FRAMEWORK!r}; add one to BACKENDS rather than measuring "
            f"a framework this harness does not actually drive"
        )
    return factory()


backend = load()


@app.get("/ok")
def ok():
    return {"ok": True, "framework": FRAMEWORK}


@app.post("/units")
def create_unit():
    return {"id": settle(backend.create())}


@app.post("/units/{unit}/turns")
def run_turn(unit: str):
    try:
        settle(backend.turn(unit, turn_input(unit)))
    except Exception as error:  # surfaced to the driver, which reports it as a failure
        raise HTTPException(status_code=500, detail=str(error)) from error
    return {"ok": True}
