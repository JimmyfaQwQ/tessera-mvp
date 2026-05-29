# Tessera 规范本身的问题清单

> 来源：在 `docs/spec-alignment.md` 的对照审计过程中，发现规范本身（`E:/Tessera-Spec/*.md`）存在若干内部矛盾、欠规定、冗余与符号细节缺失。本文档专门收集这些"规范侧"问题，与实现 bug 互不重叠。
>
> **严重程度**
> - 🔴 P0 — 规范内部自相矛盾，实现无法两全；必须修订其一
> - 🟡 P1 — 语义未明确，实现需要做"无指南"决定；建议补全
> - 🔵 P2 — 冗余/风格/编号一致性问题；建议清理

## 本轮修复状态（截至 spec-alignment fix 完成时）

| 条目 | 状态 | 修订位置 |
|---|---|---|
| A-1 HandlerDispatchError kind 命名冲突 | ✅ 已修订 | `基础类型...md:393` 将 `"TargetGone"` 改为 `"TargetTerminated"` |
| A-2 R-HANDLER-2 与 spawn_local 张力 | ✅ 已修订 | `线程与事件循环规范.md` R-HANDLER-2 / R-HANDLER-3 各自加 Rationale；实现侧用 handler-in-flight gate 满足 R-HANDLER-2 严格语义 |
| A-3 R-EXCL-3 + R-LIFE 边界 | ✅ 已修订 | `线程与事件循环规范.md` R-EXCL-3 末尾加"粘性 Terminating"子句 |
| A-4 同步原语链式绑定 | ✅ 已修订 | `同步原语与崩溃传播规范草案.md` 新增 R-SYNC-OWN-4 |
| A-5 `__ping__` 隐式 handler 语义 | ✅ 已修订 | `线程与事件循环规范.md` R-HANDLER-PING 重写为"调度即应答" |
| B-1 int 溢出语义 | ✅ 已修订 | `基础类型...md §1.1.2` 明确 wrap-around；字面量越界 parse 阶段拒绝 |
| B-2 char 转义集 | ✅ 已修订 | `基础类型...md §1.1.4` 补完整转义表（含 `\u{HHHH}`） |
| B-3 String 隐式转换格式 | ✅ 已修订（第三轮） | `基础类型...md §1.1.5.3` 补转换表 |
| B-4 try 包裹同步表达式 | ✅ 已修订（第三轮） | `错误与异常语义草案.md` 在 `try expr` 介绍处补 Rationale |
| B-5 hook 详细语义 | ✅ 已修订（第三轮） | `模板与线程规范.md §4.x` 新增 hook 语义细则 |
| B-6 define 初始化顺序 | ✅ 已修订（第三轮） | `数据共享与并发安全规范.md §4.2.1` 补字段初始化顺序 |
| B-7 `#exclusive` 嵌套 | ✅ 已修订 | `线程与事件循环规范.md` R-EXCL-1 补 reentrant 嵌套语义 |
| B-8 内建函数总表 | ✅ 已修订（第二轮已落实） | `标准容器与常用类型规范草案.md §12` 内建函数权威表 |
| B-9 `Err("kind")` 字符串比较 | ✅ 已修订 | `错误与异常语义草案.md` 补 §X 语法糖展开 |
| B-10 Future vs HandlerFuture 关系 | ✅ 已修订（第三轮） | `async await 规范草案.md §3.x` 不相容关系表 |
| B-11 generic 推断边界 | ✅ 已修订（第三轮） | `泛型与类型构造器规范草案.md §X` 推断边界 |
| B-12 顶层执行模型 | ✅ 已修订 | `Tessera 核心语义.md §1.2` 明确顶层是 sync 上下文 + 顶层控制流限制 |
| C-1 R-SCHED 重复 | ✅ 已修订（第三轮） | `核心语义.md` R-CORE-SCHED-1/2 加引用注，权威源为 `线程与事件循环.md` R-SCHED-1 / R-HANDLER-1+2 |
| C-2 R-CORE-HANDLER-1 ≡ R-SYNC-NOORPHAN-1 | ✅ 已修订（第三轮） | `核心语义.md` R-CORE-HANDLER-1 加引用注；权威源为 R-SYNC-NOORPHAN-1 |
| C-3 README/核心语义/错误异常 概述重复 | ✅ 已结构性解决 | `核心语义.md §11` 开头已声明"详见《错误与异常语义草案》" |
| C-4 规则与散文混排 | ✅ 已示范（第三轮） | `线程与事件循环.md` R-HANDLER-1 加 *Motivation:* 标签作为后续清理模板 |
| D-1 匿名 scope 模板 | ✅ 已修订（第三轮） | `模板与线程.md §4.5` |
| D-2 `:=` 优先级 | ✅ 已修订（第三轮） | `模板与线程.md §4.6` |
| D-3 原语大小写 | ✅ 已修订（第三轮） | `标准容器.md` 开头命名约定 |
| D-4 expose_mutable 引用 | ✅ 已修订（第三轮） | `数据共享.md` R-EXPOSE-3 后补 D-4 修订 |
| D-5 thread spawn 句法位置 | ✅ 已修订（第三轮） | `模板与线程.md §4.7` |
| D-6 try 优先级 | ✅ 已修订（第三轮） | `错误与异常.md` `try expr` 介绍处补优先级说明 |
| E-1 编号格式 | ✅ 已修订（第四轮） | `Tessera-Spec/README.md` "规范维护约定" §编号格式 |
| E-2 Lint Severity 边界 | ✅ 已修订（第四轮） | `Tessera-Spec/README.md` §Lint 严重级；权威源为 `crates/tessera-lint/src/passes/*.rs` |
| E-3 Rule/Rationale 版式 | ✅ 已修订（第四轮） | `Tessera-Spec/README.md` §Rule/Rationale 版式；以 R-HANDLER-1 为示范 |
| E-4 example-code 同源 | ✅ 已修订（第四轮） | `Tessera-Spec/example-code.md` 与 `tessera-mvp/helloworld.tss` 同源；由 cargo test + `--check` 守护 |
| E-5 文档顶部总览 | ✅ 已修订（第四轮） | `Tessera-Spec/README.md` §文档顶部总览（约定，按需迁移） |
| E-6 跨文档锚点 | ✅ 已修订（第四轮） | `Tessera-Spec/README.md` §跨文档锚点；CI 校验为后续工作 |

