<pre align="center">
              ______ ______ _______ _______ _______
  ▄████▄     |   __ \   __ \   _   |_     _|    |  |
▄██▄██▄██▄   |   __ <      <       |_|   |_|       |
  ▀▀  ▀▀     |______/___|__|___|___|_______|__|____|
</pre>

<p align="center"><strong>极简、分布式、可扩展的 agent 运行时。</strong></p>

<p align="center">
  <a href="https://github.com/aexhq/brain/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/aexhq/brain/actions/workflows/ci.yml/badge.svg" /></a>
  <a href="https://www.npmjs.com/package/@aexhq/brain"><img alt="npm" src="https://img.shields.io/npm/v/%40aexhq%2Fbrain?label=%40aexhq%2Fbrain" /></a>
  <img alt="Rust" src="https://img.shields.io/badge/rust-1.97%2B-orange" />
</p>

<p align="center">
  <a href="https://aex.dev/brain/docs"><strong>文档</strong></a> ·
  <a href="https://aex.dev/brain/docs/reference/api">API 参考</a> ·
  <a href="https://aex.dev/brain">官网</a> ·
  <a href="https://github.com/aexhq/extensions">官方扩展</a> ·
  <a href="ROADMAP.md">路线图</a> ·
  <a href="README.md">English</a>
</p>

> [!NOTE]
> **早期预览。** 在发布 1.0.0 之前，API 和功能可能随时变更，不保证向后兼容，也不另行通知。

## 这是什么

**Brain** 是一个独立、极简、分布式、可扩展的 agent 运行时。应用通过小型公共接口组合 Agentloop、
模型、工具和环境。同一会话可以调用多个环境中的工具；应用自行提供产品策略、调度和基础设施。

- `component(urlOrBytes)` 包装已经编译好的 WebAssembly Component。Brain 接收原始 Wasm，
  不编译应用源码。
- `brainWasm(options)` 是 Brain 内置的 Wasmtime 环境；网络、密钥以及可写的临时目录或会话工作区
  都必须由会话请求并由服务器部署显式授权，默认均不授权。
- 每次原生调用最多使用一百亿个 Wasmtime fuel 单位执行 guest 代码；挂起的 I/O 不消耗 fuel，
  session 的墙钟时间限制仍约束整个 turn。
- Agentloop 用 `agentloop({ implementation })( { env, ...options } )` 声明和放置。
- 带 `run` 的工具常驻应用进程，用 `tool({ ... , run })()` 实例化。
- 带 `implementation` 的工具由环境执行，用 `tool({ ... , implementation })( { env, ...options } )`
  显式放置。

## 持久化规则

每个会话只有一份规范日志。会话状态、公共事件、对话记录和 Agentloop slots 都是它的投影。
Brain 在发送外部副作用之前先把意图持久化提交，只发送一次，绝不自动重试。已知结果、已知失败或
未知结果都会在返回 Agentloop 之前提交。替换现有对话尾部的规范记录同时投影为
`transcript_replaced` 事件；纯追加不产生重复事件。

常驻工具通过一条注册主机 SSE 连接接收命令。`ctx.emit(kind, data)` 把扩展事件提交到同一份日志，
Promise 在提交完成后才返回。

## 架构

Agentloop 控制上下文，决定何时调用模型或工具；Brain 协调执行并记录结果。
同一会话可以使用原生工具、应用中的函数，以及多个远程环境中的工具。

```mermaid
flowchart LR
  subgraph App["你的应用"]
    Client["SDK / HTTP 客户端"]
    Resident["常驻工具"]
  end

  subgraph Brain["Brain 运行时"]
    Server["HTTP / SSE 服务器<br/>会话协调"]
    Journal[("本地日志<br/>对话、slots 和事件")]
    subgraph Worker["brainWasm · Wasmtime worker"]
      Loop["Agentloop Component"]
      Native["原生工具 Component"]
    end
    Server <-->|"提交 / 读取"| Journal
    Server <-->|"激活 / host 调用"| Loop
    Server <-->|"调用 / 结果"| Native
  end

  Client <-->|"HTTP / SSE"| Server
  Resident <-->|"host SSE / 结果"| Server
  Server <-->|"模型调用"| Models["模型提供商"]
  Server <-->|"环境协议"| EnvA["环境 A<br/>工具与资源"]
  Server <-->|"环境协议"| EnvB["环境 B<br/>工具与资源"]
```

默认每轮结束后释放执行资源，对话与已记录事件仍可读取。环境提供方独立管理资源分配、TTL 和清理，
会话挂起不会自动销毁或恢复环境资源。

Brain 是基于 [Tokio](https://tokio.rs/) 的原生 Rust 二进制文件，用
[Axum](https://github.com/tokio-rs/axum) 提供 HTTP 和 SSE API，本地部署无需外部存储。

## 快速开始

运行服务器：

```sh
docker run --rm -p 127.0.0.1:8080:8080 \
  -e BRAIN_LISTEN=0.0.0.0:8080 -e BRAIN_API_TOKEN=quickstart \
  -v brain-data:/var/lib/brain ghcr.io/aexhq/brain:latest
```

```sh
npm install @aexhq/brain @aexhq/agentloop-pi zod
```

```js
import { Brain, brainWasm, tool } from "@aexhq/brain";
import { pi } from "@aexhq/agentloop-pi";
import { z } from "zod";

const orders = { "A-1001": { status: "shipped", eta: "Thursday" } };
const lookupOrder = tool({
  name: "lookup_order",
  description: "Look up an order's status by id.",
  input: z.object({ id: z.string() }),
  run: async ({ id }, ctx) => {
    await ctx.emit("order_lookup_started", { id });
    return orders[id] ?? { status: "unknown order" };
  },
});

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080", token: "quickstart" });
const session = await brain.sessions.create({
  model: { provider: "openai", name: "gpt-5-mini", apiKey: process.env.OPENAI_API_KEY },
  agentloop: pi({ env: brainWasm() }),
  tools: [lookupOrder()],
});

await session.send("Where is order A-1001?");
for await (const event of session.events()) console.log(event.sequence, event.type);
await session.end();
await session.delete();
process.exit(0);
```

自定义 Agentloop 直接提供 Component：

```js
import { agentloop, brainWasm, component } from "@aexhq/brain";

const custom = agentloop({
  implementation: component(new URL("./agentloop.wasm", import.meta.url)),
});
const bound = custom({ env: brainWasm() });
```

## 性能与生命周期

默认每轮结束后释放会话执行状态，重启时按需读取会话，不扫描所有对话日志。可重建的检查点加快恢复；
已编译并预链接的 Wasm 模板跨调用复用，每次调用使用新的实例。原生工具拥有独立于 Agentloop 的并发容量。
历史对比数据见 [BENCHMARKS.md](BENCHMARKS.md)，不能代表当前实现的性能承诺。

## 联系方式

支持与 bug 反馈请提交 [issue](https://github.com/aexhq/brain/issues) 或写信至
[support@aex.dev](mailto:support@aex.dev)。合作与商务事宜请写信至
[admin@aex.dev](mailto:admin@aex.dev)。
