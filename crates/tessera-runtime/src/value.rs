use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use indexmap::IndexMap;

use crate::{TesseraLocked, TesseraQueue, TesseraFuture, TesseraHandlerFuture, ThreadState,
            TesseraSignal, TesseraContract, TesseraPermit};

/// Runtime value representation.
///
/// List/Map use Rc<RefCell<...>> because the spec forbids cross-thread capture
/// of non-concurrent-safe values; they are therefore single-threaded.
/// locked<T>, Queue<T>, signal, contract, and ThreadHandle use Arc for safe cross-thread sharing.
#[derive(Clone, Debug)]
pub enum Value {
    Bool(bool),
    Int(i32),
    Double(f64),
    Char(char),
    Str(String),
    Void,
    Never,

    List(Rc<RefCell<Vec<Value>>>),
    Map(Rc<RefCell<IndexMap<ValueKey, Value>>>),
    Option(Option<Box<Value>>),
    Result(std::result::Result<Box<Value>, Box<Value>>),

    Locked(Arc<TesseraLocked>),
    Queue(Arc<TesseraQueue>),
    Signal(Arc<TesseraSignal>),
    Contract(Arc<TesseraContract>),
    Permit(Arc<TesseraPermit>),

    Future(TesseraFuture),
    HandlerFuture(TesseraHandlerFuture),
    ThreadHandle(Arc<ThreadState>),

    /// Implicit `self` object inside thread template methods.
    /// Shared by Rc across all mini-threads spawned from the same template instance.
    Object(Rc<RefCell<HashMap<String, Value>>>),
}

/// Keys valid in a Map (must be hashable).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ValueKey {
    Bool(bool),
    Int(i32),
    Char(char),
    Str(String),
}

impl TryFrom<Value> for ValueKey {
    type Error = ();
    fn try_from(v: Value) -> std::result::Result<Self, ()> {
        match v {
            Value::Bool(b) => Ok(ValueKey::Bool(b)),
            Value::Int(i)  => Ok(ValueKey::Int(i)),
            Value::Char(c) => Ok(ValueKey::Char(c)),
            Value::Str(s)  => Ok(ValueKey::Str(s)),
            _ => Err(()),
        }
    }
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Double(_) => "double",
            Value::Char(_) => "char",
            Value::Str(_) => "String",
            Value::Void => "void",
            Value::Never => "never",
            Value::List(_) => "List",
            Value::Map(_) => "Map",
            Value::Option(_) => "Option",
            Value::Result(_) => "Result",
            Value::Locked(_) => "locked",
            Value::Queue(_) => "Queue",
            Value::Signal(_) => "signal",
            Value::Contract(_) => "contract",
            Value::Permit(_) => "permit",
            Value::Future(_) => "Future",
            Value::HandlerFuture(_) => "HandlerFuture",
            Value::ThreadHandle(_) => "ThreadHandle",
            Value::Object(_) => "Object",
        }
    }
}
