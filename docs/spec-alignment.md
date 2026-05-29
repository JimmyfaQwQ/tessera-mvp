# Tessera Spec Alignment 报告

> 基准：`E:/Tessera-Spec/` 当前内容 vs `E:/tessera-mvp/` commit `bbda9b1`（2026-05-29，初始审计）；本次修复在该 commit 之上叠加。
>
> 覆盖：tessera-lexer / tessera-parser / tessera-ast / tessera-types / tessera-runtime / tessera-interp / tessera-lint
>
> **状态徽标**
> - ✅ 已实现且有测试覆盖
> - ⚠️ 已实现但缺测试 / 行为未完全覆盖
> - ❌ 完全未实现
> - 🔶 实现与规范存在语义偏差，需修复或修订规范

## 本次修复总结（自初始审计以来）

下列条目已在本轮修复中处理并附测试：

### P0 全部完成

- **P0-1**：`HandlerDispatchError` kind 由 `"TargetGone"` 改为 `"TargetTerminated"`（`runtime/src/error.rs:201`）。规范侧《基础类型...》§6 表 393 行同步修正。测试 `test_target_terminated_kind`。
- **P0-2**：`__ping__` 虚拟 handler 注入（`types/src/checker/registration.rs` 自动注册 + `runtime/src/thread_state.rs::dispatch_handler` 入口特判）；不进 mpsc 队列、不受 R-HANDLER-2 / `#exclusive` 约束。测试 `tests/tss/ping_handler.tss`。
- **P0-3**：R-HANDLER-2 handler-in-flight gate（`ThreadState.handler_in_flight: watch<bool>` + `event_loop.rs` 主 select 增 gate 与 wakeup 分支 + `dispatch_handler_inline` 设置/释放门控）。测试 `tests/tss/handler_mutex.tss` 用时间戳序列断言无交错。
- **P0-4**：int 字面量越界 → parser 阶段报错（`parser/src/parser.rs::parse_primary` LitInt）。测试 `test_int_literal_overflow`。
- **P0-5**：define 字段对外不可见 — 经测试核实实现已天然隔离（FieldAccess 只查 `expose_fields` / `expose_mutable_fields`），无需改 runtime。测试 `test_define_field_external_invisible`。

### P1 主要完成（P1-6 延后）

- **P1-1 L-TOPLEVEL-CONTROL-FLOW**：顶层 `await`/`return` 禁止；顶层 `break`/`continue` 仅在 `for`/`while` 内合法（`passes/toplevel_control_flow.rs`）。
- **P1-2 L-FUNCTION-HOOK-SIGNATURE**：`__on_enter__` / `__on_exit__` 必须 sync void、`__on_terminate__` 必须 async void（`passes/hook_signature.rs`）；同时识别 parser 误归类为 MemberFunc 的情况。
- **P1-3 L-HANDLER-RESULT-IGNORED**：裸 handler 调用语句报警（`passes/handler_result_ignored.rs`）。`L-HANDLER-AWAIT-TYPE` 暂保留仅识别 Ident receiver 的现状（field 链需要表达式 typer，记为后续工作）。
- **P1-4**：char 支持 `\u{HHHH}` 转义（`lexer/src/token.rs::unescape` 与 char 正则）；String[i] 实际已实现，补类型检查 `Type::TString → Type::Char`；测试 `test_string_indexing_and_unicode`、`test_string_index_out_of_bounds`。
- **P1-5**：匿名 `${ ... }` 端到端测试 `tests/tss/anonymous_thread.tss` 用 `__ping__` 探活。
- **P1-6（延后）**：R-SYNC-BREAK-3 反例测试需要 runtime 在 `#exclusive` 期间延后 `signal/contract/permit` 的 Broken 唤醒；当前 `runtime/src/signal.rs` 未实现该延迟，写测试只会暴露已知缺口。改记入 P2 后续 TODO。

### 规范侧修订（A-1 / A-2 / A-3 / A-4 / A-5）全部完成

- **A-1**（HandlerDispatchError kind 命名）：《基础类型...》§6 表 `"TargetGone"` 改为 `"TargetTerminated"`。
- **A-2**（R-HANDLER-2 / R-HANDLER-3 张力）：保留 R-HANDLER-2 严格 handler↔handler 互斥，加 Rationale；R-HANDLER-3 显式声明 body↔handler 在挂起点交替合法；两条独立。实现侧用 P0-3 gate 满足。
- **A-3**（粘性 Terminating）：R-EXCL-3 末尾加"一旦 Terminating，即使后续主体自然结束仍按 terminate 路径执行 teardown"。
- **A-4**（同步原语链式绑定）：新增 R-SYNC-OWN-4 — 首次 expose 确立绑定主，二次 expose 不变更。
- **A-5**（R-HANDLER-PING 语义）：重写为"调度即应答；不进队列；不受 R-HANDLER-2 / `#exclusive` 约束；用户重写触发 L-HANDLER-PING-REDEFINED"。

### Linter 补漏（共 7 新增 + 1 现有保留改进）

新增 lint passes（注册于 `passes/mod.rs::all()`）：

| 规则 ID | 文件 |
|---|---|
| L-TOPLEVEL-CONTROL-FLOW | `passes/toplevel_control_flow.rs` |
| L-FUNCTION-HOOK-SIGNATURE | `passes/hook_signature.rs` |
| L-HANDLER-RESULT-IGNORED | `passes/handler_result_ignored.rs` |
| L-HANDLER-PING-REDEFINED | `passes/handler_ping_redefined.rs` |
| L-EXPOSE-READONLY-WRITE | `passes/expose_readonly_write.rs` |
| L-DEFINE-EXTERNAL-ACCESS | `passes/define_external_access.rs` |
| L-RETURN-NOT-ALL-PATHS | `passes/return_not_all_paths.rs` |

Lint pass 已 9 → 16（已实现 / 规范定义 ≈ 50+；覆盖率 ~18% → ~32%）。`L-AT-TEMPLATE-STACKING` 因 parser 语法已防御，跳过。`crates/tessera-lint/tests/lint_smoke.rs` 新增 11 个 smoke 测试覆盖每个新 pass。

### 测试规模

| 套件 | 修复前 | 修复后 |
|---|---:|---:|
| tessera-interp integration | 19 | 27 |
| tessera-lint smoke | 0 | 11 |
| tessera-parser 单测 | 5 | 5 |
| tessera-lexer 单测 | 5 | 5 |
| **合计** | **29** | **48** |

cargo build --workspace 与 cargo test --workspace 全部通过；`tessera-cli --check helloworld.tss` 与 `--check demo.tss` 均无诊断。

### 未来工作（已记入 spec-issues.md）

- R-SYNC-BREAK-3 runtime 实现 + 测试；
- L-HANDLER-AWAIT-TYPE 扩展到 field/method-chain receiver（需表达式 typer）；
- 同步原语 `signal`/`contract` 的 sync/async 上下文 lint（与现有 permit pass 镜像）；
- `keepalive` / `getchar` / 内建函数规范化为标准文档章节。

