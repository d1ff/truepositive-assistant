#[macro_use]
extern crate lazy_static;
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

use futures::StreamExt;
use std::sync::{Arc, Mutex};
use structopt::StructOpt;
use tokio::prelude::*;

mod bot;
mod errors;
mod models;
mod opts;
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
    let srv = yt_oauth::run(bot.clone()).unwrap();

    while let Some(update) = stream.next().await {
        let update = update?;
        {
            let mut bot = bot.lock().unwrap();
            bot.dispatch_update(update).await?;
        }
    }

    srv.await?;
    system.await?;

    Ok(())
}
