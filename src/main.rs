#[macro_use]
extern crate lazy_static;

use futures::StreamExt;
use hyper::Client;
use hyper_socks2::{Auth, SocksConnector};
use hyper_tls::HttpsConnector;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use telegram_bot::connector::hyper::HyperConnector;
use tera::{Context, Tera};

use lru::LruCache;
use telegram_bot::*;
use url::Url;
use youtrack_rs::client::{Executor, YouTrack};

use std::sync::Mutex;
use uuid::Uuid;

lazy_static! {
    pub static ref TEMPLATES: Tera = {
        let mut tera = match Tera::new("templates/**/*") {
            Ok(t) => t,

            Err(e) => {
                println!("Parsing error(s): {}", e);

                ::std::process::exit(1);
            }
        };

        tera.autoescape_on(vec!["html", ".sql"]);
        tera
    };
}

lazy_static! {
    pub static ref CACHES: Mutex<LruCache<Uuid, CallbackParams>> = Mutex::new(LruCache::new(100));
}

pub static BACKLOG_QUERY: &str = "project%3A%20airsearcher%20%20project%3A%20DCMon%20project%3A%20DCone%20project%3A%20KI%20project%3A%20RR%20project%3A%20RRMON%20project%3A%20TPLIT%20%23unresolved%20%20has%3A%20-%7BBoard%20All%20tasks%7D%20order%20by%3A%20Stream%20asc%2C%20Priority%20asc";

#[derive(StructOpt, Debug)]
#[structopt(name = "truepositive-assistant")]
struct BotOpt {
    #[structopt(long, env = "TELEGRAM_BOT_TOKEN")]
    telegram_token: String,

    #[structopt(long, env = "SOCKS5_PROXY")]
    socks5_proxy: Option<String>,

    #[structopt(long, env = "YOUTRACK_URL")]
    youtrack_url: String,

    #[structopt(long, env = "YOUTRACK_TOKEN")]
    youtrack_token: String,
}

impl BotOpt {
    fn telegram_api(&self) -> Api {
        match &self.socks5_proxy {
            Some(socks5_proxy) => {
                let auth = {
                    let url = Url::parse(&socks5_proxy).expect("Invalid proxy url");
                    let username = url.username();
                    if let Some(password) = url.password() {
                        Some(Auth::new(username, password))
                    } else {
                        None
                    }
                };

                let connector = HttpsConnector::new();
                let proxy = SocksConnector {
                    proxy_addr: socks5_proxy.parse().expect("Could not parse proxy url"),
                    auth: auth,
                    connector,
                };
                let proxy = proxy.with_tls().unwrap();
                let connector = Box::new(HyperConnector::new(Client::builder().build(proxy)));
                Api::with_connector(self.telegram_token.clone(), connector)
            }
            None => Api::new(self.telegram_token.clone()),
        }
    }

