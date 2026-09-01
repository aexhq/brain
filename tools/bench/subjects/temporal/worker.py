"""The agent-session worker this benchmark runs on Temporal, plus the HTTP shim the
runner drives.

This is harness code on Temporal's substrate — Temporal ships no agent-session server —
mirroring the pattern of their own openai_agents/customer_service sample: one
long-running workflow per session, messages arriving as workflow updates whose result is
the reply, the model call running as an activity through their official
OpenAIAgentsPlugin. The plugin's model provider points at the scripted provider
(chat-completions dialect, not the Responses API).

The shim is a stdlib HTTP server hopping each request onto the client's event loop; its
cost is a fraction of a millisecond and it is our code, which the manifest says.
"""

import asyncio
import json
import os
import threading
import time
from datetime import timedelta
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from agents import Agent, Runner
from agents.models.openai_provider import OpenAIProvider
from openai import AsyncOpenAI
from temporalio import workflow
from temporalio.client import Client
from temporalio.contrib.openai_agents import ModelActivityParameters, OpenAIAgentsPlugin
from temporalio.worker import Worker

TASK_QUEUE = "bench"


@workflow.defn
class SessionWorkflow:
    def __init__(self) -> None:
        self.history: list = []

    @workflow.run
    async def run(self) -> None:
        await workflow.wait_condition(
            lambda: workflow.info().is_continue_as_new_suggested()
            and workflow.all_handlers_finished()
        )
        workflow.continue_as_new()

    @workflow.update
    async def send_message(self, text: str) -> str:
        agent = Agent(
            name="bench",
            instructions="You are a benchmark assistant.",
            model="gpt-4o-mini",
        )
        self.history.append({"role": "user", "content": text})
        result = await Runner.run(agent, self.history)
        self.history = result.to_input_list()
        return result.final_output or ""


async def main() -> None:
    plugin = OpenAIAgentsPlugin(
        model_params=ModelActivityParameters(
            start_to_close_timeout=timedelta(seconds=30)
        ),
        model_provider=OpenAIProvider(
            openai_client=AsyncOpenAI(
                base_url=os.environ["BENCH_MODEL_BASE_URL"], api_key="bench"
            ),
            use_responses=False,
        ),
    )
    # The dev server boots beside this process; wait it out rather than racing it.
    client = None
    for _ in range(60):
        try:
            client = await Client.connect("127.0.0.1:7233", plugins=[plugin])
            break
        except Exception:  # noqa: BLE001 — connection refused while the server boots
            await asyncio.sleep(0.5)
    if client is None:
        raise RuntimeError("the Temporal dev server never became reachable")
    loop = asyncio.get_running_loop()

    class Shim(BaseHTTPRequestHandler):
        def log_message(self, *args) -> None:
            pass

        def reply(self, code: int, payload: dict) -> None:
            body = json.dumps(payload).encode()
            self.send_response(code)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self) -> None:
            self.reply(200, {"status": "ok"})

        def do_POST(self) -> None:
            length = int(self.headers.get("content-length", "0"))
            request = json.loads(self.rfile.read(length) or b"{}")
            try:
                if self.path == "/create":
                    session_id = f"bench-{time.time_ns():x}"
                    future = asyncio.run_coroutine_threadsafe(
                        client.start_workflow(
                            SessionWorkflow.run, id=session_id, task_queue=TASK_QUEUE
                        ),
                        loop,
                    )
                    future.result(timeout=30)
                    return self.reply(200, {"id": session_id})
                if self.path == "/send":
                    future = asyncio.run_coroutine_threadsafe(
                        send(request["id"], request["message"]), loop
                    )
                    return self.reply(200, {"text": future.result(timeout=30)})
                return self.reply(404, {"error": f"no route {self.path}"})
            except Exception as error:  # noqa: BLE001 — the driver reads the body
                return self.reply(500, {"error": str(error)})

    async def send(session_id: str, message: str) -> str:
        handle = client.get_workflow_handle_for(SessionWorkflow.run, session_id)
        return await handle.execute_update(SessionWorkflow.send_message, message)

    server = ThreadingHTTPServer(
        ("127.0.0.1", int(os.environ["BENCH_PORT"])), Shim
    )
    threading.Thread(target=server.serve_forever, daemon=True).start()

    worker = Worker(client, task_queue=TASK_QUEUE, workflows=[SessionWorkflow])
    await worker.run()


if __name__ == "__main__":
    asyncio.run(main())
