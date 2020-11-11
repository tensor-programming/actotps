use crate::context::Context;
use crate::Error;

use futures::Future;
use std::pin::Pin;

pub trait Message: 'static + Send {
    type Result: 'static + Send;
}

type ExecFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

pub type ExecCallback<A> =
    Box<dyn for<'a> FnOnce(&'a mut A, &'a mut Context<A>) -> ExecFuture<'a> + Send + 'static>;

pub enum ActorEvent<A> {
    Exec(ExecCallback<A>),
    Stop(Option<Error>),
    DropListener(usize),
}
