use anyhow::Result;
use futures::channel::mpsc::UnboundedReceiver;
use futures::channel::oneshot;
use futures::{FutureExt, StreamExt};

use async_trait::async_trait;

use crate::context::Context;
use crate::message::{ActorEvent, Message};
use crate::pid::Pid;

#[async_trait]
pub trait Actor: Sized + Send + 'static {
    async fn init(&mut self, ctx: &mut Context<Self>) -> Result<()> {
        Ok(())
    }

    async fn terminated(&mut self, ctx: &mut Context<Self>) {}

    async fn spawn_default() -> Result<Pid<Self>>
    where
        Self: Default,
    {
        Ok(Self::default().spawn().await?)
    }

    async fn spawn(self) -> Result<Pid<Self>> {
        let (etx, erx) = oneshot::channel();
        let exit = erx.shared();
        let (ctx, rx, tx) = Context::new(Some(exit));

        let erx = ctx.exit.clone();
        let pid = ctx.ctx_pid();

        spawn_actor(ctx, rx, etx, self).await?;

        Ok(Pid { pid, tx, rx: erx })
    }
}

pub(crate) async fn spawn_actor<A: Actor>(
    mut ctx: Context<A>,
    mut rx: UnboundedReceiver<ActorEvent<A>>,
    tx: oneshot::Sender<()>,
    mut actor: A,
) -> Result<()> {
    actor.init(&mut ctx).await?;

    async_std::task::spawn({
        async move {
            while let Some(evnt) = rx.next().await {
                match evnt {
                    ActorEvent::Exec(f) => f(&mut actor, &mut ctx).await,
                    ActorEvent::Stop(_e) => break,
                    ActorEvent::DropListener(pid) => {
                        if ctx.listeners.contains(pid) {
                            ctx.listeners.remove(pid);
                        }
                    }
                }
            }

            actor.terminated(&mut ctx).await;

            for (_, handle) in ctx.listeners.iter() {
                handle.abort();
            }

            tx.send(()).ok();
        }
    });

    Ok(())
}

#[async_trait]
pub trait Listener<T: Message>: Actor
where
    Self: std::marker::Sized,
{
    async fn handle_call(&mut self, ctx: &mut Context<Self>, msg: T) -> T::Result;
}

#[async_trait]
pub trait StreamListener<T: 'static>: Actor {
    async fn handle_cast(&mut self, ctx: &mut Context<Self>, msg: T);

    async fn connected(&mut self, ctx: &mut Context<Self>) {}

    async fn disconnected(&mut self, ctx: &mut Context<Self>) {
        ctx.terminate(None);
    }
}
