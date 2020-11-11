use async_trait::async_trait;
use std::time::Duration;

use actopts::*;

struct Terminate;

struct MyActor;

impl Message for Terminate {
    type Result = ();
}

#[async_trait]
impl Actor for MyActor {
    async fn init(&mut self, ctx: &mut Context<Self>) -> Result<()> {
        ctx.send_later(Terminate, Duration::from_secs(2));

        println!("Call from MyActor: {:?}", ctx.ctx_pid());

        Ok(())
    }
}

#[async_trait]
impl Listener<Terminate> for MyActor {
    async fn handle_call(&mut self, ctx: &mut Context<Self>, _: Terminate) {
        ctx.terminate(None);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    for i in 0..10 {
        let pid = MyActor.spawn().await?;

        if i == 9 {
            pid.wait_for_terminate().await;
        }
    }

    Ok(())
}
