# Tessera Spec Alignment 报告

> **当前基准**：tessera-mvp `7f993dd` / Tessera-Spec `7ded246`（2026-05-29）。`E:/tessera-mvp/` 实现 vs `E:/Tessera-Spec/` 规范的逐条对照。
> 覆盖：tessera-lexer / tessera-parser / tessera-ast / tessera-types / tessera-runtime / tessera-interp / tessera-lint。
> 测试：tessera-interp 37 integration + tessera-lint 28 smoke + parser/lexer 各 5（合计 75）；Lint pass 21。`cargo build --workspace --all-targets` 无 warning；`--check helloworld.tss` / `demo.tss` 均无诊断。
>
> **状态徽标**：✅ 已实现且有测试 ／ ⚠️ 已实现但缺测试或行为未完全覆盖 ／ ❌ 未实现 ／ 🔶 实现与规范存在语义偏差。
>
> 本文档只反映**当前最终状态**。逐轮修复脉络（4 轮 spec-alignment + 方向 1 的 Round 1「R-SYNC-BREAK-3 净简化」/ Round 2「L-EXCL-AWAIT」）保存在 git 历史中。

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
| 解释器（lexer + parser + ast + runtime + interp） | 高度对齐 | `keepalive()` 永不返回语义缺独立验证测试；List/Map 跨线程使用缺领域级诊断（§14） |
| 类型系统（tessera-types） | 大体对齐 | **用户函数泛型未实现**（`function f<T>(...)`，§11）；`expose_mutable` 字段替换的触发路径未验证（§4） |
| Linter（tessera-lint） | 21 个 pass | 规范草案约半数 L 规则未实现（L-TEMPLATE-APPLY-CONTEXT / L-AWAIT-EXPR-IN-TOPLEVEL / L-RETURN-TYPE-MISMATCH / L-VOID-RETURN-VALUE / L-ASYNC-NO-AWAIT / option·result unwrap / style 等，见 §12 未实现表）|

