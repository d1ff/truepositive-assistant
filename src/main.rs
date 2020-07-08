#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate telegram_bot;
#[macro_use]
extern crate emojicons;
#[macro_use]
extern crate tera;
#[macro_use]
extern crate log;
#[macro_use]
extern crate machine;
#[macro_use]
extern crate paste;

use futures::StreamExt;
use std::sync::{Arc, Mutex};
use structopt::StructOpt;

mod bot;
mod commands;
mod errors;
mod models;
mod opts;
mod states;
mod yt_oauth;

use bot::*;
use errors::*;
use opts::*;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let opt = BotOpt::from_args();

    let bot = Arc::new(Mutex::new(Box::new(
        Bot::new(opt.clone()).expect("Could not create bot"),
    )));

    let mut stream = bot.lock().unwrap().stream();

    let rt = tokio::task::LocalSet::new();
    let system = actix_rt::System::run_in_tokio("test", &rt);
    let srv = yt_oauth::run(bot.clone(), opt.addr).unwrap();

    while let Some(update) = stream.next().await {
        let update = update?;
        {
            let mut bot = bot.lock().unwrap();
            let res = bot.dispatch_update(update).await;
            if let Err(e) = res {
                warn!("Error occured: {}", e);
            }
        }
    }

    srv.await?;
    system.await?;

    Ok(())
}
