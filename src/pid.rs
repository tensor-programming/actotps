use futures::channel::{mpsc, oneshot};
use futures::future::Shared;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Weak};

use crate::actor::{Actor, Listener};
use crate::callback::{Caller, Sender};
use crate::message::{ActorEvent, Message};
use crate::{Error, Result};

pub struct Pid<A> {
    pub pid: u64,
    pub tx: Arc<mpsc::UnboundedSender<ActorEvent<A>>>,
    pub rx: Option<Shared<oneshot::Receiver<()>>>,
}

pub struct WeakPid<A> {
    pub pid: u64,
    pub tx: Weak<mpsc::UnboundedSender<ActorEvent<A>>>,
    pub rx: Option<Shared<oneshot::Receiver<()>>>,
}

impl<A: Actor> Pid<A> {
    pub fn pid(&self) -> u64 {
        self.pid
    }

    pub fn terminate(&mut self, err: Option<Error>) -> Result<()> {
        mpsc::UnboundedSender::clone(&*self.tx).start_send(ActorEvent::Stop(err))?;

        Ok(())
    }

    pub async fn call<T: Message>(&self, msg: T) -> Result<T::Result>
    where
        A: Listener<T>,
    {
        let (tx, rx) = oneshot::channel();

        mpsc::UnboundedSender::clone(&*self.tx).start_send(ActorEvent::Exec(Box::new(
            move |actor, ctx| {
                Box::pin(async move {
                    let res = Listener::handle_call(actor, ctx, msg).await;
                    let _ = tx.send(res);
                })
            },
        )))?;

        Ok(rx.await?)
    }

    pub fn cast<T: Message<Result = ()>>(&self, msg: T) -> Result<()>
    where
        A: Listener<T>,
    {
        mpsc::UnboundedSender::clone(&*self.tx).start_send(ActorEvent::Exec(Box::new(
            move |actor, ctx| {
                Box::pin(async move {
                    Listener::handle_call(actor, ctx, msg).await;
                })
            },
        )))?;
        Ok(())
    }

    pub fn create_sender<T: Message<Result = ()>>(&self) -> Sender<T>
    where
        A: Listener<T>,
    {
        let weak_tx = Arc::downgrade(&self.tx);
        Sender {
            pid: self.pid.clone(),
            send_fn: Box::new(move |msg| match weak_tx.upgrade() {
                Some(tx) => {
                    mpsc::UnboundedSender::clone(&tx).start_send(ActorEvent::Exec(Box::new(
                        move |actor, ctx| {
                            Box::pin(async move {
                                Listener::handle_call(&mut *actor, ctx, msg).await;
                            })
                        },
                    )))?;
                    Ok(())
                }
                None => Ok(()),
            }),
        }
    }

    pub fn create_caller<T: Message>(&self) -> Caller<T>
    where
        A: Listener<T>,
    {
        let weak_tx = Arc::downgrade(&self.tx);

        Caller {
            pid: self.pid.clone(),
            callback: Box::new(move |msg| {
                let weak_tx_option = weak_tx.upgrade();
                Box::pin(async move {
                    match weak_tx_option {
                        Some(tx) => {
                            let (oneshot_tx, oneshot_rx) = oneshot::channel();

                            mpsc::UnboundedSender::clone(&tx).start_send(ActorEvent::Exec(
                                Box::new(move |actor, ctx| {
                                    Box::pin(async move {
                                        let res =
                                            Listener::handle_call(&mut *actor, ctx, msg).await;
                                        let _ = oneshot_tx.send(res);
                                    })
                                }),
                            ))?;
                            Ok(oneshot_rx.await?)
                        }
                        None => Err(anyhow::anyhow!("Actor Dropped")),
                    }
                })
            }),
        }
    }

    pub async fn wait_for_terminate(self) {
        if let Some(exit) = self.rx {
            exit.await.ok();
        } else {
            futures::future::pending::<()>().await;
        }
    }
}

impl<A> Pid<A> {
    pub fn downgrade(&self) -> WeakPid<A> {
        WeakPid {
            pid: self.pid,
            tx: Arc::downgrade(&self.tx),
            rx: self.rx.clone(),
        }
    }
}

impl<A> Clone for Pid<A> {
    fn clone(&self) -> Self {
        Self {
            pid: self.pid,
            tx: self.tx.clone(),
            rx: self.rx.clone(),
        }
    }
}

impl<A> PartialEq for Pid<A> {
    fn eq(&self, other: &Self) -> bool {
        self.pid == other.pid
    }
}

impl<A> Hash for Pid<A> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pid.hash(state)
    }
}

impl<A> PartialEq for WeakPid<A> {
    fn eq(&self, other: &Self) -> bool {
        self.pid == other.pid
    }
}

impl<A> Hash for WeakPid<A> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pid.hash(state)
    }
}

impl<A> WeakPid<A> {
    pub fn upgrade(&self) -> Option<Pid<A>> {
        match self.tx.upgrade() {
            Some(tx) => Some(Pid {
                pid: self.pid,
                tx,
                rx: self.rx.clone(),
            }),
            None => None,
        }
    }
}

impl<A> Clone for WeakPid<A> {
    fn clone(&self) -> Self {
        Self {
            pid: self.pid,
            tx: self.tx.clone(),
            rx: self.rx.clone(),
        }
    }
}

impl<A: Actor> WeakPid<A> {
    pub fn pid(&self) -> u64 {
        self.pid
    }
}
