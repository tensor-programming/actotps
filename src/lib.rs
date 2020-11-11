mod actor;
mod callback;
mod context;
mod message;
mod pid;
mod registry;

pub type Result<T> = anyhow::Result<T>;
pub type Error = anyhow::Error;

pub use actor::{Actor, Listener, StreamListener};
pub use callback::{Caller, Sender};
pub use context::Context;
pub use message::{ActorEvent, Message};
pub use pid::{Pid, WeakPid};
pub use registry::{Registry, Service};
