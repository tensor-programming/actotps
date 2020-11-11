use std::{
    future::Future,
    hash::{Hash, Hasher},
    pin::Pin,
};

use crate::message::Message;

use crate::Result;

pub type Callback<T> = Box<
    dyn Fn(T) -> Pin<Box<dyn Future<Output = Result<<T as Message>::Result>> + Send + 'static>>
        + Send
        + 'static,
>;

pub type SendFn<T> = Box<dyn Fn(T) -> Result<()> + 'static + Send>;

pub struct Caller<T>
where
    T: Message,
{
    pub pid: u64,
    pub callback: Callback<T>,
}

pub struct Sender<T>
where
    T: Message,
{
    pub pid: u64,
    pub send_fn: SendFn<T>,
}

impl<T: Message> Caller<T> {
    pub async fn call(&self, msg: T) -> Result<T::Result> {
        (self.callback)(msg).await
    }
}

impl<T: Message> PartialEq for Caller<T> {
    fn eq(&self, other: &Self) -> bool {
        self.pid == other.pid
    }
}

impl<T: Message> Hash for Caller<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pid.hash(state)
    }
}

impl<T: Message> Sender<T> {
    pub fn send(&self, msg: T) -> Result<()> {
        (self.send_fn)(msg)
    }
}

impl<T: Message> PartialEq for Sender<T> {
    fn eq(&self, other: &Self) -> bool {
        self.pid == other.pid
    }
}

impl<T: Message> Hash for Sender<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pid.hash(state)
    }
}