    fn youtrack_api(&self) -> youtrack_rs::errors::Result<YouTrack> {
        YouTrack::new(self.youtrack_url.clone(), self.youtrack_token.clone())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BacklogParams {
    top: i32,
    skip: i32,
}

impl BacklogParams {
    fn new(top: i32) -> Self {
        Self { top, skip: 0 }
    }

    fn next(&self) -> Self {
        Self {
            top: self.top,
            skip: self.skip + self.top,
        }
    }

    fn prev(&self) -> Option<Self> {
        if self.skip - self.top >= 0 {
            Some(Self {
                top: self.top,
                skip: self.skip - self.top,
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteForIssueParams {
    id: String,
    has_vote: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "_t")]
pub enum CallbackParams {
    BacklogNext(BacklogParams),
    BacklogPrev(BacklogParams),
    VoteForIssue(VoteForIssueParams),
    BacklogStop,
    Invalid,
}

fn backlog_keyboard(issues: &Vec<Issue>, params: &BacklogParams) -> InlineKeyboardMarkup {
    let mut kb = InlineKeyboardMarkup::new();
    let mut row: Vec<InlineKeyboardButton> = Vec::new();

    let mut issues_row: Vec<InlineKeyboardButton> = Vec::new();
    for (_, issue) in issues.iter().enumerate() {
        issues_row.push(
            CallbackParams::VoteForIssue(VoteForIssueParams {
                id: issue.id_readable.clone(),
                has_vote: issue.voters.has_vote,
            })
            .into(),
        );
        if issues_row.len() == 5 {
            kb.add_row(issues_row);
            issues_row = Vec::new();
        }
    }
    kb.add_row(issues_row);

    row.push(CallbackParams::BacklogStop {}.into());

    if let Some(prev) = params.prev() {
        row.push(CallbackParams::BacklogPrev(prev).into());
    }
    if issues.len() > 0 {
        row.push(CallbackParams::BacklogNext(params.next()).into());
    } else {
        row.pop();
        if let Some(prev) = params.prev() {
            if let Some(prev) = prev.prev() {
                row.push(CallbackParams::BacklogPrev(prev).into());
            }
        }
    }
    kb.add_row(row);
    kb
}

async fn list_backlog(api: Api, yt: YouTrack, message: Message) -> Result<(), Error> {
    let issues = yt
        .get()
        .issues()
        .query(BACKLOG_QUERY)
        .top("10")
        .skip("0")
        .fields("idReadable,summary,votes,voters(hasVote)")
        .execute::<Vec<Issue>>();

    match issues {
        Ok((headers, status, json)) => {
            println!("{:#?}", headers);
            println!("{}", status);
            if let Some(issues) = json {
                let mut context = Context::new();
                context.insert("issues", &issues);
                let txt_msg = TEMPLATES.render("issues_list.md", &context).unwrap();
                let params = BacklogParams::new(10);
                api.send(
                    message
                        .text_reply(txt_msg)
                        .reply_markup(backlog_keyboard(&issues, &params)),
                )
                .await?;
            }
        }
        Err(e) => {
            api.send(message.text_reply(format!("Error occured: {}", e)))
                .await?;
        }
    };
    Ok(())
}

async fn dispatch(api: Api, yt: YouTrack, message: Message) -> Result<(), Error> {
    if let MessageKind::Text { ref data, .. } = message.kind {
        println!(
            "<{}>: {} {} {}",
            &message.from.first_name,
            &message.from.id,
            &message.chat.id(),
            data
        );

        match data.as_str() {
            "/backlog" => list_backlog(api, yt, message).await?,
            _ => {
                println!("Unrecognized command");
            }
        };
    }

    Ok(())
}

impl From<CallbackParams> for InlineKeyboardButton {
    fn from(item: CallbackParams) -> Self {
        let text: String = match &item {
            CallbackParams::BacklogStop => "stop".to_string(),
            CallbackParams::BacklogNext(_) => "next".to_string(),
            CallbackParams::BacklogPrev(_) => "prev".to_string(),
            CallbackParams::VoteForIssue(p) => p.id.clone(),
            CallbackParams::Invalid => panic!("Do not use in keyboard"),
        };
        let uuid = Uuid::new_v4();
        let mut map = CACHES.lock().unwrap();
        map.put(uuid, item.clone());
        InlineKeyboardButton::callback(text, uuid.to_string())
    }
}

fn extract_params(cb: &CallbackQuery) -> Option<CallbackParams> {
    let ref data = cb.data.clone()?;
    match Uuid::parse_str(data) {
        Ok(uuid) => {
            let mut cache = CACHES.lock().unwrap();
            let d = cache.pop(&uuid);
            d
        }
        Err(_) => Some(CallbackParams::Invalid),
    }
}

#[derive(Serialize, Deserialize)]
struct IssueVoters {
    #[serde(alias = "hasVote")]
    has_vote: bool,
}

#[derive(Serialize, Deserialize)]
struct Issue {
    #[serde(alias = "idReadable")]
    id_readable: String,
    summary: String,
    votes: i32,
    voters: IssueVoters,
}

async fn dispatch_callback(api: Api, yt: YouTrack, cb: CallbackQuery) -> Result<(), Error> {
    println!("Query: {:?}", cb);
    match extract_params(&cb) {
        None => {
            api.send(
                cb.message
                    .unwrap()
                    .edit_reply_markup(Some(reply_markup!(inline_keyboard, []))),
            )
            .await?;
        }
        Some(data) => match data {
            CallbackParams::Invalid => {
                api.send(
                    cb.message
                        .unwrap()
                        .text_reply(format!("Error occured: invalid callback parameter")),
                )
                .await?;
            }
            CallbackParams::VoteForIssue(p) => {
                api.send(
                    cb.message
                        .unwrap()
                        .text_reply(format!("Vote for issue: {:?}", p)),
                )
                .await?;
            }
            CallbackParams::BacklogStop => {
                api.send(
                    cb.message
                        .unwrap()
                        .edit_reply_markup(Some(reply_markup!(inline_keyboard, []))),
                )
                .await?;
            }
            CallbackParams::BacklogNext(params) | CallbackParams::BacklogPrev(params) => {
                let issues = yt
                    .get()
                    .issues()
                    .query(BACKLOG_QUERY)
                    .top(params.top.to_string().as_str())
                    .skip(params.skip.to_string().as_str())
                    .fields("idReadable,summary,votes,voters(hasVote)")
                    .execute::<Vec<Issue>>();

                match issues {
                    Ok((headers, status, json)) => {
                        println!("{:#?}", headers);
                        println!("{}", status);
                        if let Some(issues) = json {
                            let msg = cb.message.unwrap();
                            println!("{}", issues.len());
                            let kb = backlog_keyboard(&issues, &params);
                            if issues.len() > 0 {
                                let mut context = Context::new();
                                context.insert("issues", &issues);
                                let txt_msg = TEMPLATES.render("issues_list.md", &context).unwrap();
                                api.send(msg.edit_text(txt_msg).reply_markup(kb)).await?;
                            } else {
                                api.send(msg.edit_text("No issues to display").reply_markup(kb))
                                    .await?;
                            }
                        }
                    }
                    Err(e) => {
                        api.send(
                            cb.message
                                .unwrap()
                                .text_reply(format!("Error occured: {}", e)),
                        )
                        .await?;
                    }
                };
            }
        },
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let opt = BotOpt::from_args();

    let api = opt.telegram_api();
    let youtrack = opt
        .youtrack_api()
        .expect("Could not create YouTrack instance");

    let mut stream = api.stream();

    while let Some(update) = stream.next().await {
        let update = update?;
        println!("{:?}", update);
        match update.kind {
            UpdateKind::Message(message) => {
                dispatch(api.clone(), youtrack.clone(), message).await?
            }
            UpdateKind::CallbackQuery(callback_query) => {
                dispatch_callback(api.clone(), youtrack.clone(), callback_query).await?;
            }
            _ => println!("unsupported update kind"),
        };
    }

    Ok(())
}