> 说明：spec-alignment 四轮 + 方向 1 Round 1/2 已落地全部 P0、绝大多数 P1，以及标准库 13 方法、4 类同步原语上下文 lint、`#exclusive`/同步原语相关规则等。§1–§13 各表已逐行更新到当前状态。


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
| R-HANDLER-2 | 同线程不并发执行两个 handler | ✅ | handler-in-flight gate（`ThreadState.handler_in_flight: watch<bool>` + 主 select gate + `dispatch_handler_inline` 设/释放）| P0-3；测试 `tests/tss/handler_mutex.tss` 用时间戳序列断言无交错。规范 A-2：handler↔handler 严格互斥，body↔handler 挂起点交替合法 |
| R-HANDLER-3 | 主体 ↔ handler 仅在挂起点切换 | ✅ | 协作调度自然满足 | |
| R-HANDLER-4 | Terminating/Terminated/Crashed 拒绝新 handler；队列项以调度失败结束 | ✅ | `thread_state.rs:170-188` + `event_loop.rs::drain_handlers` | 三种状态映射到三个 dispatch error |
| R-HANDLER-5 | 主体结束后不再执行 handler | ✅ | `event_loop.rs:252,259,289` 主体完成路径 drain 队列 | |
| R-HANDLER-PING | 所有线程模板隐式具有 `async handler __ping__(): String` 返回 "pong" | ✅ | `registration.rs` 自动注册 + `thread_state.rs::dispatch_handler` 入口特判；不进队列、不受 R-HANDLER-2 / `#exclusive` 约束 | P0-2；测试 `tests/tss/ping_handler.tss`；用户重写触发 L-HANDLER-PING-REDEFINED |
| R-HANDLER-SCOPE | handler 只能访问模板对象，不可访问外部作用域 | ⚠️ | `event_loop.rs:39,120` + 创建 handler body 时使用模板 env | 实现倾向正确，但无 lint/反例测试 |
| R-EXCL-1 | `#exclusive` 内独占线程；handler/timer/IO 不交错 | ✅ | `event_loop.rs:300 + 345-350`（select gate + handler 任务 watch 等待）| `exclusive_block.tss` 覆盖 |
| R-EXCL-2 | `#exclusive` 阻塞 handler 进入，但不阻塞入队 | ✅ | mpsc 通道一直接收；只在主 select 上 gate | |
| R-EXCL-3 | 在 `#exclusive` 期间收到 terminate → 立即转 Terminating，teardown 延后 | ✅ | `event_loop.rs:157-184, 199-223, 270-294` | 代码注释明确引用 R-EXCL-3 |
| R-EXCL-4 | `#exclusive` 内 await 依赖本线程调度 → 死锁（告知性约束） | ✅（sound 子集） | `tessera-lint/src/passes/exclusive_self_primitive_await.rs`（L-EXCL-AWAIT, Warn）命中 `#exclusive` 内 await/`.wait()` 自有同步原语 | 后续立项 Round 2 完成；完整判定不可静态化，仅落地 sound 子集 |
| R-KEEPALIVE-1 | `keepalive()` 返回永不完成 Future | ⚠️ | `crates/tessera-interp/src/eval/builtin.rs`、`eval.rs` 中实现并返回挂起 Future | 需复核 await 后是否真正不返回；缺独立测试 |
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
| R-EXPOSE-3 | `expose_mutable` 字段引用不可被外部替换，只可通过其方法改内容 | ⚠️ | `runtime/src/error.rs:108-116` 定义 `ExposeMutableFieldReplace`；但触发路径需复核（eval.rs 是否在外部赋值时实际抛出） | 错误变体存在但触发覆盖未验证 |
| R-DEFINE-1 | `define` 字段仅在模板内部可见，不可经线程句柄访问 | ✅ | runtime 天然隔离（FieldAccess 只查 expose_fields / expose_mutable_fields）+ `passes/define_external_access.rs`（L-DEFINE-EXTERNAL-ACCESS）| P0-5；测试 `test_define_field_external_invisible` |
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
| 顶层 await 限制 | 顶层不可写 `await expr;` | ✅ | `passes/toplevel_control_flow.rs`（L-TOPLEVEL-CONTROL-FLOW）禁止顶层 `await`/`return` | P1-1 |
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
| §2.1 短路 | `&&` / `||` | ⚠️ | `eval.rs::eval_binop` 检查 short-circuit；需复核是否对 LHS=false 时 RHS 不求值 | 缺反例测试（带副作用 RHS）|
| §2.2 运算符优先级 | 一元 > * / % > + - > 比较 > ==/!= > && > || | ✅ | `parser.rs:964-973`（Pratt binding power）+ unary 在 primary 前 | 单元测试 `pratt_precedence` |
| §3.1 `let name = expr;` / `let name: T = expr;` | 变量声明必须初始化 | ✅ | `parser.rs:480-488` + `checker/stmts.rs:13-20` | |
| §3.2 禁止未初始化声明 | `let x: int;` 禁止 | ✅ | parser 要求 `=` 否则报错 | |
| §4.1.2 非 void 函数所有路径 return | 静态检查 | ✅ | `passes/return_not_all_paths.rs`（L-RETURN-NOT-ALL-PATHS）| P1 新增 |
| §4.1.2 void 函数允许隐式末尾 return | | ✅ | `checker/bodies.rs` 实现 | |
| §4.2 async 函数返回类型自动包 `Future<T>` | | ✅ | `registration.rs` 注册时包裹 | |
| §4.3 hooks 签名约束（`__on_enter__`/`__on_exit__` 是同步 void；`__on_terminate__` 是 async void）| | ✅ | `passes/hook_signature.rs`（L-FUNCTION-HOOK-SIGNATURE）| P1-2 |
| 函数返回类型不匹配 | 静态检查 | ❌ | 无 L-RETURN-TYPE-MISMATCH | 缺失 |
| void 函数返回值表达式 | 静态检查 | ❌ | 无 L-VOID-RETURN-VALUE | |


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
| `ParseError` | `String.toInt() / toDouble()` 返回 | `builtin.rs:52-62`：以 `Result<T, Value::Str(...)>` 表达 | ⚠️ 实际错误是 `Value::Str`，非独立 `ParseError` 类型 |
| `error`（`.kind` + `.message`）| `try` 表达式结果 | `runtime/src/value.rs:47`、`error.rs::kind_and_message` | ✅ |
| `Future<T>` | wait、isDone | `builtin.rs:148-154` | ✅ |
| 字符串方法（startsWith / endsWith / contains / indexOf / trim / split）| 标准容器规范 §13 | `builtin.rs` | ✅（round 3 加 6 方法）|
| `List.contains / indexOf / clear` 扩展方法 | 标准容器规范扩展接口 | `builtin.rs` | ✅（round 3）；`slice` / `forEach`（需一等函数）仍缺 |
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
| 嵌套深度提示 | L-GENERIC-NESTING-DEPTH | ❌ | 未实现 | |