---


## 目录

0. [摘要表](#0-摘要表)
1. [Tessera 核心语义](#1-tessera-核心语义)
2. [模板与线程规范](#2-模板与线程规范)
3. [线程与事件循环规范](#3-线程与事件循环规范)
4. [数据共享与并发安全规范](#4-数据共享与并发安全规范)
5. [async / await 规范](#5-async--await-规范)
6. [错误与异常语义](#6-错误与异常语义)
7. [同步原语与崩溃传播规范](#7-同步原语与崩溃传播规范)
8. [基础类型、表达式与函数规范](#8-基础类型表达式与函数规范)
9. [标准容器与常用类型规范](#9-标准容器与常用类型规范)
10. [语句与控制流规范](#10-语句与控制流规范)
11. [泛型与类型构造器规范](#11-泛型与类型构造器规范)
12. [Linter 规则对照](#12-linter-规则对照)
13. [example-code.md 覆盖映射](#13-example-codemd-覆盖映射)
14. [已识别偏差](#14-已识别偏差)
15. [差距修复 TODO（P0 / P1 / P2）](#15-差距修复-todo)
16. [验证与维护](#16-验证与维护)

## 0. 摘要表

### 规范文档对齐度

| # | 规范文档 | 已对齐 ✅ | 部分 ⚠️ | 缺失 ❌ | 偏差 🔶 |
|---|---|---:|---:|---:|---:|
| 1 | Tessera 核心语义 | 7 | 2 | 0 | 0 |
| 2 | 模板与线程规范 | 5 | 1 | 1 | 0 |
| 3 | 线程与事件循环规范 | 9 | 2 | 2 | 1 |
| 4 | 数据共享与并发安全规范 | 6 | 2 | 1 | 1 |
| 5 | async / await 规范 | 5 | 1 | 1 | 0 |
| 6 | 错误与异常语义 | 6 | 2 | 1 | 1 |
| 7 | 同步原语与崩溃传播规范 | 8 | 0 | 0 | 0 |
| 8 | 基础类型、表达式与函数规范 | 12 | 3 | 4 | 1 |
| 9 | 标准容器与常用类型规范 | 8 | 2 | 4 | 0 |
| 10 | 语句与控制流规范 | 11 | 1 | 1 | 0 |
| 11 | 泛型与类型构造器规范 | 3 | 1 | 2 | 0 |
| 12 | Linter 规则草案 | 9 | 0 | 36 | 0 |
| 13 | example-code.md | 8 | 2 | 0 | 0 |
| **合计** | | **97** | **19** | **53** | **4** |

### 三大维度总体对齐

| 维度 | 估算对齐率 | 主要短板 |
|---|---|---|
| 解释器（lexer + parser + ast + runtime + interp） | ~85% | `__ping__` 隐式 handler、char Unicode 转义、字符串方法 `[i]` 未实现 |
| 类型系统（tessera-types） | ~70% | 无用户函数泛型；TemplateObject 的方法解析覆盖未审计；`define` 字段越界访问未阻止 |
| Linter（tessera-lint） | ~18%（9/50+） | 36 条规则未实现；已实现规则的覆盖也有盲区（如 L-HANDLER-AWAIT-TYPE 仅识别 Ident 接收者）|


## 1. Tessera 核心语义

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| R-TYPE-STATIC-1 | `let` 决定变量类型；后续赋值必须类型兼容 | ✅ | `crates/tessera-types/src/checker/stmts.rs:13-35` | 内联测试 `test_arithmetic` 隐含覆盖 |
| R-CORE-MAIN-1 | 主体执行完毕 → 线程自动终止（不调 `__on_terminate__`） | ✅ | `crates/tessera-interp/src/event_loop.rs:226-265` | `__on_exit__` 单独执行；`__on_terminate__` 不会被触发 |
| R-CORE-TERM-1 | `Terminated` 后不再执行 handler、不再修改 expose | ✅ | `crates/tessera-runtime/src/thread_state.rs:170-188` | `dispatch_handler` 立刻返回 `TargetTerminated` |
| R-CORE-HANDLER-1 | `Terminated`/`Crashed` 后 handler future 不得永久挂起 | ✅ | `event_loop.rs:252,259,289` + `drain_handlers` | 队列在状态切换时统一 drain |
| R-CORE-SCHED-1 | 单一执行点；同步段直至挂起点不可被抢占 | ✅ | `event_loop.rs:186-306`（tokio `select!` 配合 `biased`）| 单线程 `LocalSet` 协作调度 |
| R-CORE-SCHED-2 | 同一线程的 handler 请求 FIFO；不并发执行 | ⚠️ | `event_loop.rs:300-304`（mpsc 通道 + 单点 select）+ `dispatch_handler_inline:340` | FIFO ✅；但 `dispatch_handler_inline` 使用 `spawn_local` 使 handler 与主体并发，违反"线程内不并发"的字面意思 — 详见 §14 偏差 1 |
| R-CORE-EXCL-1 | `#exclusive` 块独占执行；不交错 handler/timer/IO | ✅ | `event_loop.rs:300, 345-350` | 主路径 + handler 任务双重等待 |
| R-CORE-SHARE-1 | 子线程不可直接读写父线程局部变量 | ✅ | `event_loop.rs:37-120`（`current_thread_state` 与 env 切换；线程 body 使用新建 env）| 没有跨线程 env 共享；测试 `thread_lifecycle.tss` 隐含验证 |
| R-CORE-SHARE-2 | 不存在"只读直接引用"父局部变量的形式 | ✅ | parser 无此语法；runtime 无对应 Value 变体 | 反例不可表达 |


## 2. 模板与线程规范

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| 模板基本语法 `@template` / `$template` | lexer + parser + AST | ✅ | `lexer/src/token.rs:9-21`、`parser/src/parser.rs:205-360`、`ast/src/lib.rs`（ScopeTemplateDecl / ThreadTemplateDecl）| 命名 + 匿名两种形式都已支持 |
| 匿名简写 `${ ... }` | 创建不可终止匿名线程 | ⚠️ | `parser.rs:572-592`、`event_loop.rs`（无 decl 路径）| 词法+句法✅；无独立 .tss 测试 |
| Terminatable 判定（声明 `async function __on_terminate__()`）| 自动识别 | ✅ | `parser.rs:300`、`types/src/checker/registration.rs`、`runtime/src/thread_state.rs:42,93` | `is_terminatable` 字段贯穿 |
| R-TEMPLATE-STACKING：同一块不可叠加多个 `@template` | 语法层禁止 | ✅ | `parser.rs:457-465`：scope_block 一次只能有一个 `@template`/`@Name`；语法不允许重复 | 无显式 lint，但语法不可表达 |
| 线程模板成员：`expose` / `expose_mutable` / `define` / `async handler` / 三个生命周期 hooks / 普通 method | 全部支持 | ✅ | `parser.rs:270-360`、`ast/src/lib.rs`（ThreadTemplateMember） | 句法/AST 完整 |
| Scope 模板成员：`define` + `__on_enter__` / `__on_exit__` + member 函数（无 handler、无 expose）| 严格限制 | ✅ | `parser.rs:228-243` | scope body 不接受 `expose`/`handler` 关键字 |
| 线程句柄绑定 `:= h` 与离开作用域不自动终止 | 语义 | ✅ | `parser.rs:616-622`、句柄是 `Arc<ThreadState>`，不绑定生命周期 | 离开作用域只丢弃 `Arc`，不触发 terminate |
| Self 与模板参数可见性 | `self.fieldName`、`self.paramName` | ⚠️ | `event_loop.rs:115-120`、`types/src/checker/registration.rs`、`eval.rs`（field access） | 实现存在；但参数与 expose/define 字段是否互不覆盖未审计；缺反例测试 |
| 模板参数错误（arity 不匹配）→ 线程崩溃 | 实现层防御 | ✅ | `event_loop.rs:102-114` | 不静默吞 |
| 启动位置：spawn 仅允许在表达式语句位置 | 句法约束 | ❌ | parser 允许 `$Name(...)` 出现在多个位置 | 未实现 Linter L-TEMPLATE-APPLY-CONTEXT，运行时也不阻止滥用位置 |


## 3. 线程与事件循环规范

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| R-SCHED-1 | 协作调度；仅在 await/return/error 处挂起 | ✅ | tokio LocalSet + spawn_local | 单 OS 线程协作 |
| R-SCHED-2 | 同步段一旦开始执行到下个挂起点不可被打断 | ✅ | `event_loop.rs:186-306` 主 select 与 handler 任务通过 watch 通道同步 | |
| R-LIFE-1 | 主体自然结束 → 自动 Terminated，不调用 `__on_terminate__` | ✅ | `event_loop.rs:226-265`：只调用 `__on_exit__` | 内联测试覆盖 |
| R-LIFE-2 | 显式 `terminate()` 才会触发 `__on_terminate__` 然后 `__on_exit__` | ✅ | `event_loop.rs:199-223`（先 `run_teardown_hooks(.., true)`）；helper 在 `run_teardown_hooks:384-402` 顺序执行 `__on_terminate__` → `__on_exit__` | `thread_lifecycle.tss` 覆盖 |
| R-HANDLER-1 | handler FIFO（按线程实例）| ✅ | `mpsc::Receiver` 顺序消费 | |
| R-HANDLER-2 | 同线程不并发执行两个 handler | 🔶 | `event_loop.rs:340-355`：实际通过 `spawn_local` 把 handler 与主体并发到同一 LocalSet | 单 OS 线程 + 协作，但语义上"并发执行"已经发生（解释器代码注释也承认是为了打破 deadlock） — 详见 §14 偏差 1 |
| R-HANDLER-3 | 主体 ↔ handler 仅在挂起点切换 | ✅ | 协作调度自然满足 | |
| R-HANDLER-4 | Terminating/Terminated/Crashed 拒绝新 handler；队列项以调度失败结束 | ✅ | `thread_state.rs:170-188` + `event_loop.rs::drain_handlers` | 三种状态映射到三个 dispatch error |
| R-HANDLER-5 | 主体结束后不再执行 handler | ✅ | `event_loop.rs:252,259,289` 主体完成路径 drain 队列 | |
| R-HANDLER-PING | 所有线程模板隐式具有 `async handler __ping__(): String` 返回 "pong" | ❌ | `eval.rs::find_handler` 未注入；`registration.rs` 未注册 | 例 `exclusive_block.tss` 中 `ping` 是用户自定义 handler，与 `__ping__` 不同 |
| R-HANDLER-SCOPE | handler 只能访问模板对象，不可访问外部作用域 | ⚠️ | `event_loop.rs:39,120` + 创建 handler body 时使用模板 env | 实现倾向正确，但无 lint/反例测试 |
| R-EXCL-1 | `#exclusive` 内独占线程；handler/timer/IO 不交错 | ✅ | `event_loop.rs:300 + 345-350`（select gate + handler 任务 watch 等待）| `exclusive_block.tss` 覆盖 |
| R-EXCL-2 | `#exclusive` 阻塞 handler 进入，但不阻塞入队 | ✅ | mpsc 通道一直接收；只在主 select 上 gate | |
| R-EXCL-3 | 在 `#exclusive` 期间收到 terminate → 立即转 Terminating，teardown 延后 | ✅ | `event_loop.rs:157-184, 199-223, 270-294` | 代码注释明确引用 R-EXCL-3 |
| R-EXCL-4 | `#exclusive` 内 await 依赖外部 handler → 死锁警告 | ❌ | 无 Linter pass；运行时无死锁检测 | 静态分析缺失 |
| R-KEEPALIVE-1 | `keepalive()` 返回永不完成 Future | ⚠️ | `crates/tessera-interp/src/eval/builtin.rs`、`eval.rs` 中实现并返回挂起 Future | 需复核 await 后是否真正不返回；缺独立测试 |
| R-KEEPALIVE-2 | `keepalive()` 主体不参与 terminate；terminate 由 hook 驱动 | ✅ | `event_loop.rs:226+` 主体即使永远不返回，terminate 通过 select 路径触发 teardown | `handler_dispatch.tss` 使用 keepalive 隐含覆盖 |


## 4. 数据共享与并发安全规范

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| R-CORE-SHARE-1 / 2 | 子线程不可直接访问父线程局部变量 | ✅ | 见 §1 | |
| R-CORE-SHARE-3 | 通过参数传递初值；避免跨线程存储共享 | ✅ | `event_loop.rs:99-117`（参数复制进入 template_self）| |
| R-CORE-SHARE-4 | 跨线程访问线程状态 → 通过 expose / handler | ✅ | `thread_state.rs:47-50,164-189` | |
| R-CORE-SHARE-5 | 多线程共享数据 → 并发安全类型（`locked<T>`/`Queue<T>`）| ✅ | `runtime/src/locked.rs`、`runtime/src/queue.rs` | |
| R-EXPOSE-1 | 只读共享 = `expose`；写入只能通过 handler 或并发安全类型 | ⚠️ | `parser.rs:272`、`event_loop.rs:67-69` 区分两种 expose；runtime 强制只读未严格审计 | 缺 Linter L-EXPOSE-READONLY-WRITE |
| R-EXPOSE-2 | `expose_mutable` 字段类型必须并发安全 | ✅ | `tessera-lint/src/passes/expose_mutable_unsafe.rs:8-31` + `types/src/ty.rs:47-49` | L-EXPOSE-MUTABLE-UNSAFE pass 实现 |
| R-EXPOSE-3 | `expose_mutable` 字段引用不可被外部替换，只可通过其方法改内容 | ⚠️ | `runtime/src/error.rs:108-116` 定义 `ExposeMutableFieldReplace`；但触发路径需复核（eval.rs 是否在外部赋值时实际抛出） | 错误变体存在但触发覆盖未验证 |
| R-DEFINE-1 | `define` 字段仅在模板内部可见，不可经线程句柄访问 | ⚠️ | 字段分别存放于 template_self（非 expose_fields） | runtime 层面隔离；缺 Linter L-DEFINE-EXTERNAL-ACCESS |
| R-DEFINE-2 | `define` 在 @template / $template 都可声明；`expose`/`expose_mutable` 仅在 $template | ✅ | parser 严格限制 scope template 成员（`parser.rs:228-243`） | |
| R-TERMINATE-STABLE-1 | terminatable 线程 `terminate().wait()` 后 expose 字段稳定 | ✅ | `event_loop.rs:198-223` 主路径将 expose 同步固化（不再有 handler/body 写）| |
| R-TERMINATE-STABLE-2 | 非 terminatable 线程无稳定态语义 | ✅ | 无 terminate() 入口；自然结束触发 §3 R-LIFE-1 | |
| 跨线程引用类型限制 | List / Map 不可跨线程共享（Rc 而非 Arc） | 🔶 | `runtime/src/value.rs:25-26`：`List(Rc<...>)`、`Map(Rc<...>)` | 与规范要求一致；但通过 expose 暴露后跨线程读会触发 Rc 非 Send 编译错误而非给出领域错误信息 — 边界场景缺测试 |


## 5. async / await 规范

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| §1.1 / §1.2 | `await` 仅在 async 函数 / handler 内 | ✅ | `tessera-lint/src/passes/await_async_only.rs` + `types/src/checker/exprs.rs:71-85`（context flag）| L-AWAIT-ASYNC-ONLY pass |
| §1.3 | async 函数声明类型为"解包后类型"，调用返回 `Future<T>` | ✅ | `types/src/checker/registration.rs`（注册时包裹 Future）+ `eval.rs:737-754`（调用立即返回 Future）| |
| §2.1 | `.wait()` 同步阻塞；`await` 协程挂起 | ✅ | `builtin.rs:148-153`、`eval.rs::Expr::Await` | |
| §2.2 | `HandlerFuture` 的 wait 语义类比 Future | ✅ | `builtin.rs:157-174`、`runtime/src/future.rs:97-219` | 区分 Dispatch/Execution 失败 |
| §2.4 | `signal` / `contract` / `permit` 可直接 await | ✅ | `types/src/checker/exprs.rs:71-85` 把它们识别为 awaitable | `scope_binding.tss` 覆盖 signal |
| 顶层 await 限制 | 顶层不可写 `await expr;` | ❌ | parser 不区分；缺 L-AWAIT-EXPR-IN-TOPLEVEL lint | 运行时若顶层无 async 环境，行为未定义 |
| async 函数若全程无 await（信息提示） | L-ASYNC-NO-AWAIT 信息级提示 | ❌ | 无对应 pass | |


## 6. 错误与异常语义

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| R-TRY-1 | `try expr` 捕获运行时错误 → `Result<T, error>`；不崩溃 | ✅ | `parser.rs:816-821`、`eval.rs`（Try 分支）+ `runtime/src/error.rs:157-185`（`kind_and_message` 转换）| `scope_binding.tss` 覆盖 |
| R-TRY-2 | `try await expr` = `try (await expr)`，仅 async 上下文 | ⚠️ | parser 中 `try` 与 `await` 都作 unary prefix，组合可解析；缺乏严格上下文检查 | 缺独立测试 |
| R-TRY-3 | `error` 值具有稳定 `.kind` 与 `.message` 字段 | ✅ | `runtime/src/value.rs:47`（`ErrorObj { kind, message }`）+ `kind_and_message` 集中映射 | `scope_binding.tss` 比较 `kind == "ScopeGone"` |
| `panic(msg): never` | 立即触发线程崩溃 | ✅ | `parser.rs:795-801`、`eval.rs`、`error.rs:7-13` | 内联 `test_assert_failure` 相关 |
| `assert(cond, msg?)` | false → AssertionFailed 崩溃；所有 build 模式启用 | ✅ | `parser.rs:802-808`、`eval.rs`、`error.rs:15-24` | 无构建期开关 |
| §1.3.1 / §1.3.2 | `.wait()` / `await` 失败 Future → 调用线程崩溃 | ✅ | `builtin.rs:148-152`（Future.wait 失败 → RuntimeError::Panic）；`eval.rs::Await` 同样路径 | |
| §3.2 | 线程崩溃后停止执行，不再更新 expose / handler | ✅ | `thread_state.rs:111-127`（set_status Crashed 时 break primitives）+ event_loop 主路径 break | |
| §3.4.1 | Handler 调度失败 → `HandlerFuture` 立即失败（dispatch 前）| ✅ | `thread_state.rs:170-188`、`future.rs:88-95,182-197` | |
| §3.4.2 | Handler 执行失败 → 目标线程崩溃 + 调用方 `.wait()`/`await` 崩溃 | ✅ | `event_loop.rs::dispatch_handler_inline:351-353` + `future.rs::HandlerResolveResult::ExecutionFailed` + `builtin.rs:163-169` | |
| §3.5 | 同步原语 `Broken` 失败 kind 为 `OwnerGone`/`OwnerCrashed`/`ScopeGone`/`ScopeCrashed` | ✅ | `runtime/src/signal.rs:6-28` | `scope_binding.tss` 仅覆盖 ScopeGone |
| §5.2 | `__on_terminate__` 错误 → 线程崩溃；调用方 .wait()/await 崩溃 | ✅ | `event_loop.rs::run_teardown_hooks:384-402`（首错被记录、转 Crashed）| |
| `__on_exit__` 错误 → 线程崩溃；终止流程失败 | ✅ | 同上；内联 `test_scope_on_exit_error_surfaced` | |
| HandlerDispatchError kind 命名一致性 | "TargetTerminated" / "TargetTerminating" / "TargetCrashed" | 🔶 | `runtime/src/error.rs:198-207` 把 `TargetTerminated` 映射为 `"TargetGone"` | 与多数规范文档不一致；仅 `基础类型... 规范.md:393` 用 "TargetGone"。详见 §14 偏差 2 |


## 7. 同步原语与崩溃传播规范

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| R-SYNC-OWN-1 | 每个原语最多 1 个线程绑定主（首次 expose 获得） | ✅ | `runtime/src/signal.rs:38-51, 151`（`Ownership` + `try_claim_ownership`）；contract/permit 同样持有 | `try_claim` 在 expose 路径调用 |
| R-SYNC-OWN-2 | 未绑定的原语永不进入 Broken；wait 不失败 | ✅ | break 仅在 thread Terminated/Crashed 时触发，未绑定即未注册 owned | |
| R-SYNC-OWN-3 | @template `define` 的原语 = 作用域绑定，不随线程传递 | ✅ | `signal.rs:13-16`（`ScopeGone`/`ScopeCrashed` 变体） + 触发于 scope 退出路径 | `scope_binding.tss` 覆盖 ScopeGone |
| R-SYNC-BREAK-1 | 线程绑定 → 仅在 owner 进入 `Terminated`/`Crashed` 时 Broken；`Terminating` 不触发 | ✅ | `thread_state.rs:111-127`：`set_status(Terminated/Crashed)` 才 `break_with` | |
| R-SYNC-BREAK-2 | owner 在 terminate 之前崩溃 → 已经 Broken | ✅ | 同上；Crashed 路径覆盖 | |
| R-SYNC-BREAK-3 | `Broken` 唤醒不早于当前 `#exclusive` 结束 | ⚠️ | 实现未显式延后 `notify_waiters`；但 thread 状态切换由 event_loop 在 select 中执行，`#exclusive` 内不会切换状态 | 间接成立但缺反例测试 |
| R-SYNC-BREAK-4 | scope 绑定 → `__on_exit__` 返回后 `ScopeGone`；中途崩溃 `ScopeCrashed` | ✅ | scope 执行路径在 `eval.rs::exec_scope_block` 处理 | `scope_binding.tss` 覆盖 |
| R-SYNC-WAKE-1 | Broken 必须有限步唤醒所有 waiter | ✅ | `signal.rs:175`、`signal.rs:277`（`notify_waiters`）；permit 用 `Semaphore::close()` | |
| R-SYNC-AWAIT-1 | `Broken` 完成 `.wait()`/`await` 触发崩溃 | ✅ | `builtin.rs:268-277, 284-293, 319-328` 转为 `RuntimeError::Structured` | |
| R-SYNC-NOORPHAN-1 | owner 死亡不留孤立 wait | ✅ | 与上面三条联合保证 | |
| permit 计数 close 即 broken | tokio::Semaphore::close() | ✅ | `runtime/src/signal.rs:288-365` | `awaitPermit` 失败映射为 Structured(kind) |
| contract 的 pending-before-broken 语义 | broken 时仍可消费已 pending 的通知 | ✅ | `signal.rs:251-268` 注释和代码均覆盖 | 细致语义实现 |


## 8. 基础类型、表达式与函数规范

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| §1.1.1 bool | `!` / `&&` / `||` / `==` / `!=` | ✅ | `lexer/src/token.rs`、`parser.rs:719-735`、`eval.rs::eval_binop` | |
| §1.1.2 int | 32-bit signed；算术 + 比较 | 🔶 | 词法 `LitInt(i64)`、值 `Value::Int(i32)`、转换 `as i32` | i64 → i32 narrowing 可能溢出而不报错（用 `as` 截断），与"32 位有符号溢出语义"未明确对齐 |
| §1.1.3 double | IEEE754 64-bit | ✅ | `Value::Double(f64)`、`lexer LitDouble(f64)` | |
| §1.1.4 char | 单 Unicode scalar；转义 | ⚠️ | `lexer/src/token.rs:307-334`（`\n \t \r \" \' \\ \0`，无 `\u{...}`）| 无 Unicode 转义 |
| §1.1.5 String | 不可变；`+` 拼接（隐式转换）；`.length()`；`[x]` 取字符 | ⚠️ | `eval.rs` Add 支持 String + others；`builtin.rs:42`（`length`）；Index 表达式对 String 未实现 | `[i]` 缺失 |
| §1.2.1 never | 表达式底类型 | ✅ | `ty.rs::Type::Never`、`Value::Never`、parser `KwNever` | `panic` 返回类型 never |
| §1.3.1 panic | `panic(msg): never` | ✅ | §6 已覆盖 | |
| §1.3.2 assert | 失败 → AssertionFailed | ✅ | §6 已覆盖 | |
| §1.3.4 try expr | 捕获 → Result<T, error> | ✅ | §6 已覆盖 | |
| §2.1 求值顺序 | 子表达式从左到右 | ✅ | `eval.rs::eval_binop` 先 eval left 再 right；method args 顺序循环 | |
| §2.1 短路 | `&&` / `||` | ⚠️ | `eval.rs::eval_binop` 检查 short-circuit；需复核是否对 LHS=false 时 RHS 不求值 | 缺反例测试（带副作用 RHS）|
| §2.2 运算符优先级 | 一元 > * / % > + - > 比较 > ==/!= > && > || | ✅ | `parser.rs:964-973`（Pratt binding power）+ unary 在 primary 前 | 单元测试 `pratt_precedence` |
| §3.1 `let name = expr;` / `let name: T = expr;` | 变量声明必须初始化 | ✅ | `parser.rs:480-488` + `checker/stmts.rs:13-20` | |
| §3.2 禁止未初始化声明 | `let x: int;` 禁止 | ✅ | parser 要求 `=` 否则报错 | |
| §4.1.2 非 void 函数所有路径 return | 静态检查 | ❌ | 无 L-RETURN-NOT-ALL-PATHS | 缺失 |
| §4.1.2 void 函数允许隐式末尾 return | | ✅ | `checker/bodies.rs` 实现 | |
| §4.2 async 函数返回类型自动包 `Future<T>` | | ✅ | `registration.rs` 注册时包裹 | |
| §4.3 hooks 签名约束（`__on_enter__`/`__on_exit__` 是同步 void；`__on_terminate__` 是 async void）| | ❌ | parser 只是按名识别成 hook；不强制签名 | 缺 L-FUNCTION-HOOK-SIGNATURE |
| 函数返回类型不匹配 | 静态检查 | ❌ | 无 L-RETURN-TYPE-MISMATCH | 缺失 |
| void 函数返回值表达式 | 静态检查 | ❌ | 无 L-VOID-RETURN-VALUE | |


## 9. 标准容器与常用类型规范

| 类型 | 规范方法 | 实现位置 | 状态 |
|---|---|---|---|
| `List<T>` | length、isEmpty、push、pop、get、set、indexing | `builtin.rs:41-109`、`Value::List(Rc<RefCell<Vec<Value>>>)` | ✅ |
| `Map<K, V>` | size、get、set、remove | `builtin.rs:111-145`；构造需 `Map<K,V>(...)` 显式类型参 | ⚠️ contains() 缺失；spec 列出 contains/getOr 等方法部分未实现 |
| `Option<T>` | Some/None、isSome、isNone、unwrap、unwrapOr | `builtin.rs:22-28`、TypeCtor 路径 | ✅ |
| `Result<T, E>` | Ok/Err、isOk、isErr、unwrap、unwrapErr、unwrapOr | `builtin.rs:30-38` | ✅ |
| `HandlerDispatchError` | TargetTerminated/Terminating/Crashed 三个变体 + 字符串比较 | `runtime/src/error.rs:188-207` | 🔶（"TargetGone" 命名见 §6）|
| `HandlerFuture<R>` | wait/waitHandler、isDone、isOk、isErr、与 `Err("Xxx")` 比较 | `runtime/src/future.rs:97-219`、`builtin.rs:157-174` | ✅ |
| `Queue<T>` | push/enqueue/tryPush/tryPop/dequeue/size/isEmpty/isClosed/waitForNonEmpty/close；capacity ≤ 0 = 无界 | `runtime/src/queue.rs:1-106`、`builtin.rs:200-229` | ✅ |
| `locked<T>` | lock/tryLock/unlock/isLocked/get/set；显式 + 隐式两接口 | `runtime/src/locked.rs` | ✅ |
| `signal` / `contract` / `permit` | 已在 §7 覆盖 | `runtime/src/signal.rs` | ✅ |
| `ParseError` | `String.toInt() / toDouble()` 返回 | `builtin.rs:52-62`：以 `Result<T, Value::Str(...)>` 表达 | ⚠️ 实际错误是 `Value::Str`，非独立 `ParseError` 类型 |
| `error`（`.kind` + `.message`）| `try` 表达式结果 | `runtime/src/value.rs:47`、`error.rs::kind_and_message` | ✅ |
| `Future<T>` | wait、isDone | `builtin.rs:148-154` | ✅ |
| 字符串方法（startsWith / endsWith / split / trim / 等）| 标准容器规范列出 | 未实现 | ❌ |
| `List.indexOf / contains / slice / forEach` 等扩展方法 | 标准容器规范扩展接口 | 部分未实现 | ❌ |
| 自定义构造器（List<int>() 元素列表）| `List<int>(1,2,3)` | `parser.rs:851-882`、`eval.rs::TypeCtor` | ✅ |


## 10. 语句与控制流规范

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| `let` 必须初始化 | | ✅ | `parser.rs:480-488` | |
| 变量可在嵌套块中遮蔽，同块内重复声明禁止 | | ⚠️ | `interp/src/env.rs` 嵌套 env 实现遮蔽；同块重复声明检查未审计 | 缺反例测试 |
| §2.2 赋值 RHS 类型须匹配 | | ✅ | `checker/stmts.rs:21-35` | |
| §3.1 块作用域：`let` 离开块即失效 | | ✅ | env 在 block enter/exit 推/弹 | |
| §3.2 `@template` 修饰块 hooks 顺序 | `__on_enter__` 前置，`__on_exit__` 后置 | ✅ | `eval.rs::exec_scope_block` | `scope_hooks.tss` 覆盖 |
| §4.2 `if` 条件必须 bool | | ✅ | `checker/stmts.rs:37-55` | |
| §5.1 `while`：break/continue 支持 | | ✅ | `parser.rs:509-517` + `eval.rs::exec_while` | |
| §5.2 `for(init; cond; update)`：循环变量作用域局限 | | ✅ | `parser.rs:519-543` + scoped env | |
| §6.1 / §6.2 break / continue | 仅在循环内 | ⚠️ | parser 允许；`eval.rs` 使用 Result 退出循环；外层使用未做编译期检查 | 运行时返回的 Break/Continue 在循环外会传到顶层；缺静态拒绝 |
| §7 return 语义 | 非 void 必须有值；线程 body 不允许直接 return 出线程 | ⚠️ | 运行时 return 在 thread body 顶层会导致主体结束（自然终止）| 行为合理但与 spec "不允许"严格语义不同；缺 lint |
| §8.1 顶层禁止 return/break/continue | | ❌ | parser 允许出现；运行时把 break/continue 视为错误传播 | |
| §9 线程主体自然结束 → 线程终止 | | ✅ | `event_loop.rs:226-265` | |


## 11. 泛型与类型构造器规范

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| 容器类型显式类型参 | `List<int>()`、`Map<String, int>()` 等 | ✅ | `parser.rs:851-882`（TypeCtor）+ `checker/resolve.rs`、`checker/exprs.rs::check_type_ctor` | `looks_like_type_args` heuristic |
| 类型相等：构造器 + 所有类型参数匹配 | | ✅ | `types/src/ty.rs::PartialEq derive` | |
| 当前阶段仅不变（invariance） | 无协变/逆变 | ✅ | 隐式：所有类型比较均为相等 | |
| 命名构造器（Ok/Err/Some/None）无类型参 | `Ok(1)` 不写 `Ok<int>(1)` | ✅ | `parser.rs:864-870` 特判 | |
| 标准泛型类型 | List/Map/Option/Result/locked/Future/HandlerFuture/Queue | ✅ | `ty.rs:15-22` + `checker/resolve.rs::map_named` | `thread<T>` 不支持（用 `ThreadHandle(TemplateId)` 代替） |
| 用户函数 / 模板泛型参数 | `function f<T>(x: T): T` | ❌ | parser/AST 不支持类型参数列表 | |
| 类型参数个数错误 | L-GENERIC-TYPE-ARG-COUNT | ✅（与 MISSING 合并） | `tessera-lint/src/passes/generic_type_arg_missing.rs:24-39`（同时检查 `len != n`） | 已涵盖 |
| 嵌套深度提示 | L-GENERIC-NESTING-DEPTH | ❌ | 未实现 | |


## 12. Linter 规则对照

实现注册在 `crates/tessera-lint/src/passes/mod.rs::all()`，当前共 9 个 pass。

### 已实现（9）

| 规则 ID | 文件 | 检查内容 | 状态 |
|---|---|---|---|
| L-AWAIT-ASYNC-ONLY | `passes/await_async_only.rs` | `await` 仅在 async 函数 / handler 内 | ✅ |
| L-HANDLER-MUST-ASYNC | `passes/handler_must_async.rs` | handler 必须 async（语法已强制；pass 为空占位） | ✅（语法层）|
| L-HANDLER-AWAIT-TYPE | `passes/handler_await_type.rs` | `.wait()` 不可用于 HandlerFuture；反之亦然 | ⚠️ 仅识别简单 Ident 接收者，未覆盖 field/method 链 |
| L-EXPOSE-MUTABLE-UNSAFE | `passes/expose_mutable_unsafe.rs` | `expose_mutable` 字段类型必须并发安全 | ✅ |
| L-GENERIC-TYPE-ARG-MISSING（+ COUNT）| `passes/generic_type_arg_missing.rs` | 容器泛型类型参个数检查 | ✅（兼覆盖 wrong-count）|
| L-TERMINATE-NON-TERMINATABLE | `passes/terminate_non_terminatable.rs` | `.terminate()` 仅对 terminatable 线程 | ✅ |
| L-PERMIT-AWAIT-IN-SYNC | `passes/permit_await_in_sync.rs` | sync 上下文中不可 `.awaitPermit()` | ✅ |
| L-PERMIT-WAIT-IN-ASYNC | `passes/permit_wait_in_async.rs` | async 上下文中用 `.wait()` 警告 | ✅ |
| L-PERMIT-RELEASE-NON-POSITIVE | `passes/permit_release_non_positive.rs` | `permit(initial)` ≥ 0，`release(n)` > 0 | ✅ |

### 未实现（按规范 `Linter 规则草案.md` 列举）

| 类别 | 缺失规则 |
|---|---|
| async/await | L-ASYNC-NO-AWAIT (Info)；L-AWAIT-EXPR-IN-TOPLEVEL (Error) |
| terminate | L-TERMINATE-FUTURE-IGNORED (Warn) |
| expose / define | L-EXPOSE-READONLY-WRITE (Error)、L-EXPOSE-MUTABLE-ACCESS-PATTERN (Info)、L-DEFINE-EXTERNAL-ACCESS (Error)、L-DEFINE-IN-NON-TEMPLATE (Error) |
| handler | L-HANDLER-NO-AWAIT (Info)、L-HANDLER-CALL-SITE-AWAIT (Info)、L-HANDLER-RESULT-IGNORED (Warn)、L-HANDLER-DISPATCH-ERROR-UNHANDLED (Info)、L-HANDLER-FUTURE-MISUSE (Error) |
| template | L-TEMPLATE-APPLY-CONTEXT (Error)、L-AT-TEMPLATE-STACKING (Error) |
| function | L-RETURN-TYPE-MISMATCH、L-RETURN-NOT-ALL-PATHS、L-FUNCTION-HOOK-SIGNATURE、L-VOID-RETURN-VALUE |
| generics | L-GENERIC-NESTING-DEPTH (Info) |
| option/result | L-OPTION-UNWRAP-POSSIBLE-NONE、L-RESULT-UNWRAP-POSSIBLE-ERR |
| future | L-AWAIT-UNCONSUMED-FUTURE (Warn) |
| sync 原语 | L-SIGNAL-AWAIT-IN-SYNC、L-SIGNAL-WAIT-IN-ASYNC、L-CONTRACT-AWAIT-IN-SYNC、L-CONTRACT-WAIT-IN-ASYNC、L-SYNC-EXPOSE-OWNER (Info)、L-SYNC-AWAIT-NOCHECK、L-SYNC-SCOPE-DEFINE-PASS (Warn)、L-SYNC-TRIGGER-AFTER-BROKEN (Info) |
| panic / assert | L-ASSERT-SIDE-EFFECT、L-PANIC-OVERUSE、L-ASSERT-ALWAYS-TRUE、L-ASSERT-ALWAYS-FALSE |
| style | L-NAMING-THREAD-HANDLE (Info) |

> 估算：规范共定义 ~45–50 条 L 规则，实现仅 9 条。覆盖率 ~18%。


## 13. example-code.md 覆盖映射

| 示例 | 关键特性 | 测试覆盖 | 状态 |
|---|---|---|---|
| 1. Counter 可终止线程模板 | thread 模板、expose、expose_mutable + locked、三个 hook、async handler | `tests/tss/thread_lifecycle.tss` + `tests/tss/handler_dispatch.tss` | ✅ |
| 2. Producer/Consumer Queue<String> | Queue.enqueue / dequeue（返回 Option）、close | `helloworld.tss`、`demo.tss` | ✅ |
| 3. Heartbeat 不可终止线程 | 无 `__on_terminate__`、async tick、asleep | 间接覆盖于 `helloworld.tss`；无独立 .tss | ⚠️ |
| 4. 主流程 handler 调用 + .wait() | sync 上下文等待 HandlerFuture | `tests/tss/handler_dispatch.tss` | ✅ |
| 5. `__ping__` 健康检查 | 隐式 handler `__ping__()` | 无（实现也未注入隐式 handler） | ❌ |
| 6. expose / locked<int> 读取 | 直接 `.expose`、locked.get/set | `thread_lifecycle.tss`、`locked_shared.tss` | ✅ |
| 7. 匿名线程 `${ ... }` | shorthand 语法 | 无独立 .tss；语法被 parser 单测覆盖 | ⚠️ |
| 8. async 函数 closeBusLater() | 顶层 async 函数 | 间接（handler_dispatch.tss 使用 keepalive） | ⚠️ |
| 9. try/await 错误处理 | try await + HandlerDispatchError | `scope_binding.tss`（仅 ScopeGone） | ⚠️ |
| 10. 终止后健康检查 | terminate + Err("TargetTerminated") 比较 | `thread_lifecycle.tss`（无 TargetTerminated kind 断言） | ⚠️ |


## 14. 已识别偏差

### 偏差 1：handler 与主体的"并发执行"

- **规范**：R-HANDLER-2 / R-CORE-SCHED-1，"同一线程同一时间只有一个执行点"。
- **实现**：`crates/tessera-interp/src/event_loop.rs:340-355` 用 `tokio::task::spawn_local` 把 handler body 与主体平行地排进同一 `LocalSet`。代码注释明言这是为了打破 `readLine` 等场景下"handler 等待主体写入"导致的死锁。
- **影响**：在 await 点之间确实仍保持原子性（单 OS 线程协作调度），但语义上 handler 与主体在 await 点之间可能交替推进，比"严格序列化"宽松。
- **建议**：要么在规范中显式承认"协作调度下，handler 与主体在挂起点之间交替"（R-HANDLER-3 已暗示），要么用单一执行队列改写 `dispatch_handler_inline`。

### 偏差 2：HandlerDispatchError 的 kind 命名

- **规范（多数文档）**：`TargetTerminated` / `TargetTerminating` / `TargetCrashed`。
- **规范（基础类型规范 §6 表 393）**：使用 `"TargetGone"` 表示已终止。
- **实现**：`crates/tessera-runtime/src/error.rs:198-207` 将 `TargetTerminated` 映射为 `"TargetGone"`。
- **影响**：用户写 `if (e.kind == "TargetTerminated") { ... }`（如 `example-code.md:284`）会永远为 false。
- **建议**：以"线程与事件循环规范"和"标准容器规范"为权威，把 kind 改回 `TargetTerminated`；或者修订《基础类型...》使其与其它规范一致。

### 偏差 3：i64 token → i32 Value 的窄化

- **规范 §1.1.2**：int 为 32 位有符号。
- **实现**：词法返回 `i64`（`lexer/src/token.rs:117`），运行时存为 `i32`（`runtime/src/value.rs:18`），转换用 `as i32` 截断（如 `builtin.rs:50` 的 `f as i32`）。
- **影响**：字面量 `2147483648`（超过 i32::MAX）会被静默截断而非报错。
- **建议**：在 parser 或类型检查阶段对超界字面量报错，或在 `as` 转换时检测 wrap。

### 偏差 4：String / List 跨线程使用的诊断

- **规范**：跨线程使用 List/Map 需要显式包 `locked<T>` 或 `Queue<T>`。
- **实现**：`Value::List(Rc<...>)` 不是 `Send`，跨线程使用会触发 Rust 编译/运行时错误（而非 Tessera 领域错误）。
- **影响**：错误信息不直观；缺乏 Linter 提示。
- **建议**：增加 Linter L-EXPOSE-MUTABLE-UNSAFE 已覆盖一半，但 expose（只读）暴露 List 也应给出 Info 级提示。

### 偏差 5（潜在）：scope `define` 字段外部不可见的运行时强制

- **规范 R-DEFINE-1**：`define` 字段只在 @template 内可见，不可经 ScopeBlock 外部访问。
- **实现**：scope 的 `define` 字段存在 template_self；ScopeBlock 退出后该对象被丢弃。但 `eval.rs` 中 scope 期间 `field access` 是否区分 expose vs define 未审计。
- **风险**：若运行时仅按字段名解析，可能允许从外部（更外层 scope）读到。
- **建议**：补全 R-DEFINE-1 反例测试 + Linter L-DEFINE-EXTERNAL-ACCESS。


## 15. 差距修复 TODO（P0 / P1 / P2）

### P0 — 语义正确性 / 测试可见的偏差

1. **统一 HandlerDispatchError kind**（§14 偏差 2）
   - 改 `runtime/src/error.rs:198-207`：`TargetTerminated → "TargetTerminated"`；或修订《基础类型...》文档；
   - 增加 `tests/integration.rs` 反例：`try await handler_call_after_terminate` 比较 kind。

2. **R-HANDLER-PING：注入隐式 `__ping__()`**
   - `types/src/checker/registration.rs`：在每个 thread template 的 `handlers` 中注册 `__ping__: () → String`；
   - `interp/src/eval.rs::find_handler` 或 `event_loop.rs::dispatch_handler_inline` 兜底返回 `"pong"`；
   - 新建测试 `tests/tss/ping_handler.tss`。

3. **int 字面量溢出诊断**（§14 偏差 3）
   - 在 parser 或 checker 中检测 `LitInt(i64)` 超出 `i32` 范围 → 报错。

4. **`define` 字段访问越界拒绝**（§14 偏差 5）
   - 增加从 ScopeBlock 外部读 define 字段的反例测试；
   - 必要时在 `eval.rs` 字段访问路径区分 expose vs define。

### P1 — 核心规则缺失

5. **顶层 await 拒绝**：L-AWAIT-EXPR-IN-TOPLEVEL 或 parser 阶段拒绝；
6. **return / break / continue 顶层禁止**：在 parser/checker 中识别 toplevel context 并拒绝（§10）；
7. **hook 签名约束**：L-FUNCTION-HOOK-SIGNATURE — `__on_enter__` / `__on_exit__` 必须为同步 void；`__on_terminate__` 必须为 async void；
8. **handler 结果丢弃语义**：L-HANDLER-RESULT-IGNORED + L-HANDLER-DISPATCH-ERROR-UNHANDLED；
9. **HandlerFuture 误用**：L-HANDLER-FUTURE-MISUSE 扩展 `handler_await_type` 覆盖 field/method-chain 接收者；
10. **匿名 `${...}` 测试覆盖**：补 `tests/tss/anonymous_thread.tss`；
11. **Broken 唤醒不早于 #exclusive 结束**（R-SYNC-BREAK-3）：补反例测试；
12. **char Unicode 转义 `\u{xxxx}`**：扩展 `lexer/src/token.rs::unescape`；
13. **String `[i]` 取字符**：扩展 `eval.rs::Expr::Index` 与 checker。

### P2 — 静态分析与可用性

14. **补齐 Linter 规则**（按 §12 缺失清单优先级）：
    - 优先：L-EXPOSE-READONLY-WRITE、L-DEFINE-EXTERNAL-ACCESS、L-DEFINE-IN-NON-TEMPLATE、L-TEMPLATE-APPLY-CONTEXT、L-AT-TEMPLATE-STACKING、L-RETURN-TYPE-MISMATCH、L-RETURN-NOT-ALL-PATHS、L-VOID-RETURN-VALUE；
    - 然后：signal/contract 的 await/wait 上下文检查（与已实现的 permit 镜像）；
    - 信息级：L-NAMING-THREAD-HANDLE、L-ASSERT-SIDE-EFFECT、L-PANIC-OVERUSE。
15. **标准容器扩展方法**：Map.contains、List.indexOf/contains/slice 等；String startsWith/endsWith/split/trim；
16. **泛型嵌套深度提示**：L-GENERIC-NESTING-DEPTH；
17. **Linter 反例测试**：为每个 pass 添加 `tests/lint_*.rs`，目前 `tessera-lint` 无独立测试入口；
18. **Example 5 / 7 / 8 的端到端测试**：补 `__ping__`、`${ }`、顶层 async function 的 .tss 测试；
19. **TargetTerminated kind 与 `Err("...")` 比较的内联测试**（与 P0-1 配套）。


## 16. 验证与维护

### 如何复核本报告

1. **代码定位**：报告中每条 `file:line` 在仓库可直接打开。可用 ripgrep 复核某条目，例如：
   - `rg "is_concurrent_safe" crates/` → 验证 §4 R-EXPOSE-2；
   - `rg "__on_terminate__" crates/tessera-interp/src/event_loop.rs` → 验证 §3 R-LIFE-2。
2. **集成测试断言**：在仓库根运行 `cargo test -p tessera-interp` 应全部通过；报告中标 ✅ 的条目对应的测试名可直接 grep 自 `crates/tessera-interp/tests/integration.rs`。
3. **Linter 实装核对**：`crates/tessera-lint/src/passes/mod.rs::all()` 共 9 个 `Box::new(...)`，与 §12 已实现表完全对应。
4. **示例核对**：用 `E:/Tessera-Spec/example-code.md` 与 `helloworld.tss` / `demo.tss` / `tests/tss/*.tss` 比对 §13。

### 维护建议

- **绑定到 PR 流程**：当 PR 触及解释器语义、新增 Lint pass、新增内建函数时，更新本报告对应章节（建议加入 PR Checklist）。
- **基线版本号**：报告顶部 commit 哈希需在每次主干合并后由作者更新（例如自动化脚本扫描 `git log -1 --format=%h`）。
- **规范变更**：若 `E:/Tessera-Spec/*.md` 发生修订（即使是行号变化），需复核标 ✅ 的条目是否还和文档一致。
- **TODO 跟踪**：建议把 §15 的 P0 / P1 / P2 拆为 GitHub Issue 或类似任务系统跟踪，每条 close 时同步更新本报告。
- **报告自身的测试**：可在 CI 中加 `cargo test -p tessera-lint --tests` 类似 smoke check，确保 9 个 pass 与本报告一致。

---

*本报告由 spec-alignment 审计于 commit `bbda9b1`（2026-05-29）生成；后续更新请基于本文件，避免重复审计。*

