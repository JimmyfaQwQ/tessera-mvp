# Tessera Spec Alignment 报告

> **当前基准**：tessera-mvp 方向 1 Round 4 / Tessera-Spec `41ae1c4`（2026-05-29）。`E:/tessera-mvp/` 实现 vs `E:/Tessera-Spec/` 规范的逐条对照。
> 覆盖：tessera-lexer / tessera-parser / tessera-ast / tessera-types / tessera-runtime / tessera-interp / tessera-lint。
> 测试：tessera-interp 47 integration + tessera-lint 59 smoke + tessera-types 9 typecheck + parser/lexer 各 5（合计 125）；Lint pass 29。`cargo build --workspace --all-targets` 无 warning；`--check helloworld.tss` / `demo.tss` 均无诊断。
>
> **状态徽标**：✅ 已实现且有测试 ／ ⚠️ 已实现但缺测试或行为未完全覆盖 ／ ❌ 未实现 ／ 🔶 实现与规范存在语义偏差。
>
> 本文档只反映**当前最终状态**。逐轮修复脉络（4 轮 spec-alignment + 方向 1 的 Round 1「R-SYNC-BREAK-3 净简化」/ Round 2「L-EXCL-AWAIT」/ Round 3「4 lint + 重复声明拒绝 + 测试缺口」/ Round 4「规则 refinement：删 panic-overuse、break/continue 越界、thread-body void-like return、参数/字段重名、模板 arity、assert 常量折叠、未消费/terminate Future、自由函数体检查、补齐 R-TRY-2/R-HANDLER-SCOPE/ParseError 测试」）保存在 git 历史中。

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
15. [仍开放的 TODO](#15-仍开放的-todo)
16. [验证与维护](#16-验证与维护)

## 0. 摘要表

> 下表反映**当前状态**。逐条状态见 §1–§13；仍开放的缺口集中在 §14（偏差）与 §15（TODO）。

### 三大维度总体对齐

| 维度 | 当前状态 | 仍开放的主要短板 |
|---|---|---|
| 解释器（lexer + parser + ast + runtime + interp） | 高度对齐 | List/Map 跨线程使用缺领域级错误信息（§14 偏差 1，需 runtime 改造）|
| 类型系统（tessera-types） | 大体对齐 | **用户函数泛型未实现**（`function f<T>(...)`，§11）。自由函数体类型检查、模板参数/字段重名已在 Round 4 补齐 |
| Linter（tessera-lint） | 29 个 pass | 剩余未实现的 L 规则需数据流/CFG 分析（option/result unwrap 可空性等）或前提不成立（no-await 系）——已在 §12 逐条记录处置 |

> 说明：spec-alignment 四轮 + 方向 1 Round 1/2/3/4 已落地全部 P0、绝大多数 P1。Round 4 按"wildcard 规则需 refinement"原则：删除纯计数的 L-PANIC-OVERUSE；新增 L-CONTROL-OUTSIDE-LOOP、L-AT-TEMPLATE-PARAM-MISMATCH、L-ASSERT-ALWAYS-TRUE/FALSE、L-AWAIT-UNCONSUMED-FUTURE、L-TERMINATE-FUTURE-IGNORED；thread-body return 改为 void-like；补齐参数/字段重名拒绝、自由函数体类型检查、R-TRY-2/R-HANDLER-SCOPE/ParseError 测试。§1–§13 各表已逐行更新到当前状态。


## 1. Tessera 核心语义

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| R-TYPE-STATIC-1 | `let` 决定变量类型；后续赋值必须类型兼容 | ✅ | `crates/tessera-types/src/checker/stmts.rs:13-35` | 内联测试 `test_arithmetic` 隐含覆盖 |
| R-CORE-MAIN-1 | 主体执行完毕 → 线程自动终止（不调 `__on_terminate__`） | ✅ | `crates/tessera-interp/src/event_loop.rs:226-265` | `__on_exit__` 单独执行；`__on_terminate__` 不会被触发 |
| R-CORE-TERM-1 | `Terminated` 后不再执行 handler、不再修改 expose | ✅ | `crates/tessera-runtime/src/thread_state.rs:170-188` | `dispatch_handler` 立刻返回 `TargetTerminated` |
| R-CORE-HANDLER-1 | `Terminated`/`Crashed` 后 handler future 不得永久挂起 | ✅ | `event_loop.rs:252,259,289` + `drain_handlers` | 队列在状态切换时统一 drain |
| R-CORE-SCHED-1 | 单一执行点；同步段直至挂起点不可被抢占 | ✅ | `event_loop.rs:186-306`（tokio `select!` 配合 `biased`）| 单线程 `LocalSet` 协作调度 |
| R-CORE-SCHED-2 | 同一线程的 handler 请求 FIFO；不并发执行 | ✅ | `event_loop.rs:300-304`（mpsc + 单点 select）+ handler-in-flight gate（`ThreadState.handler_in_flight: watch<bool>`）| FIFO + handler 互斥由 gate 保证（P0-3）；规范 A-2 已澄清"主体↔handler 在挂起点交替"合法 |
| R-CORE-EXCL-1 | `#exclusive` 块独占执行；不交错 handler/timer/IO | ✅ | `event_loop.rs:300, 345-350` | 主路径 + handler 任务双重等待 |
| R-CORE-SHARE-1 | 子线程不可直接读写父线程局部变量 | ✅ | `event_loop.rs:37-120`（`current_thread_state` 与 env 切换；线程 body 使用新建 env）| 没有跨线程 env 共享；测试 `thread_lifecycle.tss` 隐含验证 |
| R-CORE-SHARE-2 | 不存在"只读直接引用"父局部变量的形式 | ✅ | parser 无此语法；runtime 无对应 Value 变体 | 反例不可表达 |


## 2. 模板与线程规范

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| 模板基本语法 `@template` / `$template` | lexer + parser + AST | ✅ | `lexer/src/token.rs:9-21`、`parser/src/parser.rs:205-360`、`ast/src/lib.rs`（ScopeTemplateDecl / ThreadTemplateDecl）| 命名 + 匿名两种形式都已支持 |
| 匿名简写 `${ ... }` | 创建不可终止匿名线程 | ✅ | `parser.rs:572-592`、`event_loop.rs`（无 decl 路径）| 端到端测试 `tests/tss/anonymous_thread.tss`（用 `__ping__` 探活）|
| Terminatable 判定（声明 `async function __on_terminate__()`）| 自动识别 | ✅ | `parser.rs:300`、`types/src/checker/registration.rs`、`runtime/src/thread_state.rs:42,93` | `is_terminatable` 字段贯穿 |
| R-TEMPLATE-STACKING：同一块不可叠加多个 `@template` | 语法层禁止 | ✅ | `parser.rs:457-465`：scope_block 一次只能有一个 `@template`/`@Name`；语法不允许重复 | 无显式 lint，但语法不可表达 |
| 线程模板成员：`expose` / `expose_mutable` / `define` / `async handler` / 三个生命周期 hooks / 普通 method | 全部支持 | ✅ | `parser.rs:270-360`、`ast/src/lib.rs`（ThreadTemplateMember） | 句法/AST 完整 |
| Scope 模板成员：`define` + `__on_enter__` / `__on_exit__` + member 函数（无 handler、无 expose）| 严格限制 | ✅ | `parser.rs:228-243` | scope body 不接受 `expose`/`handler` 关键字 |
| 线程句柄绑定 `:= h` 与离开作用域不自动终止 | 语义 | ✅ | `parser.rs:616-622`、句柄是 `Arc<ThreadState>`，不绑定生命周期 | 离开作用域只丢弃 `Arc`，不触发 terminate |
| Self 与模板参数可见性 | `self.fieldName`、`self.paramName` | ✅ | `event_loop.rs:115-120`、`eval.rs`（field access）；`checker/registration.rs::check_self_namespace_collisions` 拒绝参数/字段重名 | 参数与字段共享 `self.` 命名空间且必须互不相同（§2 规则）；测试 `typecheck.rs` 三例（param↔field / define↔param 拒绝 + distinct 允许）|
| 模板参数错误（arity 不匹配）→ 线程崩溃 | 实现层防御 | ✅ | `event_loop.rs:102-114` | 不静默吞 |
| 启动位置：spawn 只能作语句、不能嵌入表达式 | 句法约束 | ✅ | parser 无 `$` 表达式 primary（`Token::Dollar` 仅在 `parse_stmt`/prebind 消费）；§4.7 明确允许 spawn 出现在任意**语句**位置 | L-TEMPLATE-APPLY-CONTEXT 无可 lint 的 AST 目标：把 spawn 写进表达式是 parse 错误，由 parser 层强制。故跳过该 lint（见 §12 未实现表）|


## 3. 线程与事件循环规范

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| R-SCHED-1 | 协作调度；仅在 await/return/error 处挂起 | ✅ | tokio LocalSet + spawn_local | 单 OS 线程协作 |
| R-SCHED-2 | 同步段一旦开始执行到下个挂起点不可被打断 | ✅ | `event_loop.rs:186-306` 主 select 与 handler 任务通过 watch 通道同步 | |
| R-LIFE-1 | 主体自然结束 → 自动 Terminated，不调用 `__on_terminate__` | ✅ | `event_loop.rs:226-265`：只调用 `__on_exit__` | 内联测试覆盖 |
| R-LIFE-2 | 显式 `terminate()` 才会触发 `__on_terminate__` 然后 `__on_exit__` | ✅ | `event_loop.rs:199-223`（先 `run_teardown_hooks(.., true)`）；helper 在 `run_teardown_hooks:384-402` 顺序执行 `__on_terminate__` → `__on_exit__` | `thread_lifecycle.tss` 覆盖 |
| R-HANDLER-1 | handler FIFO（按线程实例）| ✅ | `mpsc::Receiver` 顺序消费 | |
| R-HANDLER-2 | 同线程不并发执行两个 handler | ✅ | handler-in-flight gate（`ThreadState.handler_in_flight: watch<bool>` + 主 select gate + `dispatch_handler_inline` 设/释放）| P0-3；测试 `tests/tss/handler_mutex.tss` 用时间戳序列断言无交错。规范 A-2：handler↔handler 严格互斥，body↔handler 挂起点交替合法 |
| R-HANDLER-3 | 主体 ↔ handler 仅在挂起点切换 | ✅ | 协作调度自然满足 | |
| R-HANDLER-4 | Terminating/Terminated/Crashed 拒绝新 handler；队列项以调度失败结束 | ✅ | `thread_state.rs:170-188` + `event_loop.rs::drain_handlers` | 三种状态映射到三个 dispatch error |
| R-HANDLER-5 | 主体结束后不再执行 handler | ✅ | `event_loop.rs:252,259,289` 主体完成路径 drain 队列 | |
| R-HANDLER-PING | 所有线程模板隐式具有 `async handler __ping__(): String` 返回 "pong" | ✅ | `registration.rs` 自动注册 + `thread_state.rs::dispatch_handler` 入口特判；不进队列、不受 R-HANDLER-2 / `#exclusive` 约束 | P0-2；测试 `tests/tss/ping_handler.tss`；用户重写触发 L-HANDLER-PING-REDEFINED |
| R-HANDLER-SCOPE | handler 只能访问模板对象，不可访问外部作用域 | ✅ | `event_loop.rs:39,120` + 创建 handler body 时使用模板 env | 测试 `test_handler_cannot_access_outer_scope`：handler 引用顶层局部 `outer` 在运行时失败（被 `try await` 捕获为 Err），证明不泄漏外部作用域 |
| R-EXCL-1 | `#exclusive` 内独占线程；handler/timer/IO 不交错 | ✅ | `event_loop.rs:300 + 345-350`（select gate + handler 任务 watch 等待）| `exclusive_block.tss` 覆盖 |
| R-EXCL-2 | `#exclusive` 阻塞 handler 进入，但不阻塞入队 | ✅ | mpsc 通道一直接收；只在主 select 上 gate | |
| R-EXCL-3 | 在 `#exclusive` 期间收到 terminate → 立即转 Terminating，teardown 延后 | ✅ | `event_loop.rs:157-184, 199-223, 270-294` | 代码注释明确引用 R-EXCL-3 |
| R-EXCL-4 | `#exclusive` 内 await 依赖本线程调度 → 死锁（告知性约束） | ✅（sound 子集） | `tessera-lint/src/passes/exclusive_self_primitive_await.rs`（L-EXCL-AWAIT, Warn）命中 `#exclusive` 内 await/`.wait()` 自有同步原语 | 后续立项 Round 2 完成；完整判定不可静态化，仅落地 sound 子集 |
| R-KEEPALIVE-1 | `keepalive()` 返回永不完成 Future | ✅ | `eval.rs::keepalive` = `std::future::pending::<()>().await`（永不 resolve）| 测试 `tests/tss/keepalive_never_returns.tss`：`await keepalive()` 后的赋值永不执行（`reached` 恒为 false），terminate() 仍可清理 |
| R-KEEPALIVE-2 | `keepalive()` 主体不参与 terminate；terminate 由 hook 驱动 | ✅ | `event_loop.rs:226+` 主体即使永远不返回，terminate 通过 select 路径触发 teardown | `handler_dispatch.tss` 使用 keepalive 隐含覆盖 |


## 4. 数据共享与并发安全规范

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| R-CORE-SHARE-1 / 2 | 子线程不可直接访问父线程局部变量 | ✅ | 见 §1 | |
| R-CORE-SHARE-3 | 通过参数传递初值；避免跨线程存储共享 | ✅ | `event_loop.rs:99-117`（参数复制进入 template_self）| |
| R-CORE-SHARE-4 | 跨线程访问线程状态 → 通过 expose / handler | ✅ | `thread_state.rs:47-50,164-189` | |
| R-CORE-SHARE-5 | 多线程共享数据 → 并发安全类型（`locked<T>`/`Queue<T>`）| ✅ | `runtime/src/locked.rs`、`runtime/src/queue.rs` | |
| R-EXPOSE-1 | 只读共享 = `expose`；写入只能通过 handler 或并发安全类型 | ✅ | `parser.rs:272`、`event_loop.rs:67-69` 区分两种 expose；`tessera-lint/src/passes/expose_readonly_write.rs`（L-EXPOSE-READONLY-WRITE）| |
| R-EXPOSE-2 | `expose_mutable` 字段类型必须并发安全 | ✅ | `tessera-lint/src/passes/expose_mutable_unsafe.rs:8-31` + `types/src/ty.rs:47-49` | L-EXPOSE-MUTABLE-UNSAFE pass 实现 |
| R-EXPOSE-3 | `expose_mutable` 字段引用不可被外部替换，只可通过其方法改内容 | ✅ | `eval.rs:227-237`：经线程句柄写 expose_mutable 字段时抛 `ExposeMutableFieldReplace` | 测试 `tests/tss/expose_mutable_replace.tss` + `test_expose_mutable_field_replace_rejected` 断言该错误变体 |
| R-DEFINE-1 | `define` 字段仅在模板内部可见，不可经线程句柄访问 | ✅ | runtime 天然隔离（FieldAccess 只查 expose_fields / expose_mutable_fields）+ `passes/define_external_access.rs`（L-DEFINE-EXTERNAL-ACCESS）| P0-5；测试 `test_define_field_external_invisible` |
| R-DEFINE-2 | `define` 在 @template / $template 都可声明；`expose`/`expose_mutable` 仅在 $template | ✅ | parser 严格限制 scope template 成员（`parser.rs:228-243`） | |
| R-TERMINATE-STABLE-1 | terminatable 线程 `terminate().wait()` 后 expose 字段稳定 | ✅ | `event_loop.rs:198-223` 主路径将 expose 同步固化（不再有 handler/body 写）| |
| R-TERMINATE-STABLE-2 | 非 terminatable 线程无稳定态语义 | ✅ | 无 terminate() 入口；自然结束触发 §3 R-LIFE-1 | |
| 跨线程引用类型限制 | List / Map 不可跨线程共享（Rc 而非 Arc） | 🔶 | `runtime/src/value.rs:25-26`：`List(Rc<...>)`、`Map(Rc<...>)` | 与规范要求一致；只读 `expose` 非并发安全容器现有 Info 提示 `L-EXPOSE-READONLY-CONTAINER`（passes/expose_readonly_container.rs）。底层 Rc 非 Send 的领域错误信息仍未细化（见 §14 偏差 1）|


## 5. async / await 规范

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| §1.1 / §1.2 | `await` 仅在 async 函数 / handler 内 | ✅ | `tessera-lint/src/passes/await_async_only.rs` + `types/src/checker/exprs.rs:71-85`（context flag）| L-AWAIT-ASYNC-ONLY pass |
| §1.3 | async 函数声明类型为"解包后类型"，调用返回 `Future<T>` | ✅ | `types/src/checker/registration.rs`（注册时包裹 Future）+ `eval.rs:737-754`（调用立即返回 Future）| |
| §2.1 | `.wait()` 同步阻塞；`await` 协程挂起 | ✅ | `builtin.rs:148-153`、`eval.rs::Expr::Await` | |
| §2.2 | `HandlerFuture` 的 wait 语义类比 Future | ✅ | `builtin.rs:157-174`、`runtime/src/future.rs:97-219` | 区分 Dispatch/Execution 失败 |
| §2.4 | `signal` / `contract` / `permit` 可直接 await | ✅ | `types/src/checker/exprs.rs:71-85` 把它们识别为 awaitable | `scope_binding.tss` 覆盖 signal |
| 顶层 await 限制 | 顶层不可写 `await expr;` | ✅ | `passes/toplevel_control_flow.rs`（L-TOPLEVEL-CONTROL-FLOW）禁止顶层 `await`/`return` | P1-1 |
| async 函数若全程无 await（信息提示） | L-ASYNC-NO-AWAIT 信息级提示 | ❌（已评估，跳过）| 无对应 pass | 合法的「无 await async 函数」无法与「冗余 async」静态区分：`await run()` 要求 `run` 为 async 即便其体无 await（helloworld `reader.run` 即如此）。即便降为 Info 也会误伤惯用写法并破坏 `--check` 干净保证，故跳过（需全程调用图分析才能 sound）|


## 6. 错误与异常语义

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| R-TRY-1 | `try expr` 捕获运行时错误 → `Result<T, error>`；不崩溃 | ✅ | `parser.rs:816-821`、`eval.rs`（Try 分支）+ `runtime/src/error.rs:157-185`（`kind_and_message` 转换）| `scope_binding.tss` 覆盖 |
| R-TRY-2 | `try await expr` = `try (await expr)`，仅 async 上下文 | ✅ | parser 中 `try`/`await` 组合解析为 `try (await ..)`；内层 `await` 的 async 上下文由 L-AWAIT-ASYNC-ONLY 守护 | 测试 `test_try_await_yields_result`（async 上下文得 Ok）+ smoke `try_await_in_sync_function_is_rejected` |
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
| HandlerDispatchError kind 命名一致性 | "TargetTerminated" / "TargetTerminating" / "TargetCrashed" | ✅ | `runtime/src/error.rs:198-207` 映射为 `"TargetTerminated"`；规范《基础类型...》§6 表同步改正 | P0-1 / A-1；测试 `test_target_terminated_kind` |


## 7. 同步原语与崩溃传播规范

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| R-SYNC-OWN-1 | 每个原语最多 1 个线程绑定主（首次 expose 获得） | ✅ | `runtime/src/signal.rs:38-51, 151`（`Ownership` + `try_claim_ownership`）；contract/permit 同样持有 | `try_claim` 在 expose 路径调用 |
| R-SYNC-OWN-2 | 未绑定的原语永不进入 Broken；wait 不失败 | ✅ | break 仅在 thread Terminated/Crashed 时触发，未绑定即未注册 owned | |
| R-SYNC-OWN-3 | @template `define` 的原语 = 作用域绑定，不随线程传递 | ✅ | `signal.rs:13-16`（`ScopeGone`/`ScopeCrashed` 变体） + 触发于 scope 退出路径 | `scope_binding.tss` 覆盖 ScopeGone |
| R-SYNC-BREAK-1 | 线程绑定 → 仅在 owner 进入 `Terminated`/`Crashed` 时 Broken；`Terminating` 不触发 | ✅ | `thread_state.rs:111-127`：`set_status(Terminated/Crashed)` 才 `break_with` | |
| R-SYNC-BREAK-2 | owner 在 terminate 之前崩溃 → 已经 Broken | ✅ | 同上；Crashed 路径覆盖 | |
| R-SYNC-BREAK-3 | `Broken` 唤醒处于 `#exclusive` 中的等待者 = 恢复独占协程自身续体（R-EXCL-1 保证原子性，无需延迟交付）；块内已成功的等待不被回溯 | ✅ | Broken 即时交付（`eval.rs`/`builtin.rs` 的 signal/contract/permit 分支）；规范 §3.3 删去"延迟到块结束"冗余子句，删除 best-effort 拐杖 `delay_broken_until_exclusive_ends` | 后续立项 Round 1 完成；测试 `exclusive_broken_wait.tss` + `exclusive_broken_success_not_reverted.tss` |
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
| §1.1.2 int | 32-bit signed；算术 wrap-around + 比较 | ✅ | 值 `Value::Int(i32)`；算术用 `wrapping_*`；超界字面量 parser 报错 | P0-4 + 规范 B-1（wrap-around 定稿）；测试 `test_int_literal_overflow` |
| §1.1.3 double | IEEE754 64-bit | ✅ | `Value::Double(f64)`、`lexer LitDouble(f64)` | |
| §1.1.4 char | 单 Unicode scalar；转义 | ✅ | `lexer/src/token.rs::unescape`（`\n \t \r \" \' \\ \0` + `\u{HHHH}`）| P1-4；规范 B-2 转义表 |
| §1.1.5 String | 不可变；`+` 拼接（隐式转换）；`.length()`；`[x]` 取字符 | ✅ | `eval.rs` Add 支持 String + others；`builtin.rs`（`length`）；String Index → char | P1-4；测试 `test_string_indexing_and_unicode` / `test_string_index_out_of_bounds` |
| §1.2.1 never | 表达式底类型 | ✅ | `ty.rs::Type::Never`、`Value::Never`、parser `KwNever` | `panic` 返回类型 never |
| §1.3.1 panic | `panic(msg): never` | ✅ | §6 已覆盖 | |
| §1.3.2 assert | 失败 → AssertionFailed | ✅ | §6 已覆盖 | |
| §1.3.4 try expr | 捕获 → Result<T, error> | ✅ | §6 已覆盖 | |
| §2.1 求值顺序 | 子表达式从左到右 | ✅ | `eval.rs::eval_binop` 先 eval left 再 right；method args 顺序循环 | |
| §2.1 短路 | `&&` / `||` | ✅ | `eval.rs:585-594`：`&&` LHS 假即返回、`||` LHS 真即返回，均不求值 RHS | 反例测试 `test_short_circuit_and_skips_rhs` / `..or..`：RHS 为 `1/0` 不触发 DivisionByZero |
| §2.2 运算符优先级 | 一元 > * / % > + - > 比较 > ==/!= > && > || | ✅ | `parser.rs:964-973`（Pratt binding power）+ unary 在 primary 前 | 单元测试 `pratt_precedence` |
| §3.1 `let name = expr;` / `let name: T = expr;` | 变量声明必须初始化 | ✅ | `parser.rs:480-488` + `checker/stmts.rs:13-20` | |
| §3.2 禁止未初始化声明 | `let x: int;` 禁止 | ✅ | parser 要求 `=` 否则报错 | |
| §4.1.2 非 void 函数所有路径 return | 静态检查 | ✅ | `passes/return_not_all_paths.rs`（L-RETURN-NOT-ALL-PATHS）| P1 新增 |
| §4.1.2 void 函数允许隐式末尾 return | | ✅ | `checker/bodies.rs` 实现 | |
| §4.2 async 函数返回类型自动包 `Future<T>` | | ✅ | `registration.rs` 注册时包裹 | |
| §4.3 hooks 签名约束（`__on_enter__`/`__on_exit__` 是同步 void；`__on_terminate__` 是 async void）| | ✅ | `passes/hook_signature.rs`（L-FUNCTION-HOOK-SIGNATURE）| P1-2 |
| 函数返回类型不匹配 | 静态检查（sound 子集）| ✅ | `passes/return_type_mismatch.rs`（L-RETURN-TYPE-MISMATCH, Error）| 命中：非-void 单元的裸 `return;`；标量字面量返回与声明类型确定不符（int⇆double 保守跳过）。依赖推断的返回交由类型检查器，保零假阳性 |
| void 函数返回值表达式 | 静态检查 | ✅ | `passes/void_return_value.rs`（L-VOID-RETURN-VALUE, Error）| `void` 单元出现 `return <expr>;` 即报 |


## 9. 标准容器与常用类型规范

| 类型 | 规范方法 | 实现位置 | 状态 |
|---|---|---|---|
| `List<T>` | length、isEmpty、push、pop、get、set、indexing | `builtin.rs:41-109`、`Value::List(Rc<RefCell<Vec<Value>>>)` | ✅ |
| `Map<K, V>` | size、get、set、remove、contains | `builtin.rs`；构造需 `Map<K,V>(...)` 显式类型参 | ✅ contains 已加（round 3）；keys/values/forEach 仍缺（需一等函数，见 §14/backlog）|
| `Option<T>` | Some/None、isSome、isNone、unwrap、unwrapOr | `builtin.rs:22-28`、TypeCtor 路径 | ✅ |
| `Result<T, E>` | Ok/Err、isOk、isErr、unwrap、unwrapErr、unwrapOr | `builtin.rs:30-38` | ✅ |
| `HandlerDispatchError` | TargetTerminated/Terminating/Crashed 三个变体 + 字符串比较 | `runtime/src/error.rs:188-207` | ✅（kind 命名 P0-1 已统一）|
| `HandlerFuture<R>` | wait/waitHandler、isDone、isOk、isErr、与 `Err("Xxx")` 比较 | `runtime/src/future.rs:97-219`、`builtin.rs:157-174` | ✅ |
| `Queue<T>` | push/enqueue/tryPush/tryPop/dequeue/size/isEmpty/isClosed/waitForNonEmpty/close；capacity ≤ 0 = 无界 | `runtime/src/queue.rs:1-106`、`builtin.rs:200-229` | ✅ |
| `locked<T>` | lock/tryLock/unlock/isLocked/get/set；显式 + 隐式两接口 | `runtime/src/locked.rs` | ✅ |
| `signal` / `contract` / `permit` | 已在 §7 覆盖 | `runtime/src/signal.rs` | ✅ |
| `ParseError` | `String.toInt() / toDouble()` 返回 | `builtin.rs:153-163`：`Result<T, Value::Str(...)>` | ✅ 规范《错误与异常语义草案 §0.x》明确定义 ParseError 的运行时表示**就是字符串**（`.unwrapErr()` 取出消息），故现实现已对齐；测试 `test_parse_error_is_string_payload` |
| `error`（`.kind` + `.message`）| `try` 表达式结果 | `runtime/src/value.rs:47`、`error.rs::kind_and_message` | ✅ |
| `Future<T>` | wait、isDone | `builtin.rs:148-154` | ✅ |
| 字符串方法（startsWith / endsWith / contains / indexOf / trim / split）| 标准容器规范 §13 | `builtin.rs` | ✅（round 3 加 6 方法）|
| `List.contains / indexOf / clear` 扩展方法 | 标准容器规范扩展接口 | `builtin.rs` | ✅（round 3）；`slice` / `forEach`（需一等函数）仍缺 |
| 自定义构造器（List<int>() 元素列表）| `List<int>(1,2,3)` | `parser.rs:851-882`、`eval.rs::TypeCtor` | ✅ |


## 10. 语句与控制流规范

| 条目 | 描述 | 状态 | 实现位置 | 备注 |
|---|---|---|---|---|
| `let` 必须初始化 | | ✅ | `parser.rs:480-488` | |
| 变量可在嵌套块中遮蔽，同块内重复声明禁止 | | ✅ | 遮蔽由嵌套 env 实现；`checker/stmts.rs::check_dup_lets`（接入 `check_block` / scope 块 / Pass 4 顶层）拒绝同块 let-vs-let 重复 | 测试 `tessera-types/tests/typecheck.rs`：拒绝重复 + 三个 must-not-fire（嵌套遮蔽 / 兄弟块 / let 遮蔽参数）+ 自由函数体内重复（Round 4 起 Pass 3 也检查顶层自由函数体，见 `duplicate_let_in_free_function_body_is_rejected`）|
| §2.2 赋值 RHS 类型须匹配 | | ✅ | `checker/stmts.rs:21-35` | |
| §3.1 块作用域：`let` 离开块即失效 | | ✅ | env 在 block enter/exit 推/弹 | |
| §3.2 `@template` 修饰块 hooks 顺序 | `__on_enter__` 前置，`__on_exit__` 后置 | ✅ | `eval.rs::exec_scope_block` | `scope_hooks.tss` 覆盖 |
| §4.2 `if` 条件必须 bool | | ✅ | `checker/stmts.rs:37-55` | |
| §5.1 `while`：break/continue 支持 | | ✅ | `parser.rs:509-517` + `eval.rs::exec_while` | |
| §5.2 `for(init; cond; update)`：循环变量作用域局限 | | ✅ | `parser.rs:519-543` + scoped env | |
| §6.1 / §6.2 break / continue | 仅在循环内 | ✅ | `passes/break_continue_outside_loop.rs`（L-CONTROL-OUTSIDE-LOOP, Error）静态拒绝任意上下文的循环外 break/continue | 循环上下文在函数/handler/spawn 边界重置，对 if/scope/#exclusive 透明；测试 4 例 |
| §7 return 语义（线程 body）| void-like：裸 `return;` 合法（=自然终止），`return expr;` 禁止 | ✅ | `passes/void_return_value.rs` 扩展：thread spawn body 顶层带值 return → L-VOID-RETURN-VALUE | 规范《语句与控制流 §9》改为 void-like；测试 `return_value_from_thread_body_is_rejected` / `bare_return_in_thread_body_is_ok` |
| §8.1 顶层禁止 return/break/continue | | ✅ | `passes/toplevel_control_flow.rs`（L-TOPLEVEL-CONTROL-FLOW）：顶层 `await`/`return` 禁止，`break`/`continue` 仅循环内合法 | P1-1 |
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
| 嵌套深度提示 | L-GENERIC-NESTING-DEPTH | ✅ | `passes/generic_nesting_depth.rs`（Info）：深度 = 叶 1 / 构造器 `1+max(参数)`，> 3 触发 | 测试 `deeply_nested_generic_is_flagged`（深度 5）+ `shallow_generic_is_ok`（深度 3 不报）|


## 12. Linter 规则对照

实现注册在 `crates/tessera-lint/src/passes/mod.rs::all()`，当前共 **29** 个 pass（权威列表以 `mod.rs::all()` 为准）。

### 已实现（29）

| 规则 ID | 文件 | 检查内容 | 状态 |
|---|---|---|---|
| L-AWAIT-ASYNC-ONLY | `passes/await_async_only.rs` | `await` 仅在 async 函数 / handler 内 | ✅ |
| L-HANDLER-AWAIT-TYPE | `passes/handler_await_type.rs` | `.waitHandler()` 不可用于 Future（链式接收者经 ScopedTyper 解析）| ✅ |
| L-EXPOSE-MUTABLE-UNSAFE | `passes/expose_mutable_unsafe.rs` | `expose_mutable` 字段类型必须并发安全 | ✅ |
| L-GENERIC-TYPE-ARG-MISSING（+ COUNT）| `passes/generic_type_arg_missing.rs` | 容器泛型类型参个数检查 | ✅（兼覆盖 wrong-count）|
| L-TERMINATE-NON-TERMINATABLE | `passes/terminate_non_terminatable.rs` | `.terminate()` 仅对 terminatable 线程 | ✅ |
| L-PERMIT-AWAIT-IN-SYNC | `passes/permit_await_in_sync.rs` | sync 上下文中不可 `.awaitPermit()` | ✅ |
| L-PERMIT-WAIT-IN-ASYNC | `passes/permit_wait_in_async.rs` | async 上下文中用 `.wait()` 警告 | ✅ |
| L-PERMIT-RELEASE-NON-POSITIVE | `passes/permit_release_non_positive.rs` | `permit(initial)` ≥ 0，`release(n)` > 0 | ✅ |
| L-EXCL-AWAIT | `passes/exclusive_self_primitive_await.rs` | `#exclusive` 内 await/`.wait()` 自有同步原语（R-EXCL-4 sound 子集）| ✅ Warn |
| L-EXPOSE-READONLY-WRITE | `passes/expose_readonly_write.rs` | `expose`（只读）字段不可外部写 | ✅ |
| L-DEFINE-EXTERNAL-ACCESS | `passes/define_external_access.rs` | `define` 字段不可经句柄外部访问 | ✅ |
| L-TOPLEVEL-CONTROL-FLOW | `passes/toplevel_control_flow.rs` | 顶层禁 `await`/`return`（`break`/`continue` 移交 L-CONTROL-OUTSIDE-LOOP）| ✅ |
| L-CONTROL-OUTSIDE-LOOP | `passes/break_continue_outside_loop.rs` | `break`/`continue` 仅在循环内（任意上下文）| ✅ Error |
| L-FUNCTION-HOOK-SIGNATURE | `passes/hook_signature.rs` | hook 签名约束（sync/async void）| ✅ |
| L-HANDLER-RESULT-IGNORED | `passes/handler_result_ignored.rs` | 裸 handler 调用语句报警 | ✅ Warn |
| L-HANDLER-PING-REDEFINED | `passes/handler_ping_redefined.rs` | 用户重定义 `__ping__` 报错 | ✅ |
| L-RETURN-NOT-ALL-PATHS | `passes/return_not_all_paths.rs` | 非 void 函数所有路径须 return | ✅ |
| L-RETURN-TYPE-MISMATCH | `passes/return_type_mismatch.rs` | 非-void 单元裸 `return;` + 标量字面量返回类型不符（sound 子集）| ✅ Error |
| L-VOID-RETURN-VALUE | `passes/void_return_value.rs` | `void` 单元出现 `return <expr>;` | ✅ Error |
| L-EXPOSE-READONLY-CONTAINER | `passes/expose_readonly_container.rs` | 只读 `expose` 非并发安全容器（List/Map）| ✅ Info |
| L-GENERIC-NESTING-DEPTH | `passes/generic_nesting_depth.rs` | 泛型嵌套深度 > 3 | ✅ Info |
| L-AT-TEMPLATE-PARAM-MISMATCH | `passes/template_param_mismatch.rs` | 模板应用实参数 ∉ [min..=max] | ✅ Warn |
| L-ASSERT-ALWAYS-TRUE / L-ASSERT-ALWAYS-FALSE | `passes/assert_const_condition.rs` | assert 条件字面量常量折叠为 true/false | ✅ Warn |
| L-AWAIT-UNCONSUMED-FUTURE | `passes/await_unconsumed_future.rs` | 裸语句丢弃 Future/HandlerFuture（async 调用 / Future 方法）| ✅ Warn |
| L-TERMINATE-FUTURE-IGNORED | `passes/terminate_future_ignored.rs` | 裸 `handle.terminate();` 丢弃 teardown Future | ✅ Warn |
| L-SIGNAL-AWAIT-IN-SYNC | `passes/signal_await_in_sync.rs` | sync 上下文 `await <signal>` | ✅ |
| L-SIGNAL-WAIT-IN-ASYNC | `passes/signal_wait_in_async.rs` | async 上下文 `.wait()` on signal | ✅ Warn |
| L-CONTRACT-AWAIT-IN-SYNC | `passes/contract_await_in_sync.rs` | sync 上下文 `await <contract>` | ✅ |
| L-CONTRACT-WAIT-IN-ASYNC | `passes/contract_wait_in_async.rs` | async 上下文 `.wait()` on contract | ✅ Warn |

### 未实现（按规范 `Linter 规则草案.md` 列举）

说明：标「跳过」者经评估**不可 sound 静态化**或**已由更早的层（parser/语法）强制**，故不单列 pass；理由随行。其余为需数据流/常量折叠分析的后续项。

| 类别 | 规则及处置 |
|---|---|
| async/await | L-ASYNC-NO-AWAIT (Info) — **弃**：合法的「无 await async 函数」（如 `await run()` 需 `run` 为 async）与冗余 async 不可静态区分，会误伤且破坏 `--check` 干净；L-AWAIT-EXPR-IN-TOPLEVEL — 顶层场景已由 L-TOPLEVEL-CONTROL-FLOW 覆盖 |
| expose / define | L-EXPOSE-MUTABLE-ACCESS-PATTERN (Info) — 需用法流分析；L-DEFINE-IN-NON-TEMPLATE — **跳过**：`define` 仅在模板成员位置消费，他处为 parse 错误，parser 层已强制 |
| handler | L-HANDLER-MUST-ASYNC — **语法层强制**（无 pass，曾有空占位已删）；L-HANDLER-NO-AWAIT (Info) — **弃**（前提"无 await⇒冗余"不成立，会误伤 demo `status` 并破坏 `--check`）；L-HANDLER-CALL-SITE-AWAIT (Info)、L-HANDLER-DISPATCH-ERROR-UNHANDLED (Info)、L-HANDLER-FUTURE-MISUSE (Error；`L-HANDLER-AWAIT-TYPE` 已覆盖反方向) — 需流分析 |
| template | L-TEMPLATE-APPLY-CONTEXT — **跳过**：parser 无 `$` 表达式 primary，spawn 只能作语句（§4.7 许可所有语句位置），嵌入表达式即 parse 错误；L-AT-TEMPLATE-STACKING — 语法已防御 |
| option/result | L-OPTION-UNWRAP-POSSIBLE-NONE、L-RESULT-UNWRAP-POSSIBLE-ERR — **弃**：直接 `.unwrap()` 表示作者已知晓 None/Err 风险；有用版需数据流/CFG，零FP版（字面量 `None.unwrap()`）无价值。L-OPTION-PATTERN-USE 同理 |
| sync 原语 | L-SYNC-EXPOSE-OWNER (Info)、L-SYNC-AWAIT-NOCHECK、L-SYNC-SCOPE-DEFINE-PASS (Warn)、L-SYNC-TRIGGER-AFTER-BROKEN (Info) — 需 owner/flow 追踪 |
| panic / assert | L-ASSERT-SIDE-EFFECT — **弃**：Tessera 赋值是语句非表达式（`count++`/`x=5` 不可表达于 assert 条件），仅余调用、纯/非纯不可判定，会误伤 `assert(list.isEmpty())`；L-PANIC-OVERUSE — **已删**（纯计数阈值属 wildcard 启发式，价值不足；语义版"在 Result/Option 返回函数里 panic"留作 backlog）|
| style | L-NAMING-THREAD-HANDLE (Info) — **弃**：约定句柄名以 `Thread` 结尾，但项目惯用短名（`r`/`d`/`w1`/`logger`…），触发会破坏 `--check` 干净并误伤惯用写法 |


## 13. example-code.md 覆盖映射

| 示例 | 关键特性 | 测试覆盖 | 状态 |
|---|---|---|---|
| 1. Counter 可终止线程模板 | thread 模板、expose、expose_mutable + locked、三个 hook、async handler | `tests/tss/thread_lifecycle.tss` + `tests/tss/handler_dispatch.tss` | ✅ |
| 2. Producer/Consumer Queue<String> | Queue.enqueue / dequeue（返回 Option）、close | `helloworld.tss`、`demo.tss` | ✅ |
| 3. Heartbeat 不可终止线程 | 无 `__on_terminate__`、async tick、asleep | `tests/tss/heartbeat.tss`（`test_heartbeat`）| ✅ |
| 4. 主流程 handler 调用 + .wait() | sync 上下文等待 HandlerFuture | `tests/tss/handler_dispatch.tss` | ✅ |
| 5. `__ping__` 健康检查 | 隐式 handler `__ping__()` | `tests/tss/ping_handler.tss` | ✅（P0-2 注入）|
| 6. expose / locked<int> 读取 | 直接 `.expose`、locked.get/set | `thread_lifecycle.tss`、`locked_shared.tss` | ✅ |
| 7. 匿名线程 `${ ... }` | shorthand 语法 | `tests/tss/anonymous_thread.tss` | ✅（P1-5）|
| 8. async 函数 closeBusLater() | 顶层 async 函数 | `tests/tss/async_toplevel_func.tss`（`test_async_toplevel_func`：`compute(41).wait()`）| ✅ |
| 9. try/await 错误处理 | try await + HandlerDispatchError | `tests/tss/try_await_error.tss`（`test_try_await_error`：TargetTerminated）| ✅ |
| 10. 终止后健康检查 | terminate + Err("TargetTerminated") 比较 | `test_target_terminated_kind`（断言 kind）| ✅（P0-1）|


## 14. 已识别偏差

> 初始审计的偏差 handler/主体并发（A-2 修订规范 + P0-3 gate）、HandlerDispatchError kind 命名（P0-1）、int 字面量窄化（P0-4 + 规范 B-1 wrap-around 定稿）、scope `define` 外部可见（P0-5 + L-DEFINE-EXTERNAL-ACCESS）均已解决（详见 git 历史）。当前仅余下列一项。

### 偏差 1：List / Map 跨线程使用缺领域级诊断（部分缓解）

- **规范**：跨线程使用 List/Map 需要显式包 `locked<T>` 或 `Queue<T>`。
- **实现**：`Value::List(Rc<...>)` / `Value::Map(Rc<...>)` 不是 `Send`，跨线程使用会触发 Rust 层错误而非 Tessera 领域错误。`L-EXPOSE-MUTABLE-UNSAFE` 已覆盖 `expose_mutable` 一侧（Error）。
- **已缓解**：只读 `expose` 暴露非并发安全容器现有 `L-EXPOSE-READONLY-CONTAINER`（Info，`passes/expose_readonly_container.rs`）提示改用 `locked<T>`/`Queue<T>` 或保持 `define` 私有。
- **仍开放**：底层 Rc 非 Send 触发的运行期错误信息仍是 Rust 层信息，未细化为 Tessera 领域错误；这需要运行时层改造，非 lint 可解。


## 15. 仍开放的 TODO

> P0 全部、绝大多数 P1 已落地（见 §1–§13 与 git 历史）。以下为当前仍开放项。

### 语言能力

- **用户函数泛型** `function f<T>(x: T): T`（§11）— parser/AST 无类型参数列表；影响面大，建议先 spec 后实现（monomorphization vs 类型变量）。
- **一等函数 / lambda** — 解锁 `List.map/filter/reduce/forEach`、`Map.forEach` 等需要函数参数的标准库方法。
- **标准库纯增量** — `Map.keys/values`、`List.slice`、`HashSet<T>`（`Channel<T>` 与 `Queue<T>` 高度重叠）。

### 缺失的 Linter 规则（见 §12 未实现表）

- **Round 3 已落地**：L-RETURN-TYPE-MISMATCH、L-VOID-RETURN-VALUE（Error）；L-EXPOSE-READONLY-CONTAINER、L-GENERIC-NESTING-DEPTH（Info）。
- **Round 4 已落地**：L-CONTROL-OUTSIDE-LOOP、L-AT-TEMPLATE-PARAM-MISMATCH、L-ASSERT-ALWAYS-TRUE/FALSE、L-AWAIT-UNCONSUMED-FUTURE、L-TERMINATE-FUTURE-IGNORED；并删除 L-PANIC-OVERUSE（纯计数 wildcard）。
- **经评估弃用/跳过**（理由见 §12）：L-TEMPLATE-APPLY-CONTEXT、L-DEFINE-IN-NON-TEMPLATE（parser 层已强制）；L-ASYNC-NO-AWAIT、L-HANDLER-NO-AWAIT、L-NAMING-THREAD-HANDLE、L-ASSERT-SIDE-EFFECT、option/result unwrap（不可 sound / 误伤 / 直接 unwrap 即已知晓风险）。
- **仍开放**：L-SYNC-* 系、L-EXPOSE-MUTABLE-ACCESS-PATTERN、L-HANDLER-FUTURE-MISUSE 等需数据流/owner 追踪。

### 测试缺口

- **全部已补齐**：`keepalive()` 永不返回（§3）、`expose_mutable` 字段替换（§4）、短路求值反例（§8）、同块重复声明拒绝（§10）、example 3/8/9（§13）；Round 4 补齐 R-TRY-2（§6）、R-HANDLER-SCOPE（§3）、ParseError 字符串负载（§9）。


## 16. 验证与维护

### 如何复核本报告

1. **代码定位**：报告中每条 `file:line` 在仓库可直接打开。可用 ripgrep 复核某条目，例如：
   - `rg "is_concurrent_safe" crates/` → 验证 §4 R-EXPOSE-2；
   - `rg "__on_terminate__" crates/tessera-interp/src/event_loop.rs` → 验证 §3 R-LIFE-2。
2. **集成测试断言**：在仓库根运行 `cargo test -p tessera-interp`（47）与 `cargo test -p tessera-types`（9 typecheck）应全部通过；报告中标 ✅ 的条目对应的测试名可直接 grep 自 `crates/tessera-interp/tests/integration.rs` 与 `crates/tessera-types/tests/typecheck.rs`。
3. **Linter 实装核对**：`crates/tessera-lint/src/passes/mod.rs::all()` 共 29 个 `Box::new(...)`，与 §12 已实现表完全对应。
4. **示例核对**：用 `E:/Tessera-Spec/example-code.md` 与 `helloworld.tss` / `demo.tss` / `tests/tss/*.tss` 比对 §13。

### 维护建议

- **绑定到 PR 流程**：当 PR 触及解释器语义、新增 Lint pass、新增内建函数时，更新本报告对应章节（建议加入 PR Checklist）。
- **基线版本号**：报告顶部 commit 哈希需在每次主干合并后由作者更新（例如自动化脚本扫描 `git log -1 --format=%h`）。
- **规范变更**：若 `E:/Tessera-Spec/*.md` 发生修订（即使是行号变化），需复核标 ✅ 的条目是否还和文档一致。
- **TODO 跟踪**：§15 仍开放项建议拆为 GitHub Issue 跟踪，每条 close 时同步更新本报告。
- **报告自身的测试**：CI 可加 `cargo test -p tessera-lint --tests` smoke check，确保 29 个 pass 与本报告一致。

---

*本报告反映 tessera-mvp 方向 1 Round 4 / Tessera-Spec `41ae1c4` 的当前状态；逐轮修复脉络见 git 历史。*

