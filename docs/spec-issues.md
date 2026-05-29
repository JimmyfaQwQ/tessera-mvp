# Tessera 规范本身的问题清单

> 来源：在 `docs/spec-alignment.md` 的对照审计过程中，发现规范本身（`E:/Tessera-Spec/*.md`）存在若干内部矛盾、欠规定、冗余与符号细节缺失。本文档专门收集这些"规范侧"问题，与实现 bug 互不重叠。
>
> **严重程度**：🔴 P0 内部矛盾（必修订其一）／🟡 P1 语义未明确（建议补全）／🔵 P2 冗余·风格·编号一致性（建议清理）。
>
> 本文档只保留**裁决结果**。各问题的原始陈述、冲突来源与裁决论证保存在 git 历史中（详见 `docs/spec-alignment.md` 同期提交）。

## 裁决结果（A 内部矛盾 / B 欠规定 / C 冗余 / D 符号细节 / E 风格一致性）

| 条目 | 状态 | 修订位置 |
|---|---|---|
| A-1 HandlerDispatchError kind 命名冲突 | ✅ 已修订 | `基础类型...md:393` 将 `"TargetGone"` 改为 `"TargetTerminated"` |
| A-2 R-HANDLER-2 与 spawn_local 张力 | ✅ 已修订 | `线程与事件循环规范.md` R-HANDLER-2 / R-HANDLER-3 各自加 Rationale；实现侧用 handler-in-flight gate 满足 R-HANDLER-2 严格语义 |
| A-3 R-EXCL-3 + R-LIFE 边界 | ✅ 已修订 | `线程与事件循环规范.md` R-EXCL-3 末尾加"粘性 Terminating"子句 |
| A-4 同步原语链式绑定 | ✅ 已修订 | `同步原语与崩溃传播规范草案.md` 新增 R-SYNC-OWN-4 |
| A-5 `__ping__` 隐式 handler 语义 | ✅ 已修订 | `线程与事件循环规范.md` R-HANDLER-PING 重写为"调度即应答" |
| B-1 int 溢出语义 | ✅ 已修订 | `基础类型...md §1.1.2` 明确 wrap-around；字面量越界 parse 阶段拒绝 |
| B-2 char 转义集 | ✅ 已修订 | `基础类型...md §1.1.4` 补完整转义表（含 `\u{HHHH}`） |
| B-3 String 隐式转换格式 | ✅ 已修订 | `基础类型...md §1.1.5.3` 补转换表 |
| B-4 try 包裹同步表达式 | ✅ 已修订 | `错误与异常语义草案.md` 在 `try expr` 介绍处补 Rationale |
| B-5 hook 详细语义 | ✅ 已修订 | `模板与线程规范.md §4.x` 新增 hook 语义细则 |
| B-6 define 初始化顺序 | ✅ 已修订 | `数据共享与并发安全规范.md §4.2.1` 补字段初始化顺序 |
| B-7 `#exclusive` 嵌套 | ✅ 已修订 | `线程与事件循环规范.md` R-EXCL-1 补 reentrant 嵌套语义 |
| B-8 内建函数总表 | ✅ 已修订 | `标准容器与常用类型规范草案.md §12` 内建函数权威表 |
| B-9 `Err("kind")` 字符串比较 | ✅ 已修订 | `错误与异常语义草案.md` 补语法糖展开 |
| B-10 Future vs HandlerFuture 关系 | ✅ 已修订 | `async await 规范草案.md §3.x` 不相容关系表 |
| B-11 generic 推断边界 | ✅ 已修订 | `泛型与类型构造器规范草案.md` 推断边界 |
| B-12 顶层执行模型 | ✅ 已修订 | `Tessera 核心语义.md §1.2` 明确顶层是 sync 上下文 + 顶层控制流限制 |
| C-1 R-SCHED 重复 | ✅ 已修订 | `核心语义.md` R-CORE-SCHED-1/2 加引用注，权威源为 `线程与事件循环.md` R-SCHED-1 / R-HANDLER-1+2 |
| C-2 R-CORE-HANDLER-1 ≡ R-SYNC-NOORPHAN-1 | ✅ 已修订 | `核心语义.md` R-CORE-HANDLER-1 加引用注；权威源为 R-SYNC-NOORPHAN-1 |
| C-3 README/核心语义/错误异常 概述重复 | ✅ 已结构性解决 | `核心语义.md §11` 开头已声明"详见《错误与异常语义草案》" |
| C-4 规则与散文混排 | ✅ 已示范 | `线程与事件循环.md` R-HANDLER-1 加 *Motivation:* 标签作为后续清理模板 |
| D-1 匿名 scope 模板 | ✅ 已修订 | `模板与线程.md §4.5` |
| D-2 `:=` 优先级 | ✅ 已修订 | `模板与线程.md §4.6` |
| D-3 原语大小写 | ✅ 已修订 | `标准容器.md` 开头命名约定 |
| D-4 expose_mutable 引用 | ✅ 已修订 | `数据共享.md` R-EXPOSE-3 后补 D-4 修订 |
| D-5 thread spawn 句法位置 | ✅ 已修订 | `模板与线程.md §4.7` |
| D-6 try 优先级 | ✅ 已修订 | `错误与异常.md` `try expr` 介绍处补优先级说明 |
| E-1 编号格式 | ✅ 已修订 | `Tessera-Spec/README.md` "规范维护约定" §编号格式 |
| E-2 Lint Severity 边界 | ✅ 已修订 | `Tessera-Spec/README.md` §Lint 严重级；权威源为 `crates/tessera-lint/src/passes/*.rs` |
| E-3 Rule/Rationale 版式 | ✅ 已修订 | `Tessera-Spec/README.md` §Rule/Rationale 版式；以 R-HANDLER-1 为示范 |
| E-4 example-code 同源 | ✅ 已修订 | `Tessera-Spec/example-code.md` 与 `tessera-mvp/helloworld.tss` 同源；由 cargo test + `--check` 守护 |
| E-5 文档顶部总览 | ✅ 已修订 | `Tessera-Spec/README.md` §文档顶部总览（约定，按需迁移） |
| E-6 跨文档锚点 | ✅ 已修订 | `Tessera-Spec/README.md` §跨文档锚点；CI 校验为后续工作 |

