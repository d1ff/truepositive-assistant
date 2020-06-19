#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate error_chain;
#[macro_use]
extern crate telegram_bot;

use futures::StreamExt;
use structopt::StructOpt;

mod bot;
mod errors;
mod opts;

use bot::*;
use errors::*;
use opts::*;

#[tokio::main]
async fn main() -> Result<()> {
    let opt = BotOpt::from_args();

    let bot = Bot::new(opt).expect("Could not create bot");

    let mut stream = bot.stream();

    while let Some(update) = stream.next().await {
        let update = update?;
        bot.dispatch_update(update).await?;
    }

    Ok(())
}
