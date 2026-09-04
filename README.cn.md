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

## 这是什么

**Brain** 是一个极简、*极速*、可扩展的 agent 运行时服务器。你编写 agent loop 和工具，
Brain 负责运行会话。工具可以在任何地方运行，从浏览器标签页到服务器沙箱。Agent loop 运行在
Wasm 沙箱中，因此运行时天生安全。每个会话只占用极少内存，每一步都是可以实时观察的事件。

> [!NOTE]
> **早期预览。** 在发布 1.0.0 之前，API 和功能可能随时变更，不保证向后兼容，也不另行通知。

## 特性

- **工具在任何地方运行。** 工具就是一个带类型的函数。它可以运行在你自己的进程里、microVM
  沙箱里、浏览器页面里，或者你的后端上，而且同一个会话可以同时使用其中几种。
- **开销极低。** 会话状态保存在内存中，日志在一轮结束后再写入。一个空闲会话约占用 14 KiB，
  一次往返耗时 40 ms。CI 在每次构建时都会检查这些数字。
- **自带模型，派生子 agent。** Brain 通过 [models.dev](https://models.dev) 内置了 70 多个 LLM
  提供商的绑定。模型按会话固定，一个会话可以创建其他会话来完成子 agent 的工作。
- **隔离的 agent loop。** Agent loop 编译为 WebAssembly，在自己的沙箱中运行。Brain 代它执行
  I/O，因此每个决策都是确定性的、可重放的。
- **端到端可观测。** 每一次观察、决策、模型调用和工具结果都是一个事件。你可以实时流式查看，
  也可以事后回读。

## 扩展

Brain 拥有会话。其他一切都是扩展。你用 `@aexhq/brain` SDK 编写扩展，运行 `npx brain build`，
然后把生成的工厂传给会话。扩展有三种，每一种都是一个小巧的带类型声明。

- **Agent loop** 决定接下来发生什么。它为每种观察注册一个同步处理器，每个处理器返回一个动作：
  调用模型、运行工具、回复或停止。你用 TypeScript 编写它。`npx brain build` 把它编译为
  WebAssembly 组件，Brain 在 [Wasmtime](https://wasmtime.dev/) 沙箱中运行该组件，沙箱中没有
  文件系统、网络、时钟和密钥。Loop 请求的每个副作用都由 Brain 执行，因此每个决策都是确定性的，
  可以从日志重放。
  [编写 agent loop](https://aex.dev/brain/docs/guides/write-a-loop)
- **工具** 负责干活。它声明输入输出的 schema 以及所操作的资源（`fs`、`process`、`net`、`dom`、
  `secrets`）。内部就是运行平台上的普通代码。如果环境没有声明工具所需的资源，Brain 会在创建
  会话时直接拒绝。只有一条 shell 命令或一次 HTTP 请求的工具完全不需要写代码。
  [编写工具](https://aex.dev/brain/docs/guides/write-a-tool)
- **环境** 负责运行程序。它打开一个实例，声明程序在其中能找到的资源，并注册每种程序的启动
  方式。Brain 会把对它的每次调用写入日志。
  [编写环境](https://aex.dev/brain/docs/guides/write-an-environment)

### 官方扩展

[aexhq/extensions](https://github.com/aexhq/extensions) 中的包使用同一套 SDK 和同一套构建。
内置的东西没有任何捷径。

| 包 | 类型 | 说明 |
| --- | --- | --- |
| [`@aexhq/agentloop-pi`](https://www.npmjs.com/package/@aexhq/agentloop-pi) | Agent loop | Pi 风格的编码 loop。工具调用并行执行。 |
| [`@aexhq/agentloop-codex`](https://www.npmjs.com/package/@aexhq/agentloop-codex) | Agent loop | Codex 风格的编码 loop。工具调用逐个执行。 |
| [`@aexhq/tools`](https://www.npmjs.com/package/@aexhq/tools) | 工具 | `read`、`write`、`edit`、`ls`、`glob`、`grep`、`bash`、`todo` |
| [`@aexhq/env-aws-microvm`](https://www.npmjs.com/package/@aexhq/env-aws-microvm) | 环境 | 每个会话一台 AWS microVM，提供 `fs`、`process` 和 `net` |

## 工作原理

下面是一轮从头到尾的完整过程。Agent loop 决定下一步做什么。Brain 执行 I/O，在行动前把意图
写入日志，并在这一轮仍在进行时流式输出结果。

```text
                    +-------------------------------+
   your app ------->| session state, in memory      |
            <-------| live event feed               |
                    +---------------+---------------+
                                    | activate
                                    v
                    +-------------------------------+
                    | agent loop, a Wasm component  |   decides
                    +---------------+---------------+
                                    | decision
                                    v
                    +-------------------------------+        +-----------------+
                    | Brain does the I/O            |<------>| append-only log |
                    | for the loop                  | intent | off the turn's  |
                    +---------------+---------------+ result | hot path        |
                                    |                        +-----------------+
                                    +--> model provider, streaming
                                    |
                                    +--> tool, in any environment
```

Brain 拥有会话。你提供 agent loop、模型、工具和环境。四个设计决定让它很快：

- **隔离的 WebAssembly agent loop。** Loop 可以从任何语言编译为
  [Wasmtime](https://wasmtime.dev/) 组件。它只编译一次，每次决策时以原生速度激活，并且完全
  沙箱化。因为 I/O 由 Brain 执行，每个决策都是确定性的，可以从它在日志中的位置重放。
- **预写日志。** 唯一的持久状态是一个只追加的日志，在一轮结束后写入，因此不在热路径上。会话
  常驻内存，启动时从日志重建，所以重启后对话会从停下的地方继续。
- **有界的实时流。** 模型的增量一经提供商产出就送达订阅者。每个订阅者拥有固定 1,024 个事件
  的环形缓冲区，落后的读取者会从日志中它最后看到的那条记录继续。每个订阅者的成本保持恒定。
- **事件即数据模型。** 每一次观察、决策、模型调用、token 和工具结果都是同一条事件流中的一个
  事件。实时观看和读取历史使用同样的记录，所以追踪一个会话就等于重放它。

Brain 是一个基于 [Tokio](https://tokio.rs/) 的原生 Rust 二进制文件。它用
[Axum](https://github.com/tokio-rs/axum) 通过 HTTP 和 SSE 提供会话 API，不需要任何外部存储。

## 快速开始

在这个例子中，工具是你自己进程里的一个普通函数。你声明一次，然后传给会话。SDK 从会话的事件
流中响应模型的调用，因此你的应用不需要服务器、不需要开放端口，也不需要额外的通道。

运行服务器：

```sh
docker run --rm -p 127.0.0.1:8080:8080 \
  -e BRAIN_LISTEN=0.0.0.0:8080 -e BRAIN_API_TOKEN=quickstart \
  -v brain-data:/var/lib/brain ghcr.io/aexhq/brain:latest
```

```sh
npm install @aexhq/brain @aexhq/agentloop-pi zod
```

保存为 `order.mjs`，然后用 `node order.mjs` 运行：

```js
import { Brain, tool } from "@aexhq/brain";
import { pi } from "@aexhq/agentloop-pi";
import { z } from "zod";

const orders = { "A-1001": { status: "shipped", eta: "Thursday" } };
const lookupOrder = tool({
  name: "lookup_order",
  description: "Look up an order's status by id.",
  input: z.object({ id: z.string() }),
  execute: ({ id }) => orders[id] ?? { status: "unknown order" },
});

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080", token: "quickstart" });
const session = await brain.sessions.create({
  model: { provider: "openai", name: "gpt-5-mini", apiKey: process.env.OPENAI_API_KEY },
  agentloop: pi(),
  tools: [lookupOrder],
});

await session.send("Where is order A-1001?");
for await (const event of session.events()) console.log(event.sequence, event.type);

await session.end();
await session.delete();
process.exit(0);
```

模型读取问题并调用 `lookup_order`。这次调用以带类型记录的形式出现在会话的事件流中，你的函数
用它闭包中的 `orders` 对象作答，SDK 再把结果发回去。日志同时保留调用和结果。如果工具必须在
别处运行，比如浏览器页面、沙箱或另一台机器，它只需改为声明一个托管环境，会话 API 保持不变。
参见[应用工具指南](https://aex.dev/brain/docs/guides/app-tools)。

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
<code>c7g.xlarge</code> 上对每个对象测得的中位数。柱状图使用对数刻度。图表只包含通过 API
拥有会话的 agent 运行时。<a href="BENCHMARKS.md">BENCHMARKS.md</a> 记录了方法和各对象的版本。</sub>

## 路线图

- [x] 四部分运行时：agent loop、模型、工具、环境
- [x] 用 `brain build` 统一编写 `brain`、`tool` 和 `environment`
- [x] 只追加的分段日志，支持重启恢复
- [x] 带类型的内容标识
- [x] HTTP/SSE 会话 API 和 `@aexhq/brain` SDK
- [x] 远程环境契约，附带 `env-app` 和 `env-aws-microvm`
- [ ] 跨会话隔离测试
- [ ] 原生子 agent 支持，会话之间的父子关联
- [ ] 多模态输入，`send` 支持图片和文件
- [ ] 冻结 v1 API 并打标签发布
- [ ] 文件访问与工作区同步
- [ ] 发布到 crates.io
- [ ] 会话分布在多台机器上并共享环境
- [ ] `checkpoint` 与 `restore`
- [ ] 自定义镜像，带受限凭证和网络计量
- [ ] 本地环境：在你自己机器上的目录或容器中运行工具
- [ ] 浏览器环境与 DOM 工具：把页面作为工具运行的场所
- [ ] 基于同一份 `agentloop.wit` 契约、用 Rust 编写的 agent loop

## 联系方式

支持与 bug 反馈请提交 [issue](https://github.com/aexhq/brain/issues) 或写信至
[support@aex.dev](mailto:support@aex.dev)。合作与商务事宜请写信至
[admin@aex.dev](mailto:admin@aex.dev)。