## 后续立项 Round 1 / Round 2（方向 1）

| 条目 | 状态 | 修订位置 |
|---|---|---|
| R-SYNC-BREAK-3 第一句删除（Round 1）| ✅ 已修订 | `同步原语与崩溃传播规范草案.md §3.3` + §6 总览：删去"`Broken` 唤醒不早于 `#exclusive` 块结束"（不可实现 + 冗余）；Rationale 重写为"原子性由 R-EXCL-1 成立、与 R-EXCL-4 衔接"。实现侧删除 best-effort 拐杖 `delay_broken_until_exclusive_ends`。见 `spec-alignment.md §7`。 |
| R-EXCL-4 lint（L-EXCL-AWAIT，Round 2）| ✅ 已修订 | `线程与事件循环规范.md §4.5` 在 R-EXCL-4 后补 *Lint 锚定* 段。审计发现原拟的"await self-handler"子集不可表达（`self` 是 `TemplateObject` 非 `ThreadHandle`），改为 sound 子集"`#exclusive` 内 await/`.wait()` 自有 signal/contract/permit"。实现 `passes/exclusive_self_primitive_await.rs`（L-EXCL-AWAIT, Warn）。见 `spec-alignment.md §3` R-EXCL-4。 |

## 后续立项 Round 3（方向 1）：对齐补全

> 本轮把 §15 剩余的"对齐"缺口逐条 sound 性裁定后落地/降级/跳过。详见 `spec-alignment.md` §12/§14/§15。

| 条目 | 状态 | 处置与位置 |
|---|---|---|
| L-RETURN-TYPE-MISMATCH | ✅ 落地（Error）| `passes/return_type_mismatch.rs`：sound 子集 = 非-void 裸 `return;` + 标量字面量返回类型确定不符（int⇆double 保守跳过）。规范 `Linter 规则草案.md §9` 已有定义。 |
| L-VOID-RETURN-VALUE | ✅ 落地（Error）| `passes/void_return_value.rs`：`void` 单元 `return <expr>;`。 |
| L-EXPOSE-READONLY-CONTAINER | ✅ 落地（Info，新规则）| `passes/expose_readonly_container.rs`。规范侧新增：`Linter 规则草案.md §3` + `数据共享与并发安全规范.md §3.2` Lint 锚定。对应 `spec-alignment.md §14 偏差 1`。 |
| L-GENERIC-NESTING-DEPTH | ✅ 落地（Info）| `passes/generic_nesting_depth.rs`：深度 > 3 触发。规范侧补「阈值约定」于 `Linter 规则草案.md §10`。 |
| L-PANIC-OVERUSE | ✅ 落地（Info）| `passes/panic_overuse.rs`：单元内 `panic(...)` > 3。规范侧补「阈值约定」于 `Linter 规则草案.md §7`。 |
| L-TEMPLATE-APPLY-CONTEXT | ⏭️ 跳过 | parser 无 `$` 表达式 primary，spawn 只能作语句（§4.7 许可所有语句位置）；嵌入表达式即 parse 错误 —— 无可 lint 的 AST 目标，parser 层已强制。规范侧补实现注记于 `Linter 规则草案.md §5`（经 §3 注）。 |
| L-DEFINE-IN-NON-TEMPLATE | ⏭️ 跳过 | `KwDefine` 仅在模板成员解析位置消费；他处为 parse 错误 —— parser 层已强制。规范侧补实现注记于 `Linter 规则草案.md §3`。 |
| L-ASYNC-NO-AWAIT | ⏭️ 跳过 | 合法的「无 await async 函数」（`await run()` 需 `run` 为 async，即便其体无 await）与冗余 async 不可静态区分；即便降 Info 也会误伤 `helloworld reader.run` 并破坏 `--check` 干净。需全程调用图分析才 sound。 |
| L-NAMING-THREAD-HANDLE | ⏭️ 跳过 | 约定句柄名以 `Thread` 结尾，但项目自身惯用短名（`r`/`d`/`w1`/`logger`…），触发会破坏 `--check` 干净并误伤惯用写法。 |
| L-ASSERT-SIDE-EFFECT | ⏭️ 跳过 | Tessera 赋值是语句非表达式（`count++`/`x=5` 不可表达于 assert 条件），仅余调用，纯/非纯不可判定，会误伤 `assert(list.isEmpty())` 类合法写法。 |
| 同块重复声明拒绝 | ✅ 落地 | `checker/stmts.rs::check_dup_lets`（let-vs-let，接入 `check_block`/scope 块/Pass 4）。规范 `语句与控制流规范草案.md §2.1.2` 已有定义，无需改规范。测试 `tessera-types/tests/typecheck.rs`。 |
| 测试缺口（keepalive / expose_mutable 替换 / 短路 / example 3·8·9）| ✅ 补齐 | 见 `spec-alignment.md §3/§4/§8/§13`；`tests/tss/*.tss` + `integration.rs`。 |
