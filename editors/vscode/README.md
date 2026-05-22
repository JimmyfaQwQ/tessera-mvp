# Tessera VS Code Extension

Provides **syntax highlighting**, **language configuration**, and **code snippets** for the [Tessera](https://github.com/JimmyfaQwQ/tessera-mvp) language (`.tss` files).

## Features

### Syntax Highlighting

All Tessera language constructs are highlighted:

| Construct | Examples |
|---|---|
| Sigil keywords | `$template`, `@template`, `${`, `#exclusive` |
| Thread instantiation | `$Worker(...)` |
| Lifecycle hooks | `__on_enter__`, `__on_exit__`, `__on_terminate__` |
| Declaration keywords | `function`, `async`, `handler`, `expose`, `expose_mutable`, `let` |
| Control flow | `if`, `else`, `while`, `for`, `break`, `continue`, `return`, `await` |
| Error keywords | `panic`, `assert` |
| Primitive types | `bool`, `int`, `double`, `char`, `String`, `void`, `never` |
| Built-in types | `Queue`, `Option`, `Result`, `locked`, `List`, `Map`, `HandlerFuture` |
| Boolean literals | `true`, `false` |
| Numeric literals | `42`, `3.14` |
| String & char literals | `"hello"`, `'x'` |
| Comments | `// line comment`, `/* block comment */` |
| Post-bind operator | `} := handle` |

### Language Configuration

- **Comment toggling**: `//` (line), `/* */` (block) via `Ctrl+/` / `Shift+Alt+A`
- **Auto-closing pairs**: `{}`, `()`, `[]`, `""`, `''`
- **Bracket matching**: `{}`, `()`, `[]`
- **Auto-indentation** after `{`

### Snippets

| Prefix | Description |
|---|---|
| `$template` | Thread template with full lifecycle hooks |
| `@template` | Scope template |
| `${}` | Anonymous thread `${ ... }` |
| `thread` | Thread instantiation with handle bind |
| `handler` | `async handler` declaration |
| `fn` | `function` declaration |
| `afn` | `async function` declaration |
| `expose` | `expose` field |
| `expose_mutable` | `expose_mutable locked<T>` field |
| `let` | Variable declaration |
| `for` | C-style for loop |
| `while` | while loop |
| `if` | if block |
| `ifelse` | if/else block |
| `result` | `Result<T, E>` pattern with `.isOk()` check |
| `option` | `Option<T>` pattern with `.isNone()` check |

## Installation

### From source

1. Copy or symlink the `editors/vscode/` directory into your VS Code extensions folder:
   - **Windows**: `%USERPROFILE%\.vscode\extensions\tessera-0.1.0`
   - **macOS/Linux**: `~/.vscode/extensions/tessera-0.1.0`
2. Restart VS Code.

### Via VSIX (once packaged)

```
code --install-extension tessera-0.1.0.vsix
```

> Requires `vsce` to package: `npm install -g @vscode/vsce && vsce package` from this directory.

## File Association

The extension automatically activates for files with the `.tss` extension.
