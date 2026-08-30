"""The graph LangGraph Server runs for the benchmark.

The smallest thing that is a fair analogue of Brain's agentloop: one node that calls the
model and appends the reply. Anything more would be measuring a graph we designed rather
than the server running it, and anything less would not be a turn.

We wrote this graph, and any published number has to say so — the same caveat the
benchmark's own README applies to every framework subject.
"""

import os

from langchain_openai import ChatOpenAI
from langgraph.graph import END, START, MessagesState, StateGraph

model = ChatOpenAI(
    model=os.environ.get("BENCH_MODEL", "gpt-4o-mini"),
    base_url=os.environ["OPENAI_BASE_URL"],
    api_key=os.environ.get("OPENAI_API_KEY", "bench"),
    timeout=30,
    max_retries=0,
)


def call_model(state: MessagesState):
    return {"messages": [model.invoke(state["messages"])]}


builder = StateGraph(MessagesState)
builder.add_node("agent", call_model)
builder.add_edge(START, "agent")
builder.add_edge("agent", END)
graph = builder.compile()
