<pre align="center">
              ______ ______ _______ _______ _______
  ▄████▄     |   __ \   __ \   _   |_     _|    |  |
▄██▄██▄██▄   |   __ <      <       |_|   |_|       |
  ▀▀  ▀▀     |______/___|__|___|___|_______|__|____|
</pre>

<p align="center"><strong>极简、极速、可扩展的 agent 运行时。</strong></p>

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
  <a href="README.md">English</a>
</p>

> [!NOTE]
> **早期预览。** 在发布 1.0.0 之前，API 和功能可能随时变更，不保证向后兼容，也不另行通知。

## 这是什么

**Brain** 是一个持久化 agent 会话内核。Agentloop 决定一轮如何进行；Brain 管理唯一的会话日志、
模型调用、工具分发、取消和结果。应用显式提供模型、Agentloop、工具与环境。

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

## 工作原理

```text
应用 <------ HTTP / SSE ------> Brain
                              |
                              +-- 每个会话一份规范日志
                              +-- 模型提供商
                              +-- Agentloop 环境
                              +-- 工具环境
                              `-- 常驻应用主机（SSE）
```

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

## 基准测试

所有数字都不包含模型延迟。每张图中 ★ 标记的是 Brain。

**单轮往返**

```text
pi         █                                       5.1 ms
Brain      ████████████                             40 ms ★
Codex      █████████████                            47 ms
ZeroClaw   ██████████████                           53 ms
OpenFang   ██████████████████                      128 ms
OpenCode   ███████████████████                     155 ms
AgentScope ████████████████████████                338 ms
Letta      ███████████████████████████             678 ms
LangGraph  ███████████████████████████████         1.22 s
Awaken     ██████████████████████████████████      2.23 s
OpenClaw   ████████████████████████████████████    3.33 s
```

**首个 token 时间**

```text
Brain      █                                       2.9 ms ★
pi         █████                                   6.3 ms
ZeroClaw   ████████                                 11 ms
Codex      ███████████████                          39 ms
OpenFang   ██████████████████                       70 ms
OpenCode   ████████████████████                     99 ms
LangGraph  ████████████████████████                207 ms
AgentScope ███████████████████████████             332 ms
Letta      ████████████████████████████            407 ms
OpenClaw   ██████████████████████████████████      1.33 s
Awaken     ████████████████████████████████████    1.93 s
```

**新建会话**

```text
LangGraph  █                                      0.66 ms
Brain      ██                                     0.76 ms ★
OpenCode   ██████████                              2.1 ms
ZeroClaw   ██████████                              2.2 ms
Awaken     ████████████████                        5.1 ms
OpenClaw   █████████████████                       5.4 ms
pi         ███████████████████                     7.0 ms
OpenFang   █████████████████████                   9.8 ms
Codex      ████████████████████████                 14 ms
Letta      ████████████████████████████████████     67 ms
```

**每个空闲会话的内存**

```text
Brain      █                                        14 KiB ★
OpenFang   ██████████████                          0.6 MiB
ZeroClaw   ████████████████████████████             50 MiB
OpenClaw   ████████████████████████████████████    490 MiB
```

<sub>每个数字都是 <a href="tools/bench">tools/bench</a> 中的测试工具在同一台 AWS
<code>c7g.xlarge</code> 上对每个对象测得的中位数。柱状图使用对数刻度。
<a href="BENCHMARKS.md">BENCHMARKS.md</a> 记录方法和版本。</sub>

## 路线图

- [x] 四部分：Agentloop、模型、工具、环境
- [x] 原始 Wasm Component 与显式 `{ env, ...options }` 放置
- [x] 一份规范会话日志和可重建投影
- [x] 先提交后执行，不自动重试
- [x] 常驻工具 SSE 主机与持久化 `ctx.emit`
- [x] HTTP/SSE 会话 API 和 `@aexhq/brain` SDK
- [x] 跨会话原生工作区隔离测试
- [ ] 原生子 agent 支持
- [ ] 多模态输入
- [ ] 冻结 v1 API 并发布带标签版本
- [ ] 会话跨机器分片并共享环境
- [ ] 外部 SessionStore、备份和区域恢复

## 联系方式

支持与 bug 反馈请提交 [issue](https://github.com/aexhq/brain/issues) 或写信至
[support@aex.dev](mailto:support@aex.dev)。合作与商务事宜请写信至
[admin@aex.dev](mailto:admin@aex.dev)。