## 12. Linter 规则对照

实现注册在 `crates/tessera-lint/src/passes/mod.rs::all()`，当前共 **21** 个 pass（权威列表以 `mod.rs::all()` 为准）。

### 已实现（21）

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
| L-EXCL-AWAIT | `passes/exclusive_self_primitive_await.rs` | `#exclusive` 内 await/`.wait()` 自有同步原语（R-EXCL-4 sound 子集）| ✅ Warn |
| L-EXPOSE-READONLY-WRITE | `passes/expose_readonly_write.rs` | `expose`（只读）字段不可外部写 | ✅ |
| L-DEFINE-EXTERNAL-ACCESS | `passes/define_external_access.rs` | `define` 字段不可经句柄外部访问 | ✅ |
| L-TOPLEVEL-CONTROL-FLOW | `passes/toplevel_control_flow.rs` | 顶层禁 `await`/`return`，`break`/`continue` 仅循环内 | ✅ |
| L-FUNCTION-HOOK-SIGNATURE | `passes/hook_signature.rs` | hook 签名约束（sync/async void）| ✅ |
| L-HANDLER-RESULT-IGNORED | `passes/handler_result_ignored.rs` | 裸 handler 调用语句报警 | ✅ Warn |
| L-HANDLER-PING-REDEFINED | `passes/handler_ping_redefined.rs` | 用户重定义 `__ping__` 报错 | ✅ |
| L-RETURN-NOT-ALL-PATHS | `passes/return_not_all_paths.rs` | 非 void 函数所有路径须 return | ✅ |
| L-SIGNAL-AWAIT-IN-SYNC | `passes/signal_await_in_sync.rs` | sync 上下文 `await <signal>` | ✅ |
| L-SIGNAL-WAIT-IN-ASYNC | `passes/signal_wait_in_async.rs` | async 上下文 `.wait()` on signal | ✅ Warn |
| L-CONTRACT-AWAIT-IN-SYNC | `passes/contract_await_in_sync.rs` | sync 上下文 `await <contract>` | ✅ |
| L-CONTRACT-WAIT-IN-ASYNC | `passes/contract_wait_in_async.rs` | async 上下文 `.wait()` on contract | ✅ Warn |

### 未实现（按规范 `Linter 规则草案.md` 列举）

