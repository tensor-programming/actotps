use fnv::FnvHasher;
use futures::channel::oneshot;
use futures::lock::Mutex;
use futures::FutureExt;
use once_cell::sync::OnceCell;
use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::marker::PhantomData;

use crate::actor::{spawn_actor, Actor, Listener};
use crate::callback::Sender;
use crate::context::Context;
use crate::message::Message;
use crate::pid::Pid;
use crate::Result;
use async_trait::async_trait;

thread_local! {
    static LOCAL_REGISTRY: RefCell<HashMap<TypeId, Box<dyn Any + Send>, BuildHasherDefault<FnvHasher>>> = Default::default();
}

#[async_trait]
pub trait Service: Actor + Default {
    async fn from_registry() -> Result<Pid<Self>> {
        static REGISTRY: OnceCell<
            Mutex<HashMap<TypeId, Box<dyn Any + Send>, BuildHasherDefault<FnvHasher>>>,
        > = OnceCell::new();

        let registry = REGISTRY.get_or_init(Default::default);
        let mut registry = registry.lock().await;

        match registry.get_mut(&TypeId::of::<Self>()) {
            Some(addr) => Ok(addr.downcast_ref::<Pid<Self>>().unwrap().clone()),
            None => {
                let (etx, erx) = oneshot::channel();
                let exit = erx.shared();
                let (ctx, rx, tx) = Context::new(Some(exit));
                registry.insert(TypeId::of::<Self>(), Box::new(ctx.pid()));
                drop(registry);
                let pid = ctx.ctx_pid();
                let exit = ctx.exit.clone();

                spawn_actor(ctx, rx, etx, Self::default()).await?;

                Ok(Pid { pid, tx, rx: exit })
            }
        }
    }
}

#[async_trait::async_trait]
pub trait LocalService: Actor + Default {
    async fn from_registry() -> Result<Pid<Self>> {
        let res = LOCAL_REGISTRY.with(|registry| {
            registry
                .borrow_mut()
                .get_mut(&TypeId::of::<Self>())
                .map(|addr| addr.downcast_ref::<Pid<Self>>().unwrap().clone())
        });
        match res {
            Some(addr) => Ok(addr),
            None => {
                let addr = {
                    let (tx_exit, rx_exit) = oneshot::channel();
                    let exit = rx_exit.shared();
                    let (ctx, rx, tx) = Context::new(Some(exit));
                    let pid = ctx.ctx_pid();
                    let rx_exit = ctx.exit.clone();
                    spawn_actor(ctx, rx, tx_exit, Self::default()).await?;
                    Pid {
                        pid,
                        tx,
                        rx: rx_exit,
                    }
                };
                LOCAL_REGISTRY.with(|registry| {
                    registry
                        .borrow_mut()
                        .insert(TypeId::of::<Self>(), Box::new(addr.clone()));
                });
                Ok(addr)
            }
        }
    }
}

pub struct Connect<T: Message<Result = ()>> {
    pub pid: u64,
    pub sender: Sender<T>,
}

impl<T: Message<Result = ()>> Message for Connect<T> {
    type Result = ();
}

pub(crate) struct Disconnect {
    pub(crate) pid: u64,
}

impl Message for Disconnect {
    type Result = ();
}

struct Publish<T: Message<Result = ()> + Clone>(T);

impl<T: Message<Result = ()> + Clone> Message for Publish<T> {
    type Result = ();
}

pub struct Registry<T: Message<Result = ()>> {
    listeners: HashMap<u64, Box<dyn Any + Send>, BuildHasherDefault<FnvHasher>>,
    mark: PhantomData<T>,
}

impl<T: Message<Result = ()>> Default for Registry<T> {
    fn default() -> Self {
        Self {
            listeners: Default::default(),
            mark: PhantomData,
        }
    }
}

impl<T: Message<Result = ()>> Actor for Registry<T> {}

impl<T: Message<Result = ()>> Service for Registry<T> {}

#[async_trait::async_trait]
impl<T: Message<Result = ()>> Listener<Connect<T>> for Registry<T> {
    async fn handle_call(&mut self, _ctx: &mut Context<Self>, msg: Connect<T>) {
        self.listeners.insert(msg.pid, Box::new(msg.sender));
    }
}

#[async_trait::async_trait]
impl<T: Message<Result = ()>> Listener<Disconnect> for Registry<T> {
    async fn handle_call(&mut self, _ctx: &mut Context<Self>, msg: Disconnect) {
        self.listeners.remove(&msg.pid);
    }
}

#[async_trait::async_trait]
impl<T: Message<Result = ()> + Clone> Listener<Publish<T>> for Registry<T> {
    async fn handle_call(&mut self, _ctx: &mut Context<Self>, msg: Publish<T>) {
        for sender in self.listeners.values_mut() {
            if let Some(sender) = sender.downcast_mut::<Sender<T>>() {
                sender.send(msg.0.clone()).ok();
            }
        }
    }
}

impl<T: Message<Result = ()> + Clone> Pid<Registry<T>> {
    pub fn publish(&mut self, msg: T) -> Result<()> {
        self.cast(Publish(msg))
    }
}
