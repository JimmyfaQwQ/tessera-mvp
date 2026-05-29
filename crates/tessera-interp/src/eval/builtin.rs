//! Built-in method dispatch (`Value::X.method(args)`).
//!
//! Extracted from `mod.rs` because it's the single largest method in the
//! interpreter (~330 lines of `match (method, receiver)` arms) and gets in the
//! way of navigating control flow. Stays as an `impl Interpreter` block so the
//! method lookup matches the rest of the eval surface.

use tessera_ast::*;
use tessera_runtime::{
    FutureOutcome, HandlerResolveResult, QueuePushError, RuntimeError, Value, ValueKey,
};

use super::Interpreter;

impl Interpreter {
    pub(super) async fn eval_method_call(&self, m: &MethodCallExpr) -> Result<Value, RuntimeError> {
        let recv = self.eval_expr(&m.receiver).await?;
        let mut args = Vec::new();
        for a in &m.args { args.push(self.eval_expr(a).await?); }

        match (&m.method.name[..], recv.clone()) {
            // ── Option ────────────────────────────────────────────────────────
            ("isSome", Value::Option(v)) => Ok(Value::Bool(v.is_some())),
            ("isNone", Value::Option(v)) => Ok(Value::Bool(v.is_none())),
            ("unwrap", Value::Option(Some(v))) => Ok(*v),
            ("unwrap", Value::Option(None)) => Err(RuntimeError::UnwrapNone { location: m.span }),
            ("unwrapOr", Value::Option(Some(v))) => Ok(*v),
            ("unwrapOr", Value::Option(None)) => Ok(args.into_iter().next().unwrap_or(Value::Void)),

            // ── Result ────────────────────────────────────────────────────────
            ("isOk",  Value::Result(r)) => Ok(Value::Bool(r.is_ok())),
            ("isErr", Value::Result(r)) => Ok(Value::Bool(r.is_err())),
            ("unwrap",    Value::Result(Ok(v)))  => Ok(*v),
            ("unwrap",    Value::Result(Err(_))) => Err(RuntimeError::UnwrapErr { location: m.span }),
            ("unwrapErr", Value::Result(Err(e))) => Ok(*e),
            ("unwrapErr", Value::Result(Ok(_)))  => Err(RuntimeError::UnwrapErr { location: m.span }),
            ("unwrapOr",  Value::Result(Ok(v)))  => Ok(*v),
            ("unwrapOr",  Value::Result(Err(_))) => Ok(args.into_iter().next().unwrap_or(Value::Void)),

            // ── List ──────────────────────────────────────────────────────────
            ("length", Value::List(l)) => Ok(Value::Int(l.borrow().len() as i32)),
            ("length", Value::Str(s))  => Ok(Value::Int(s.chars().count() as i32)),
            ("isEmpty", Value::List(l)) => Ok(Value::Bool(l.borrow().is_empty())),

            // ── Standard-library extensions (round 3) ─────────────────────────
            ("startsWith", Value::Str(s)) => {
                let prefix = match args.first() {
                    Some(Value::Str(p)) => p.clone(),
                    _ => return Err(RuntimeError::Panic {
                        message: "String.startsWith(prefix: String) requires a String argument".into(),
                        location: m.span,
                    }),
                };
                Ok(Value::Bool(s.starts_with(&prefix)))
            }
            ("endsWith", Value::Str(s)) => {
                let suffix = match args.first() {
                    Some(Value::Str(p)) => p.clone(),
                    _ => return Err(RuntimeError::Panic {
                        message: "String.endsWith(suffix: String) requires a String argument".into(),
                        location: m.span,
                    }),
                };
                Ok(Value::Bool(s.ends_with(&suffix)))
            }
            ("contains", Value::Str(s)) => {
                let needle = match args.first() {
                    Some(Value::Str(p)) => p.clone(),
                    _ => return Err(RuntimeError::Panic {
                        message: "String.contains(needle: String) requires a String argument".into(),
                        location: m.span,
                    }),
                };
                Ok(Value::Bool(s.contains(&needle)))
            }
            ("indexOf", Value::Str(s)) => {
                let needle = match args.first() {
                    Some(Value::Str(p)) => p.clone(),
                    _ => return Err(RuntimeError::Panic {
                        message: "String.indexOf(needle: String) requires a String argument".into(),
                        location: m.span,
                    }),
                };
                // Byte offset → Unicode-scalar index so the return value
                // composes with String[i] (which is scalar-indexed).
                let idx = s.find(&needle).map(|byte_off| {
                    s[..byte_off].chars().count() as i32
                }).unwrap_or(-1);
                Ok(Value::Int(idx))
            }
            ("trim", Value::Str(s)) => Ok(Value::Str(s.trim().to_string())),
            ("split", Value::Str(s)) => {
                let sep = match args.first() {
                    Some(Value::Str(p)) => p.clone(),
                    _ => return Err(RuntimeError::Panic {
                        message: "String.split(sep: String) requires a String argument".into(),
                        location: m.span,
                    }),
                };
                let pieces: Vec<Value> = if sep.is_empty() {
                    s.chars().map(|c| Value::Str(c.to_string())).collect()
                } else {
                    s.split(&sep).map(|p| Value::Str(p.to_string())).collect()
                };
                Ok(Value::List(std::rc::Rc::new(std::cell::RefCell::new(pieces))))
            }
            ("contains", Value::List(l)) => {
                let needle = args.first().cloned().unwrap_or(Value::Void);
                let l = l.borrow();
                let found = l.iter().any(|v| super::helpers::values_equal(v, &needle));
                Ok(Value::Bool(found))
            }
            ("indexOf", Value::List(l)) => {
                let needle = args.first().cloned().unwrap_or(Value::Void);
                let l = l.borrow();
                let idx = l.iter().position(|v| super::helpers::values_equal(v, &needle))
                    .map(|i| i as i32)
                    .unwrap_or(-1);
                Ok(Value::Int(idx))
            }
            ("clear", Value::List(l)) => {
                l.borrow_mut().clear();
                Ok(Value::Void)
            }
            ("contains", Value::Map(m_val)) => {
                let key = match args.into_iter().next() {
                    Some(k) => k,
                    None => return Err(RuntimeError::Panic {
                        message: "Map.contains(key) requires a key".into(),
                        location: m.span,
                    }),
                };
                match ValueKey::try_from(key) {
                    Ok(vk) => Ok(Value::Bool(m_val.borrow().contains_key(&vk))),
                    Err(_) => Err(RuntimeError::Panic {
                        message: "Map.contains() key must be bool/int/char/String".into(),
                        location: m.span,
                    }),
                }
            }
            ("isDigit", Value::Char(c)) => Ok(Value::Bool(c.is_ascii_digit())),
            ("isAlpha", Value::Char(c)) => Ok(Value::Bool(c.is_ascii_alphabetic())),
            ("isWhitespace", Value::Char(c)) => Ok(Value::Bool(matches!(c, ' ' | '\t' | '\n' | '\r'))),


            // ── Type conversions ───────────────────────────────────────────────
            ("toString", Value::Int(n))    => Ok(Value::Str(n.to_string())),
            ("toString", Value::Double(f)) => Ok(Value::Str(f.to_string())),
            ("toString", Value::Char(c))   => Ok(Value::Str(c.to_string())),
            ("toString", Value::Bool(b))   => Ok(Value::Str(b.to_string())),
            ("toInt",    Value::Double(f)) => Ok(Value::Int(f as i32)),
            ("toInt",    Value::Char(c))   => Ok(Value::Int(c as i32)),
            ("toInt",    Value::Str(s)) => Ok(Value::Result(
                s.trim().parse::<i32>()
                    .map(|n| Box::new(Value::Int(n)))
                    .map_err(|e| Box::new(Value::Str(e.to_string())))
            )),
            ("toDouble", Value::Int(n))  => Ok(Value::Double(n as f64)),
            ("toDouble", Value::Str(s))  => Ok(Value::Result(
                s.trim().parse::<f64>()
                    .map(|f| Box::new(Value::Double(f)))
                    .map_err(|e| Box::new(Value::Str(e.to_string())))
            )),
            ("toChar",   Value::Int(n)) => Ok(Value::Option(
                char::from_u32(n as u32).map(|c| Box::new(Value::Char(c)))
            )),
            ("push", Value::List(l)) => {
                l.borrow_mut().push(args.into_iter().next().unwrap_or(Value::Void));
                Ok(Value::Void)
            }
            ("pop", Value::List(l)) => Ok(
                l.borrow_mut().pop()
                    .map(|v| Value::Option(Some(Box::new(v))))
                    .unwrap_or(Value::Option(None))
            ),
            ("get", Value::List(l)) => {
                if let Some(Value::Int(i)) = args.first() {
                    let l = l.borrow();
                    let idx = *i as usize;
                    if idx >= l.len() {
                        return Err(RuntimeError::IndexOutOfBounds {
                            index: *i, length: l.len() as i32, location: m.span,
                        });
                    }
                    Ok(l[idx].clone())
                } else {
                    Err(RuntimeError::Panic {
                        message: "List.get() requires an int index".into(),
                        location: m.span,
                    })
                }
            }
            ("set", Value::List(l)) => {
                if let (Some(Value::Int(i)), Some(v)) = (args.first(), args.get(1)) {
                    let mut l = l.borrow_mut();
                    let idx = *i as usize;
                    if idx >= l.len() {
                        return Err(RuntimeError::IndexOutOfBounds {
                            index: *i, length: l.len() as i32, location: m.span,
                        });
                    }
                    l[idx] = v.clone();
                    Ok(Value::Void)
                } else {
                    Err(RuntimeError::Panic {
                        message: "List.set() requires an int index and a value".into(),
                        location: m.span,
                    })
                }
            }

            // ── Map ───────────────────────────────────────────────────────────
            ("size", Value::Map(m_val)) => Ok(Value::Int(m_val.borrow().len() as i32)),
            ("get", Value::Map(m_val)) => {
                let key = args.into_iter().next();
                let result = key.and_then(|k| {
                    ValueKey::try_from(k).ok()
                        .and_then(|vk| m_val.borrow().get(&vk).cloned())
                });
                Ok(Value::Option(result.map(Box::new)))
            }
            ("set", Value::Map(m_val)) => {
                if let (Some(key), Some(val)) = (args.first().cloned(), args.get(1).cloned()) {
                    match ValueKey::try_from(key) {
                        Ok(vk) => { m_val.borrow_mut().insert(vk, val); }
                        Err(_) => return Err(RuntimeError::Panic {
                            message: "Map.set() key type is not hashable (must be bool/int/char/String)".into(),
                            location: m.span,
                        }),
                    }
                } else {
                    return Err(RuntimeError::Panic {
                        message: "Map.set() requires a key and a value".into(),
                        location: m.span,
                    });
                }
                Ok(Value::Void)
            }
            ("remove", Value::Map(m_val)) => {
                let key = args.into_iter().next();
                let removed = key.and_then(|k| {
                    ValueKey::try_from(k).ok()
                        .and_then(|vk| m_val.borrow_mut().shift_remove(&vk))
                });
                Ok(Value::Option(removed.map(Box::new)))
            }

            // ── Future .wait() / .isDone() ───────────────────────────────────
            ("wait", Value::Future(fut)) => {
                match fut.resolve().await {
                    FutureOutcome::Ok(v) => Ok(v),
                    FutureOutcome::Failed(msg) => Err(RuntimeError::Panic { message: msg, location: m.span }),
                }
            }
            ("isDone", Value::Future(fut)) => Ok(Value::Bool(fut.is_done())),

            // ── HandlerFuture .wait() (sync) / state checks ───────────────────
            ("wait", Value::HandlerFuture(hf)) => {
                match hf.resolve().await {
                    HandlerResolveResult::Ok(v) => Ok(v),
                    HandlerResolveResult::DispatchFailed(e) => {
                        let (kind, message) = e.kind_and_message();
                        Err(RuntimeError::Structured { kind, message, location: m.span })
                    }
                    HandlerResolveResult::ExecutionFailed(msg) =>
                        Err(RuntimeError::Structured {
                            kind: "ExecutionFailed".into(),
                            message: msg,
                            location: m.span,
                        }),
                }
            }
            ("isDone", Value::HandlerFuture(hf)) => Ok(Value::Bool(hf.is_done())),
            ("isOk",   Value::HandlerFuture(hf)) => Ok(Value::Bool(hf.is_ok())),
            ("isErr",  Value::HandlerFuture(hf)) => Ok(Value::Bool(hf.is_err())),

            // ── ThreadHandle .terminate() ─────────────────────────────────────
            ("terminate", Value::ThreadHandle(state)) => {
                if !state.is_terminatable {
                    return Err(RuntimeError::Panic {
                        message: format!(
                            "thread '{}' is not terminatable (no __on_terminate__ declared)",
                            state.template_name.as_deref().unwrap_or("<anonymous>")
                        ),
                        location: m.span,
                    });
                }
                let fut = state.terminate().await;
                Ok(Value::Future(fut))
            }

            // ── ThreadHandle handler calls ────────────────────────────────────
            (method, Value::ThreadHandle(state)) => {
                let hf_result = state.dispatch_handler(method.to_string(), args).await;
                match hf_result {
                    Ok(fut) => Ok(Value::HandlerFuture(tessera_runtime::TesseraHandlerFuture::from_future(fut))),
                    Err(e)  => Ok(Value::HandlerFuture(tessera_runtime::TesseraHandlerFuture::rejected(e))),
                }
            }

            // ── Queue ─────────────────────────────────────────────────────────
            ("push", Value::Queue(q)) => {
                let v = args.into_iter().next().unwrap_or(Value::Void);
                match q.push(v) {
                    Ok(()) => Ok(Value::Result(Ok(Box::new(Value::Void)))),
                    Err(QueuePushError::Full)   => Ok(Value::Result(Err(Box::new(Value::Str("Full".into()))))),
                    Err(QueuePushError::Closed) => Ok(Value::Result(Err(Box::new(Value::Str("Closed".into()))))),
                }
            }
            ("enqueue", Value::Queue(q)) => {
                let v = args.into_iter().next().unwrap_or(Value::Void);
                if !q.enqueue(v).await {
                    return Err(RuntimeError::Panic {
                        message: "QueueClosed".into(),
                        location: m.span,
                    });
                }
                Ok(Value::Void)
            }
            ("dequeue", Value::Queue(q)) => Ok(Value::Option(q.dequeue().await.map(Box::new))),
            ("tryPush", Value::Queue(q)) => {
                let v = args.into_iter().next().unwrap_or(Value::Void);
                Ok(Value::Bool(q.try_push(v)))
            }
            ("tryPop", Value::Queue(q)) => Ok(Value::Option(q.try_pop().map(Box::new))),
            ("size",    Value::Queue(q)) => Ok(Value::Int(q.size() as i32)),
            ("isEmpty", Value::Queue(q)) => Ok(Value::Bool(q.is_empty())),
            ("isClosed",Value::Queue(q)) => Ok(Value::Bool(q.is_closed())),
            ("waitForNonEmpty", Value::Queue(q)) => { q.wait_for_non_empty().await; Ok(Value::Void) }
            ("close",   Value::Queue(q)) => { q.close(); Ok(Value::Void) }

            // ── locked<T> ─────────────────────────────────────────────────────
            ("lock", Value::Locked(l)) => {
                let owner_id = self.current_thread_id();
                match l.lock(owner_id).await {
                    Ok(()) => Ok(Value::Void),
                    Err(()) => Err(RuntimeError::ReentrantLock { location: m.span }),
                }
            }
            ("tryLock", Value::Locked(l)) => {
                let owner_id = self.current_thread_id();
                match l.try_lock(owner_id) {
                    Ok(acquired) => Ok(Value::Bool(acquired)),
                    Err(()) => Err(RuntimeError::ReentrantLock { location: m.span }),
                }
            }
            ("unlock", Value::Locked(l)) => {
                let owner_id = self.current_thread_id();
                if l.unlock(owner_id) {
                    Ok(Value::Void)
                } else {
                    Err(RuntimeError::UnlockNotOwned { location: m.span })
                }
            }
            ("isLocked", Value::Locked(l)) => Ok(Value::Bool(l.is_locked())),
            ("get", Value::Locked(l)) => Ok(l.get().await),
            ("set", Value::Locked(l)) => {
                let v = args.into_iter().next().unwrap_or(Value::Void);
                l.set(v).await;
                Ok(Value::Void)
            }

            // ── signal ────────────────────────────────────────────────────────
            ("raise",    Value::Signal(s)) => { s.raise();  Ok(Value::Void) }
            ("reset",    Value::Signal(s)) => { s.reset();  Ok(Value::Void) }
            ("isRaised", Value::Signal(s)) => Ok(Value::Bool(s.is_raised())),
            ("isOk",     Value::Signal(s)) => Ok(Value::Bool(s.is_ok())),
            ("isErr",    Value::Signal(s)) => Ok(Value::Bool(s.is_err())),
            ("wait",     Value::Signal(s)) => {
                match s.wait().await {
                    Ok(()) => Ok(Value::Void),
                    Err(r) => {
                        Err(RuntimeError::Structured {
                            kind: r.as_str().into(),
                            message: format!("signal broken: {}", r.as_str()),
                            location: m.span,
                        })
                    }
                }
            }

            // ── contract ──────────────────────────────────────────────────────
            ("fulfill",   Value::Contract(c)) => { c.fulfill(); Ok(Value::Void) }
            ("isPending", Value::Contract(c)) => Ok(Value::Bool(c.is_pending())),
            ("isOk",      Value::Contract(c)) => Ok(Value::Bool(c.is_ok())),
            ("isErr",     Value::Contract(c)) => Ok(Value::Bool(c.is_err())),
            ("wait",      Value::Contract(c)) => {
                match c.wait().await {
                    Ok(()) => Ok(Value::Void),
                    Err(r) => {
                        Err(RuntimeError::Structured {
                            kind: r.as_str().into(),
                            message: format!("contract broken: {}", r.as_str()),
                            location: m.span,
                        })
                    }
                }
            }

            // ── permit ────────────────────────────────────────────────────────
            ("release", Value::Permit(p)) => {
                let n = args.into_iter().next();
                match n {
                    Some(Value::Int(n)) => {
                        if n <= 0 {
                            return Err(RuntimeError::Panic {
                                message: format!("permit.release(n): n must be positive, got {n}"),
                                location: m.span,
                            });
                        }
                        p.release_n(n);
                    }
                    None => p.release(),
                    _ => return Err(RuntimeError::Panic {
                        message: "permit.release(n): n must be an int".into(),
                        location: m.span,
                    }),
                }
                Ok(Value::Void)
            }
            ("count",   Value::Permit(p)) => Ok(Value::Int(p.count())),
            ("isOk",   Value::Permit(p)) => Ok(Value::Bool(p.is_ok())),
            ("isErr",  Value::Permit(p)) => Ok(Value::Bool(p.is_err())),
            ("wait",   Value::Permit(p)) => {
                match p.acquire().await {
                    Ok(()) => Ok(Value::Void),
                    Err(r) => {
                        Err(RuntimeError::Structured {
                            kind: r.as_str().into(),
                            message: format!("permit broken: {}", r.as_str()),
                            location: m.span,
                        })
                    }
                }
            }

            (method, recv) => Err(RuntimeError::Panic {
                message: format!("no method '{}' on type {}", method, recv.type_name()),
                location: m.span,
            }),
        }
    }

    /// Returns the current Tessera thread's stable identifier.
    ///
    /// Used as `owner_id` for `locked<T>` explicit locking. Falls back to `0`
    /// (never produced by `ThreadState::new`'s counter, which starts at 1) when
    /// no thread state is bound — top-level code outside any spawned thread.
    pub(super) fn current_thread_id(&self) -> tessera_runtime::ThreadId {
        self.0.current_thread_state.borrow()
            .as_ref()
            .map(|arc| arc.id)
            .unwrap_or(0)
    }
}