| 类别 | 缺失规则 |
|---|---|
| async/await | L-ASYNC-NO-AWAIT (Info)；L-AWAIT-EXPR-IN-TOPLEVEL (Error，顶层场景已由 L-TOPLEVEL-CONTROL-FLOW 覆盖) |
| terminate | L-TERMINATE-FUTURE-IGNORED (Warn) |
| expose / define | L-EXPOSE-MUTABLE-ACCESS-PATTERN (Info)、L-DEFINE-IN-NON-TEMPLATE (Error) |
| handler | L-HANDLER-NO-AWAIT (Info)、L-HANDLER-CALL-SITE-AWAIT (Info)、L-HANDLER-DISPATCH-ERROR-UNHANDLED (Info)、L-HANDLER-FUTURE-MISUSE (Error；`L-HANDLER-AWAIT-TYPE` 已覆盖反方向) |
| template | L-TEMPLATE-APPLY-CONTEXT (Error)、L-AT-TEMPLATE-STACKING (Error；语法已防御) |
| function | L-RETURN-TYPE-MISMATCH、L-VOID-RETURN-VALUE |
| generics | L-GENERIC-NESTING-DEPTH (Info) |
| option/result | L-OPTION-UNWRAP-POSSIBLE-NONE、L-RESULT-UNWRAP-POSSIBLE-ERR |
| future | L-AWAIT-UNCONSUMED-FUTURE (Warn) |
| sync 原语 | L-SYNC-EXPOSE-OWNER (Info)、L-SYNC-AWAIT-NOCHECK、L-SYNC-SCOPE-DEFINE-PASS (Warn)、L-SYNC-TRIGGER-AFTER-BROKEN (Info) |
| panic / assert | L-ASSERT-SIDE-EFFECT、L-PANIC-OVERUSE、L-ASSERT-ALWAYS-TRUE、L-ASSERT-ALWAYS-FALSE |
| style | L-NAMING-THREAD-HANDLE (Info) |


## 13. example-code.md 覆盖映射

| 示例 | 关键特性 | 测试覆盖 | 状态 |
|---|---|---|---|
| 1. Counter 可终止线程模板 | thread 模板、expose、expose_mutable + locked、三个 hook、async handler | `tests/tss/thread_lifecycle.tss` + `tests/tss/handler_dispatch.tss` | ✅ |
| 2. Producer/Consumer Queue<String> | Queue.enqueue / dequeue（返回 Option）、close | `helloworld.tss`、`demo.tss` | ✅ |
| 3. Heartbeat 不可终止线程 | 无 `__on_terminate__`、async tick、asleep | 间接覆盖于 `helloworld.tss`；无独立 .tss | ⚠️ |
| 4. 主流程 handler 调用 + .wait() | sync 上下文等待 HandlerFuture | `tests/tss/handler_dispatch.tss` | ✅ |
| 5. `__ping__` 健康检查 | 隐式 handler `__ping__()` | `tests/tss/ping_handler.tss` | ✅（P0-2 注入）|
| 6. expose / locked<int> 读取 | 直接 `.expose`、locked.get/set | `thread_lifecycle.tss`、`locked_shared.tss` | ✅ |
| 7. 匿名线程 `${ ... }` | shorthand 语法 | `tests/tss/anonymous_thread.tss` | ✅（P1-5）|
| 8. async 函数 closeBusLater() | 顶层 async 函数 | 间接（handler_dispatch.tss 使用 keepalive） | ⚠️ |
| 9. try/await 错误处理 | try await + HandlerDispatchError | `scope_binding.tss`（仅 ScopeGone） | ⚠️ |
| 10. 终止后健康检查 | terminate + Err("TargetTerminated") 比较 | `test_target_terminated_kind`（断言 kind）| ✅（P0-1）|


## 14. 已识别偏差

> 初始审计的偏差 handler/主体并发（A-2 修订规范 + P0-3 gate）、HandlerDispatchError kind 命名（P0-1）、int 字面量窄化（P0-4 + 规范 B-1 wrap-around 定稿）、scope `define` 外部可见（P0-5 + L-DEFINE-EXTERNAL-ACCESS）均已解决（详见 git 历史）。当前仅余下列一项。

### 偏差 1：List / Map 跨线程使用缺领域级诊断

