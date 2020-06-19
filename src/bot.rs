use lru::LruCache;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use telegram_bot::prelude::*;
use telegram_bot::types::*;
use telegram_bot::{Api, UpdatesStream};
use tera::{Context, Tera};
use uuid::Uuid;
use youtrack_rs::client::{Executor, YouTrack};

use super::errors::*;
use super::opts::*;

lazy_static! {
    pub static ref CACHES: Mutex<LruCache<Uuid, CallbackParams>> = Mutex::new(LruCache::new(100));
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
pub enum CallbackParams {
    BacklogNext(BacklogParams),
    BacklogPrev(BacklogParams),
    VoteForIssue(BacklogParams, VoteForIssueParams),
    BacklogStop,
    Invalid,
}

fn backlog_keyboard(issues: &Vec<Issue>, params: &BacklogParams) -> InlineKeyboardMarkup {
    let mut kb = InlineKeyboardMarkup::new();
    let mut row: Vec<InlineKeyboardButton> = Vec::new();

    let mut issues_row: Vec<InlineKeyboardButton> = Vec::new();
    for (_, issue) in issues.iter().enumerate() {
        issues_row.push(
            CallbackParams::VoteForIssue(
                params.clone(),
                VoteForIssueParams {
                    id: issue.id_readable.clone(),
                    has_vote: issue.voters.has_vote,
                },
            )
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

impl From<CallbackParams> for InlineKeyboardButton {
    fn from(item: CallbackParams) -> Self {
        let text: String = match &item {
            CallbackParams::BacklogStop => "stop".to_string(),
            CallbackParams::BacklogNext(_) => "next".to_string(),
            CallbackParams::BacklogPrev(_) => "prev".to_string(),
            CallbackParams::VoteForIssue(_, p) => p.id.clone(),
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

pub struct Bot {
    api: Api,
    yt: YouTrack,
    templates: Tera,
    backlog_query: String,
}

use url::form_urlencoded::byte_serialize;

impl Bot {
    pub fn new(opts: BotOpt) -> Result<Self> {
        let mut templates = match Tera::new("templates/**/*") {
            Ok(t) => t,

            Err(e) => {
                println!("Parsing error(s): {}", e);
                ::std::process::exit(1);
            }
        };

        templates.autoescape_on(vec!["html", ".sql"]);
        Ok(Self {
            api: opts.telegram_api(),
            yt: opts.youtrack_api()?,
            templates,
            backlog_query: byte_serialize(opts.youtrack_backlog.as_bytes()).collect(),
        })
    }

    pub fn stream(&self) -> UpdatesStream {
        self.api.stream()
    }

    pub async fn get_youtrack(&self, user: UserId) -> Option<YouTrack> {
        Some(self.yt.clone())
    }

    pub async fn list_backlog(&self, message: Message) -> Result<()> {
        self.fetch_issues(message, BacklogParams::new(10)).await?;
        Ok(())
    }

    pub async fn fetch_issues(&self, msg: Message, params: BacklogParams) -> Result<()> {
        let issues = self
            .get_youtrack(msg.from.id)
            .await
            .unwrap()
            .get()
            .issues()
            .query(self.backlog_query.as_str())
            .top(params.top.to_string().as_str())
            .skip(params.skip.to_string().as_str())
            .fields("idReadable,summary,votes,voters(hasVote)")
            .execute::<Vec<Issue>>();

        match issues {
            Ok((headers, status, json)) => {
                println!("{:#?}", headers);
                println!("{}", status);
                if let Some(issues) = json {
                    println!("{}", issues.len());
                    let kb = backlog_keyboard(&issues, &params);
                    let mut txt_msg: String = "No issues to display".to_string();
                    if issues.len() > 0 {
                        let mut context = Context::new();
                        context.insert("issues", &issues);
                        txt_msg = self.templates.render("issues_list.md", &context).unwrap();
                    }

                    // TODO: check whether original message is from our bot
                    if msg.from.is_bot {
                        self.api
                            .send(msg.edit_text(txt_msg).reply_markup(kb))
                            .await?;
                    } else {
                        self.api
                            .send(msg.text_reply(txt_msg).reply_markup(kb))
                            .await?;
                    };
                }
            }
            Err(e) => {
                self.api
                    .send(msg.text_reply(format!("Error occured: {}", e)))
                    .await?;
            }
        };

        Ok(())
    }

    pub async fn dispatch(&self, message: Message) -> Result<()> {
        if let MessageKind::Text { ref data, .. } = message.kind {
            println!(
                "<{}>: {} {} {}",
                &message.from.first_name,
                &message.from.id,
                &message.chat.id(),
                data
            );

            match data.as_str() {
                "/backlog" => self.list_backlog(message).await?,
                _ => {
                    println!("Unrecognized command");
                }
            };
        }

        Ok(())
    }

    pub async fn dispatch_callback(&self, cb: CallbackQuery) -> Result<()> {
        println!("Query: {:?}", cb);
        match extract_params(&cb) {
            None => {
                let msg = cb.message.unwrap();
                self.api
                    .send(msg.edit_reply_markup(Some(reply_markup!(inline_keyboard, []))))
                    .await?;
            }
            Some(data) => match data {
                CallbackParams::Invalid => {
                    let msg = cb.message.unwrap();
                    self.api
                        .send(msg.text_reply(format!("Error occured: invalid callback parameter")))
                        .await?;
                }
                CallbackParams::VoteForIssue(b, p) => {
                    let msg = cb.message.unwrap();
                    self.api
                        .send(msg.text_reply(format!("Vote for issue: {:?}", p)))
                        .await?;
                    self.fetch_issues(msg, b).await?;
                }
                CallbackParams::BacklogStop => {
                    let msg = cb.message.unwrap();
                    self.api
                        .send(msg.edit_reply_markup(Some(reply_markup!(inline_keyboard, []))))
                        .await?;
                }
                CallbackParams::BacklogNext(params) | CallbackParams::BacklogPrev(params) => {
                    let msg = cb.message.unwrap();
                    self.fetch_issues(msg, params).await?;
                }
            },
        }
        Ok(())
    }

    pub async fn dispatch_update(&self, update: Update) -> Result<()> {
        println!("{:?}", update);
        match update.kind {
            UpdateKind::Message(message) => self.dispatch(message).await?,
            UpdateKind::CallbackQuery(callback_query) => {
                self.dispatch_callback(callback_query).await?;
            }
            _ => println!("unsupported update kind"),
        };

        Ok(())
    }
}
