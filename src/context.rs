use futures::channel::{mpsc, oneshot};
use futures::future::{AbortHandle, Abortable, Shared};
use futures::{Stream, StreamExt};
use once_cell::sync::OnceCell;
use slab::Slab;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use async_std::task::{sleep, spawn};

use crate::actor::{Listener, StreamListener};
use crate::message::{ActorEvent, Message};
use crate::pid::Pid;
use crate::registry::{Connect, Disconnect, Registry, Service};
use crate::{Error, Result};

pub struct Context<A> {
    pid: u64,
    tx: Weak<mpsc::UnboundedSender<ActorEvent<A>>>,
    pub exit: Option<Shared<oneshot::Receiver<()>>>,
    pub listeners: Slab<AbortHandle>,
}

impl<A> Context<A> {
    pub fn new(
        exit: Option<Shared<oneshot::Receiver<()>>>,
    ) -> (
        Self,
        mpsc::UnboundedReceiver<ActorEvent<A>>,
        Arc<mpsc::UnboundedSender<ActorEvent<A>>>,
    ) {
        static PID: OnceCell<AtomicU64> = OnceCell::new();

        let pid = PID
            .get_or_init(Default::default)
            .fetch_add(1, Ordering::Relaxed);

        let (tx, rx) = mpsc::unbounded::<ActorEvent<A>>();
        let tx = Arc::new(tx);
        let weak_tx = Arc::downgrade(&tx);

        (
            Self {
                pid,
                tx: weak_tx,
                exit,
                listeners: Default::default(),
            },
            rx,
            tx,
        )
    }

    pub fn pid(&self) -> Pid<A> {
        Pid {
            pid: self.pid,
            tx: self.tx.upgrade().unwrap(),
            rx: self.exit.clone(),
        }
    }

    pub fn ctx_pid(&self) -> u64 {
        self.pid
    }

    pub fn terminate(&self, err: Option<Error>) {
        if let Some(tx) = self.tx.upgrade() {
            mpsc::UnboundedSender::clone(&*tx)
                .start_send(ActorEvent::Stop(err))
                .ok();
        }
    }

    pub fn add_listener<S>(&mut self, mut listener: S)
    where
        S: Stream + Unpin + Send + 'static,
        S::Item: 'static + Send,
        A: StreamListener<S::Item>,
    {
        let tx = self.tx.clone();
        let entry = self.listeners.vacant_entry();
        let id = entry.key();
        let (handle, registration) = futures::future::AbortHandle::new_pair();
        entry.insert(handle);

        let future = {
            async move {
                if let Some(tx) = tx.upgrade() {
                    mpsc::UnboundedSender::clone(&*tx)
                        .start_send(ActorEvent::Exec(Box::new(move |actor, ctx| {
                            Box::pin(async move {
                                StreamListener::connected(actor, ctx).await;
                            })
                        })))
                        .ok();
                } else {
                    return;
                }

                while let Some(msg) = listener.next().await {
                    if let Some(tx) = tx.upgrade() {
                        let res = mpsc::UnboundedSender::clone(&*tx).start_send(ActorEvent::Exec(
                            Box::new(move |actor, ctx| {
                                Box::pin(async move {
                                    StreamListener::handle_cast(actor, ctx, msg).await;
                                })
                            }),
                        ));
                        if res.is_err() {
                            return;
                        }
                    } else {
                        return;
                    }
                }

                if let Some(tx) = tx.upgrade() {
                    mpsc::UnboundedSender::clone(&*tx)
                        .start_send(ActorEvent::Exec(Box::new(move |actor, ctx| {
                            Box::pin(async move {
                                StreamListener::disconnected(actor, ctx).await;
                            })
                        })))
                        .ok();
                }
                if let Some(tx) = tx.upgrade() {
                    mpsc::UnboundedSender::clone(&*tx)
                        .start_send(ActorEvent::DropListener(id))
                        .ok();
                }
            }
        };

        spawn(Abortable::new(future, registration));
    }

    pub fn send_later<T>(&self, msg: T, after: Duration)
    where
        A: Listener<T>,
        T: Message<Result = ()>,
    {
        let sender = self.pid().create_sender();
        spawn(async move {
            sleep(after).await;
            sender.send(msg).ok();
        });
    }

    pub async fn connect<T: Message<Result = ()>>(&self) -> Result<()>
    where
        A: Listener<T>,
    {
        let broker = Registry::<T>::from_registry().await?;
        let sender = self.pid().create_sender();
        broker
            .cast(Connect {
                pid: self.pid,
                sender,
            })
            .ok();
        Ok(())
    }

    pub async fn disconnect<T: Message<Result = ()>>(&self) -> Result<()> {
        let broker = Registry::<T>::from_registry().await?;
        broker.cast(Disconnect { pid: self.pid })
    }
}