## 后续立项 Round 1（方向 1）：R-SYNC-BREAK-3 净简化

| 条目 | 状态 | 修订位置 |
|---|---|---|
| R-SYNC-BREAK-3 第一句删除 | ✅ 已修订 | `同步原语与崩溃传播规范草案.md §3.3` + §6 总览：删去"`Broken` 唤醒不早于 `#exclusive` 块结束"（不可实现 + 冗余）；Rationale 重写为"原子性由 R-EXCL-1 成立、与 R-EXCL-4 衔接"。实现侧删除 best-effort 拐杖 `delay_broken_until_exclusive_ends`。详见 `spec-alignment.md` 顶部本轮小结。 |
| R-EXCL-4 lint（L-EXCL-AWAIT） | 🔵 Round 2 候选 | `线程与事件循环规范.md §4.5` 的 R-EXCL-4 目前为纯散文"告知性约束"，无可锚定 lint；Round 2 实现其可静态命中子集（`#exclusive` 内 await self-handler，Warn）并补规范引用。 |

## 目录

- A. [内部矛盾](#a-内部矛盾)
- B. [语义未明确 / 欠规定](#b-语义未明确--欠规定)
- C. [重复与冗余](#c-重复与冗余)
- D. [语法/符号细节缺失](#d-语法符号细节缺失)
- E. [风格与一致性](#e-风格与一致性)
- F. [修订优先级与建议处理顺序](#f-修订优先级与建议处理顺序)

## A. 内部矛盾

### A-1 🔴 `HandlerDispatchError` 的 kind 命名前后冲突

- **冲突来源**：
  - `基础类型，表达式与函数规范草案.md:393` 表格使用 `"TargetGone"` 表示"已终止"。
  - `Tessera 核心语义.md:47, 548-550`、`标准容器与常用类型规范草案.md:521-531`、`线程与事件循环规范.md:227-229, 391-458`、`错误与异常语义草案.md` 全部使用 `TargetTerminated`。
  - `example-code.md:284-285` 也用 `Err("TargetTerminated")`。
- **影响**：用户照《基础类型...》写 `if (e.kind == "TargetTerminated")` 与照其他文档写 `if (e.kind == "TargetGone")` 二者只有一个会真正命中。实现（`runtime/src/error.rs:198-207`）采用了 `"TargetGone"`，与多数文档冲突。
- **建议修订**：以《线程与事件循环规范》《标准容器规范》为权威，把《基础类型...》§6 表格的 `"TargetGone"` 改为 `"TargetTerminated"`。

### A-2 🔴 R-HANDLER-2 "不并发执行" 与协作调度暗含的"交替推进" 张力

- **冲突来源**：
  - `线程与事件循环规范.md`：R-HANDLER-2 "同一线程在同一时刻不会并发执行两个 handler"。
  - 同文档 R-HANDLER-3："主体 ↔ handler 仅在挂起点切换"。
  - 同文档 R-SCHED-1/2：协作调度，同步段不可被打断。
- **冲突点**：当一个 handler 在自己 await 后挂起，主体可在同一线程继续推进（甚至开启 #exclusive）。同一时刻"语义执行点"只有一个，但"未结束的 handler"在挂起点之间确实与主体交替。R-HANDLER-2 字面意思（"不会并发执行"）易被误读为"必须串行至 handler 完成"。
- **影响**：实现 `event_loop.rs::dispatch_handler_inline` 显式使用 `spawn_local` 让 handler 与主体在同一 LocalSet 内交替推进（注释承认这是为了打破 readLine 等场景的死锁）。如果按字面意 R-HANDLER-2 严格化，实现需要重写为串行队列，并且 readLine 模式必须改 API。
- **建议修订**：R-HANDLER-2 改为"任一时刻最多一个执行点；handler 与主体在挂起点之间可交替推进，但同一时间不可有两条 handler 同时持有执行点"。或在 R-HANDLER-3 后增加一条 R-HANDLER-2.1 显式承认"挂起点交替"。

### A-3 🔴 `terminate()` 触发 hook 的范围与 R-LIFE-1 / R-LIFE-2 表述不完全对齐

- **冲突来源**：
  - R-LIFE-1：主体自然结束 → 不调用 `__on_terminate__`。
  - R-LIFE-2：显式 `terminate()` → 先 `__on_terminate__` 后 `__on_exit__`。
  - 但 R-EXCL-3 描述：terminate 落入 #exclusive 时"立即转 Terminating，teardown 延后"。
- **缝隙**：如果 `#exclusive` 块结束前主体也自然结束了（block 结束 = body 结束的常见模式），那么按 R-EXCL-3 是 terminate 路径，按 R-LIFE-1 又是自然结束路径。两者对是否运行 `__on_terminate__` 给出相反结论。
- **当前实现选择**：`event_loop.rs:229-246` — 选择 terminate 路径（运行 `__on_terminate__`）。
- **建议修订**：在 R-EXCL-3 末尾明示"一旦进入 Terminating 状态，即使主体随后自然结束，仍按 terminate 路径执行 teardown"。

### A-4 🔴 同步原语在线程绑定 vs 作用域绑定的"链式"语义未给出权威结论

- **冲突来源**：
  - R-SYNC-OWN-1 "首次 expose 获得线程绑定"。
  - R-SYNC-OWN-3 "@template define 字段的原语 = 作用域绑定"。
  - 但若在 `@template` 的 define 中创建 signal，再在该作用域内 spawn 一个 `$template` 并把 signal 作为参数传入再 expose ——
- **未定义场景**：此时 signal 的"绑定主"是 scope 还是子 thread？现行规范倾向"首次 expose 获得"，但与 R-SYNC-OWN-3 "scope 绑定不随线程传递"语义抵触。
- **建议修订**：明确"已绑定的原语不可被二次 expose 修改绑定主"，并把这一规则提升为 R-SYNC-OWN-4。

### A-5 🟡 `__ping__` 隐式 handler 的"用户覆盖"行为未规定

- **来源**：`example-code.md` 示例 5 与 `线程与事件循环规范.md` 的 R-HANDLER-PING（或同义条款）说"所有线程模板隐式具有 `async handler __ping__(): String`"。
- **缝隙**：
  - 用户能否手动定义同名 `__ping__`？冲突时编译错误还是用户优先？
  - `__ping__` 的存在是否影响 terminatable 判定？（不应影响，但规范未明示）
  - `__ping__` 返回值是否必须为 `"pong"`，还是允许实现自定？
- **建议修订**：增加 R-HANDLER-PING-2 "`__ping__` 由编译器隐式注入；用户定义同名 handler 触发编译错误 L-HANDLER-PING-REDEFINED"。


## B. 语义未明确 / 欠规定

### B-1 🟡 int 溢出语义

- **规范**：`基础类型... §1.1.2` 说 int 是 32 位有符号，列出 `+`/`-`/`*`/`/`/`%` 但没说溢出时是 wrap、saturate 还是 panic。
- **实现选择**：`runtime/src/value.rs` 使用 `i32`，`eval.rs:599-601` 用 `wrapping_add/sub/mul`（即 wrap-around）。
- **建议**：明确写"算术运算采用 wrap-around（二补码语义），不触发运行时错误"，或改为"溢出 → RuntimeError::IntegerOverflow"。

### B-2 🟡 char 转义集

- **规范**：§1.1.4 只说"支持转义序列"，未列出完整集合。
- **实现选择**：`lexer/src/token.rs:313-334` 支持 `\n \t \r \" \' \\ \0`，不支持 `\u{...}`、`\xHH`、`\a` 等。
- **建议**：在 §1.1.4 后增加一张表，明确列出"必须支持"和"建议支持"的转义。

### B-3 🟡 String 隐式转换的方向与格式

- **规范**：§1.1.5 提到"`+` 拼接时存在隐式转换"，但未说明：
  - `int + String`、`String + double`、`bool + String` 等所有组合的方向；
  - `double` → String 的精度（保留位数、小数表示）；
  - `bool` → String 是否一定是 `"true"`/`"false"`。
- **实现选择**：依赖 Rust 的 `.to_string()`，因此 `1.0 + ""` 会得到 `"1"`（Rust 默认）。
- **建议**：补一节"§1.1.5.3 隐式转换格式"明确两侧规则与 double 的输出格式。

### B-4 🟡 `try expr` 是否能包裹纯同步表达式

- **规范**：R-TRY-1 说 `try expr` 捕获运行时错误为 `Result<T, error>`；R-TRY-2 说 `try await expr` = `try (await expr)`。
- **缝隙**：`try 1 / 0` 在 sync 函数中是否合法？是否捕获 `DivisionByZero`？规范没显式禁止也没显式允许。
- **实现选择**：允许；捕获后产出 `Err(ErrorObj{kind:"DivisionByZero", ...})`。
- **建议**：把 R-TRY-1 改为 "`try expr` 在 sync 与 async 上下文都可使用；async 上下文可与 `await` 组合"，明确允许。

### B-5 🟡 hook 的可见性 / 调用顺序细节

- **规范**：`模板与线程规范.md` 说明 `__on_enter__` 在主体前、`__on_exit__` 在主体后；但没规定：
  - hook 内能否调用同模板的其他 member function？
  - hook 内是否能够 `expose` 字段（即在 `__on_enter__` 中赋初值给 expose）？
  - `__on_enter__` 失败后，`__on_exit__` 是否仍执行？
- **实现选择**：member function 可调用；expose 初值通常在 `__on_enter__` 中赋；`__on_enter__` 失败时 `__on_exit__` 不执行（thread crash 路径）。
- **建议**：增加"hook 语义细则"小节统一规定。

### B-6 🟡 `define` 字段的初始化时机与互相引用

- **规范**：`数据共享与并发安全规范.md` 提到 `define` 字段在模板内可见，但未规定：
  - 多个 `define` 字段的求值顺序？
  - 后续 define 的初始化器能否引用前面 define 的值？
  - `define` 与 expose 之间的求值顺序？
- **实现选择**：`event_loop.rs:63-98` 按声明顺序求值；后定义可引用先定义；但 expose 字段先于 define 求值（见 64-69 行）。
- **建议**：明确声明顺序求值 + 互相引用语义。

### B-7 🟡 `#exclusive` 块的嵌套

- **规范**：`#exclusive { ... }` 描述独占执行，但未说明：
  - 同一线程内可否嵌套 `#exclusive`？
  - 跨函数调用边界时，被调函数若也开启 #exclusive 是否冗余/报错？
- **实现选择**：实现使用 `exclusive_mode: watch<bool>`，嵌套会被压平（内层结束时整体退出 exclusive）—— 存在 bug 风险。
- **建议**：明确"嵌套 #exclusive 是合法的；计数式跟踪，最外层结束才退出"或"嵌套是 compile-time 错误"。

### B-8 🟡 `keepalive()` 与 `getchar()` 的归属

- **规范**：14 份文档没有专门一节定义这两个内建函数。`keepalive()` 出现在 R-KEEPALIVE-1/2 但作为符号未被引入；`getchar()` 只在 example-code 出现。
- **实现选择**：作为 builtin 函数。
- **建议**：在《标准容器与常用类型规范》末尾或独立"§内建函数列表"中补全 `print/println/asleep/keepalive/getchar/signal/contract/permit/locked` 的签名与语义。

### B-9 🟡 `Err("TargetCrashed")` 字符串比较的类型规则

- **规范**：`README.md:67`、`example-code.md:284`、多处显示 `hf == Err("TargetCrashed")`。
- **缝隙**：此处 `Err("...")` 是构造 `Result<_, String>` 还是 `HandlerDispatchError` 字面量？`hf` 是 `HandlerFuture<R>`，与 Result 怎样比较？
- **实现选择**：`runtime/src/error.rs::HandlerDispatchError::Display` impl + 字符串值 + 自定 `==` 路径推断。但这是规范没明示的语法捷径。
- **建议**：要么把 `hf == Err("kind")` 列为规范的内置语法糖（明示其展开），要么改用 `hf.isErr() && hf.errKind() == "..."` 等显式 API。

### B-10 🟡 `Future<T>` 与 `HandlerFuture<R>` 的子类型关系

- **规范**：多处说"HandlerFuture wait 语义类似 Future"，但没明示二者是同类型、子类型还是无关联。
- **实现选择**：`HandlerFuture` 是独立 Value 变体；不能赋给 `Future<T>` 变量。
- **建议**：明确"HandlerFuture<R> 与 Future<R> 类型不相容；二者无隐式转换"。

### B-11 🟡 generic 类型推断的边界

- **规范**：泛型规范说"类型参数必须显式"，但 `let v = List<int>()` 之后 `v.push(...)` 推断什么类型？`let x = Some(1)` 是 `Option<int>` 吗？
- **实现选择**：`Some(1)`、`Ok(1)` 通过 TypeCtor 路径返回 `Option<int>` / `Result<int, _>`；后者的 E 未确定。
- **建议**：补充"Some(v)/Ok(v) 的 E/T 推断规则"以及"None / Err 的 placeholder 类型"。

### B-12 🟡 顶层执行模型

- **规范**：核心语义里说"顶层执行 main 线程"，但顶层语句能否调用 async function？顶层是否处于隐式 async 上下文？
- **实现选择**：顶层是 async 上下文（`run_thread_task` 即顶层 driver），但 parser 允许 `await` 出现在任何 primary 位置。
- **建议**：明确"顶层执行环境的同步/异步性"以及"顶层 `await` 是否合法"。


## C. 重复与冗余

### C-1 🔵 R-SCHED-1/2 在多文档中重复表述

- 《Tessera 核心语义》R-CORE-SCHED-1/2 与《线程与事件循环规范》R-SCHED-1/2 含义相同但编号不同。
- **风险**：将来修订一处而忘了另一处，导致两文档相互矛盾。
- **建议**：以《线程与事件循环规范》为权威源，《核心语义》中以"参见 R-SCHED-1"形式引用。

### C-2 🔵 R-CORE-HANDLER-1 与 R-SYNC-NOORPHAN-1 实质等价

- 二者都在说"主从对象死亡不留孤立等待者"。
- **建议**：保留 R-SYNC-NOORPHAN-1，将 R-CORE-HANDLER-1 改为"参见 R-SYNC-NOORPHAN-1（applied to handler futures）"。

### C-3 🔵 README、核心语义、错误与异常 三处都有"业务失败 vs 运行时错误"概述

- 三段表述措辞不同，细节略有出入（例如对 `try` 的描述顺序不同）。
- **建议**：以《错误与异常语义草案》为权威源，README 与核心语义改为"简介 + 链接"。

### C-4 🔵 同一规则同时以 R-XXX 与散文形式存在

- 多处出现"规则 R-XXX-Y：xxx。"紧接一段散文重述同样内容。
- **建议**：规则体保留单一权威表述；散文部分改为"动机/例子"。


## D. 语法 / 符号细节缺失

### D-1 🟡 匿名 scope 模板 `@{ ... }` 是否合法？

- **来源**：规范多处提"线程模板可匿名 `${ ... }`"，但是否存在 scope 模板的匿名形式 `@{ ... }`？
- **实现**：parser 不支持 `@{`（只支持 `@Name(...)` 或 `@template ... { ... } { ... }` 的内联匿名）。
- **建议**：明确"scope 模板的匿名简写不存在；如需匿名，使用 `@template { hooks } { body }` 形式"。

### D-2 🟡 `:=` post-bind 操作符的优先级与可应用对象

- **规范**：`thread spawn 后 := h` 绑定句柄；但 `:=` 是否还有其它用法？是否参与表达式优先级？
- **实现**：`:=` 只在 thread spawn 语句末尾出现，不进入表达式。
- **建议**：明确"`:=` 是语句级 post-bind 操作符，仅用于 thread spawn"。

### D-3 🟡 `signal`、`contract`、`permit`、`locked` 的大小写

- **规范**：标准容器规范用 `signal` / `contract` / `permit` 全小写；类型构造器表又出现 `Signal` / `Contract` / `Permit` 大写。
- **实现**：Value 变体 `Signal/Contract/Permit/Locked` 大写；构造调用 `signal()/contract()/permit(...)/locked(v)` 全小写。
- **建议**：统一约定"类型名首字母大写，构造函数全小写"，并在标准容器规范开头声明。

### D-4 🟡 `expose_mutable` 是否能取 `expose` 的引用？

- **规范**：未列出"外部赋值"以外的禁用动作。
- **缝隙**：`let r = handle.shared;` 是合法别名（外部别名持有 locked<T>）还是禁用？
- **实现**：clone 路径合法。
- **建议**：明确"通过 `let` 取得 expose_mutable 字段的引用是合法的；引用与原字段共享同一 `locked<T>` 实例"。

### D-5 🟡 顶层 thread 与 anonymous thread 的句法占位

- **规范**：`$Name(...) { body } := h;` 与 `${ body };` 的语句位置规则未列。
- **实现**：parser 允许出现在任意 stmt 位置，包括 if/while 块内。
- **建议**：明确"thread spawn 可出现在任意语句位置；spawn 创建的句柄生命周期与外层 scope 相关但不被作用域 break"。

### D-6 🟡 `try` 后的表达式语法范围

- **规范**：`try expr` 中 expr 包含哪些范围？`try a + b` 是 `(try a) + b` 还是 `try (a + b)`？
- **实现**：parser 中 `try` 是 primary-level prefix，等同于 `try (postfix-expression)`；不延伸到二元运算。
- **建议**：明确"`try` 是高于一元运算符但不延伸到二元运算的 prefix；如需包裹复合表达式，应写 `try (expr)`"。


## E. 风格与一致性

### E-1 🔵 规则编号格式不统一

- 出现：`R-CORE-SCHED-1`、`R-LIFE-1`、`R-EXCL-3`、`R-SYNC-NOORPHAN-1`、`R-EXPOSE-MUTABLE-SAFE-1`、`L-AWAIT-ASYNC-ONLY` 等。
- **问题**：前缀长度（CORE/LIFE/EXCL）、分级位数（1 vs 1-2 段）、单数复数不一致。
- **建议**：统一为 `R-<DOMAIN>-<TOPIC>-<N>` 与 `L-<DOMAIN>-<TOPIC>` 两种格式；每个规则编号在仓库内全局唯一。

### E-2 🔵 Linter Severity 边界模糊

- `Linter 规则草案.md` 标 Error / Warn / Info，但部分规则的分类难以辨别（如 L-SYNC-AWAIT-NOCHECK 被标 "Info/Warn"）。
- **建议**：明确给每条 lint 一个固定 Severity；如要可调，需声明默认级别。

### E-3 🔵 规则与"建议/动机"段落混排

- 部分文档先写规则、又用一段散文重述同样内容；阅读时容易混淆"哪部分是规范"。
- **建议**：使用统一的版式（如 ⟦RULE⟧ + ⟦Rationale⟧ + ⟦Example⟧ 三段），机读友好。

### E-4 🔵 example-code.md 的代码与规范一致性未机器校验

- 示例代码中的方法签名 / 错误 kind 字符串 / 函数名 偶有与权威章节出入（已发现 `Err("TargetTerminated")` vs `"TargetGone"` 问题）。
- **建议**：把 example-code.md 中所有 .tss 块抽出来加入 `tests/tss/`，作为"spec compliance test"。

### E-5 🔵 R-XXX 规则未在文档头部聚合

- 阅读时缺乏总目录；目前要 grep 才能找到所有规则编号。
- **建议**：在每份规范文档顶部加一张"本文规则总览"小表（编号 + 一句话）。

### E-6 🔵 14 份文档之间互链稀疏

- 大量"详见某章节"是手写，链接易腐烂；某些章节标题在跳转时不带锚点。
- **建议**：在仓库内引入 Markdown 锚点 lint（CI 跑），并把跨文档引用集中到一张索引表。


## F. 修订优先级与建议处理顺序

### 第一批（解除二义性，使实现可被唯一裁定）

1. **A-1** TargetTerminated / TargetGone 命名统一（与实现侧 P0 配套修复）。
2. **A-2** R-HANDLER-2 措辞改为"挂起点之间可交替推进"。
3. **A-3** R-EXCL-3 + R-LIFE 边界澄清。
4. **A-4** 同步原语链式绑定规则定稿。

### 第二批（补全语义细节）

5. **B-1, B-2, B-3** 数值与字符串：溢出语义、char 转义、隐式转换格式。
6. **B-5, B-6** hook / define 字段的求值顺序与可见性。
7. **B-7** `#exclusive` 嵌套的语义。
8. **B-8** 内建函数总表（`keepalive` / `getchar` / `signal` / ...）。
9. **B-9, B-10** `Err("...")` 比较语法与 Future/HandlerFuture 关系。
10. **A-5, B-11, B-12** `__ping__` 覆盖、泛型推断、顶层执行模型。

### 第三批（清理与规范化）

11. **C-1 ~ C-4** 合并/引用，去除冗余。
12. **D-1 ~ D-6** 语法细节填补。
13. **E-1 ~ E-6** 编号、Severity、跨文档链接整顿。
14. **E-4** 引入 example-code → 测试套件的自动化。

### 与实现侧 TODO 的对照

本文档每条规范侧问题在 `docs/spec-alignment.md §15` 中至少有一条对应的实现侧 TODO（或将随规范修订消失）。两文件应同步维护：

| spec-issues 条目 | spec-alignment §15 中关联项 |
|---|---|
| A-1 | P0-1（统一 HandlerDispatchError kind） |
| A-2 | §14 偏差 1（保持实现，修订规范） |
| A-3 | （新增）event_loop 终止路径在 R-EXCL-3 + R-LIFE 边界场景的测试 |
| A-5 | P0-2（注入隐式 `__ping__`） |
| B-1 | P0-3（int 溢出诊断） |
| B-2 | P1-12（char Unicode 转义） |
| B-7 | （新增）#exclusive 嵌套测试 |
| B-8 | P2-15（标准容器扩展方法 / 内建函数列表） |
| D-1 | P2-18（example 5/7/8 端到端测试） |

---

*本文档由 spec-alignment 审计（commit `bbda9b1`，2026-05-29）生成。规范作者可对每条进行 Accept/Reject/Defer，更新时请同步刷新 `docs/spec-alignment.md`。*