- **规范**：跨线程使用 List/Map 需要显式包 `locked<T>` 或 `Queue<T>`。
- **实现**：`Value::List(Rc<...>)` / `Value::Map(Rc<...>)` 不是 `Send`，跨线程使用会触发 Rust 层错误而非 Tessera 领域错误。`L-EXPOSE-MUTABLE-UNSAFE` 已覆盖 `expose_mutable` 一侧。
- **影响**：错误信息不直观；只读 `expose` 暴露 List/Map 缺 Info 级提示。
- **建议**：增加一条 Info 级 lint，对 `expose` 暴露非并发安全容器给出提示。


## 15. 仍开放的 TODO

> P0 全部、绝大多数 P1 已落地（见 §1–§13 与 git 历史）。以下为当前仍开放项。

### 语言能力

- **用户函数泛型** `function f<T>(x: T): T`（§11）— parser/AST 无类型参数列表；影响面大，建议先 spec 后实现（monomorphization vs 类型变量）。
- **一等函数 / lambda** — 解锁 `List.map/filter/reduce/forEach`、`Map.forEach` 等需要函数参数的标准库方法。
- **标准库纯增量** — `Map.keys/values`、`List.slice`、`HashSet<T>`（`Channel<T>` 与 `Queue<T>` 高度重叠）。

### 缺失的 Linter 规则（见 §12 未实现表）

- 优先：L-TEMPLATE-APPLY-CONTEXT、L-RETURN-TYPE-MISMATCH、L-VOID-RETURN-VALUE、L-DEFINE-IN-NON-TEMPLATE。
- 信息级：L-ASYNC-NO-AWAIT、L-NAMING-THREAD-HANDLE、L-ASSERT-SIDE-EFFECT、L-PANIC-OVERUSE、L-GENERIC-NESTING-DEPTH；以及 expose 只读容器跨线程提示（§14 偏差 1）。
- option/result unwrap 可空性、L-AWAIT-UNCONSUMED-FUTURE 等需数据流分析。

### 测试缺口（⚠️ 行）

- `keepalive()` 永不返回的独立验证测试（§3）；`expose_mutable` 字段替换触发路径验证（§4）；短路求值带副作用 RHS 反例（§8）；同块重复声明拒绝（§10）；example 3/8/9 的独立 .tss。


## 16. 验证与维护

### 如何复核本报告

1. **代码定位**：报告中每条 `file:line` 在仓库可直接打开。可用 ripgrep 复核某条目，例如：
   - `rg "is_concurrent_safe" crates/` → 验证 §4 R-EXPOSE-2；
   - `rg "__on_terminate__" crates/tessera-interp/src/event_loop.rs` → 验证 §3 R-LIFE-2。
2. **集成测试断言**：在仓库根运行 `cargo test -p tessera-interp` 应全部通过；报告中标 ✅ 的条目对应的测试名可直接 grep 自 `crates/tessera-interp/tests/integration.rs`。
3. **Linter 实装核对**：`crates/tessera-lint/src/passes/mod.rs::all()` 共 21 个 `Box::new(...)`，与 §12 已实现表完全对应。
4. **示例核对**：用 `E:/Tessera-Spec/example-code.md` 与 `helloworld.tss` / `demo.tss` / `tests/tss/*.tss` 比对 §13。

### 维护建议

- **绑定到 PR 流程**：当 PR 触及解释器语义、新增 Lint pass、新增内建函数时，更新本报告对应章节（建议加入 PR Checklist）。
- **基线版本号**：报告顶部 commit 哈希需在每次主干合并后由作者更新（例如自动化脚本扫描 `git log -1 --format=%h`）。
- **规范变更**：若 `E:/Tessera-Spec/*.md` 发生修订（即使是行号变化），需复核标 ✅ 的条目是否还和文档一致。
- **TODO 跟踪**：§15 仍开放项建议拆为 GitHub Issue 跟踪，每条 close 时同步更新本报告。
- **报告自身的测试**：CI 可加 `cargo test -p tessera-lint --tests` smoke check，确保 21 个 pass 与本报告一致。

---

*本报告反映 tessera-mvp `7f993dd` / Tessera-Spec `7ded246` 的当前状态；逐轮修复脉络见 git 历史。*

