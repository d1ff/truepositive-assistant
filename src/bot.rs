use lru::LruCache;
use oauth2::basic::BasicClient;
use oauth2::{CsrfToken, Scope};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use telegram_bot::prelude::*;
use telegram_bot::types::*;
use telegram_bot::{Api, UpdatesStream};
use tera::{Context, Tera};
use ttl_cache::TtlCache;
use uuid::Uuid;
use youtrack_rs::client::{Executor, YouTrack};

use super::errors::*;
use super::models::*;
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

fn backlog_keyboard(issues: &Issues, params: &BacklogParams) -> InlineKeyboardMarkup {
    let mut kb = InlineKeyboardMarkup::new();
    let mut row: Vec<InlineKeyboardButton> = Vec::new();

    let mut issues_buttons: Vec<InlineKeyboardButton> = Vec::new();
    for issue in issues.iter() {
        issues_buttons.push(
            CallbackParams::VoteForIssue(
                params.clone(),
                VoteForIssueParams {
                    id: issue.id_readable.clone(),
                    has_vote: issue.voters.has_vote,
                },
            )
            .into(),
        );
    }
    for row in issues_buttons.chunks(3) {
        kb.add_row(row.to_vec());
    }

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
            CallbackParams::VoteForIssue(_, p) => {
                if p.has_vote {
                    format!("{} {}", emoji!("star2"), p.id)
                } else {
                    p.id.clone()
                }
            }
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

pub struct Bot {
    api: Api,
    youtrack_url: String,
    pub templates: Tera,
    pub yt_oauth: BasicClient,
    backlog_query: String,
    csrf_tokens: HashMap<String, UserId>,
    yt_tokens: TtlCache<UserId, String>,
}

unsafe impl Send for Bot {}

use url::form_urlencoded::byte_serialize;

fn markdown_escape(value: &Value, _: &HashMap<String, Value>) -> tera::Result<Value> {
    let mut s = try_get_value!("escape_html", "value", String, value);
    let escaped_chars = vec!['_', '*', '`', '['];
    for c in escaped_chars {
        s = s.replace(c, format!("\\{}", c).as_str())
    }
    Ok(Value::String(s))
}

impl Bot {
    pub fn new(opts: BotOpt) -> Result<Self> {
        let mut templates = match Tera::new("templates/**/*") {
            Ok(t) => t,

            Err(e) => {
                error!("Parsing error(s): {}", e);
                ::std::process::exit(1);
            }
        };

        templates.autoescape_on(vec!["html", ".sql"]);
        templates.register_filter("markdown_escape", markdown_escape);
        Ok(Self {
            api: opts.telegram_api(),
            youtrack_url: opts.youtrack_url.clone(),
            templates,
            backlog_query: byte_serialize(opts.youtrack_backlog.as_bytes()).collect(),
            yt_oauth: opts.oauth_client(),
            csrf_tokens: HashMap::new(),
            yt_tokens: TtlCache::new(100),
        })
    }

    pub fn stream(&self) -> UpdatesStream {
        self.api.stream()
    }

    pub async fn get_youtrack(&self, user: UserId) -> Option<YouTrack> {
        self.yt_tokens.get(&user).and_then(|token| {
            Some(YouTrack::new(self.youtrack_url.clone(), token.to_string()).unwrap())
        })
    }

    pub async fn list_backlog(&self, message: Message) -> Result<()> {
        self.fetch_issues(message.from.id, message, BacklogParams::new(5))
            .await?;
        Ok(())
    }

    async fn _fetch_issues(&self, yt: YouTrack, top: i32, skip: i32) -> Result<Issues> {
        let issues = yt
            .get()
            .issues()
            .query(self.backlog_query.as_str())
            .top(top.to_string().as_str())
            .skip(skip.to_string().as_str())
            .fields("idReadable,summary,votes,voters(hasVote)")
            .execute::<Issues>()?;

        let (headers, status, issues) = issues;

        debug!("{:#?}", headers);
        debug!("{}", status);

        if !status.is_success() {
            bail!("Unable to fetch issues from youtrack")
        };
        if let Some(issues) = issues {
            Ok(issues)
        } else {
            bail!("Unable to parse issues list")
        }
    }

    pub async fn fetch_issues(
        &self,
        user: UserId,
        msg: Message,
        params: BacklogParams,
    ) -> Result<()> {
        match self.get_youtrack(user).await {
            Some(yt) => {
                match self._fetch_issues(yt, params.top, params.skip).await {
                    Ok(issues) => {
                        debug!("{}", issues.len());
                        let kb = backlog_keyboard(&issues, &params);
                        let mut txt_msg: String = "No issues to display".to_string();
                        if issues.len() > 0 {
                            let mut context = Context::new();
                            context.insert("issues", &issues);
                            context.insert("skip", &params.skip);
                            context.insert("youtrack_url", &self.youtrack_url);
                            txt_msg = self.templates.render("issues_list.md", &context).unwrap();
                        }

                        // TODO: check whether original message is from our bot
                        if msg.from.is_bot {
                            self.api
                                .send(
                                    msg.edit_text(txt_msg)
                                        .reply_markup(kb)
                                        .parse_mode(ParseMode::Markdown),
                                )
                                .await?;
                        } else {
                            self.api
                                .send(
                                    msg.text_reply(txt_msg)
                                        .reply_markup(kb)
                                        .parse_mode(ParseMode::Markdown),
                                )
                                .await?;
                        };
                    }
                    Err(e) => {
                        warn!("Error occured: {}", e);
                        self.api
                            .send(msg.text_reply(format!("Error occured: {}", e)))
                            .await?;
                    }
                };
            }
            None => {
                warn!("No token found for user: {}", user);
                self.api
                    .send(msg.text_reply(format!(
                        "No valid access token founds, use /start command to login in youtrack"
                    )))
                    .await?;
            }
        }
        Ok(())
    }

    async fn handle_start(&mut self, msg: Message) -> Result<()> {
        // Generate youtrack url
        let (auth_url, csrf_token) = self
            .yt_oauth
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new("YouTrack".to_string()))
            .use_implicit_flow()
            .url();
        self.csrf_tokens
            .insert(csrf_token.secret().clone(), msg.from.id);
        let kb = reply_markup!(inline_keyboard,
            ["Login" url auth_url]);
        self.api
            .send(
                msg.text_reply(format!("Hello, {}", msg.from.first_name))
                    .reply_markup(kb),
            )
            .await?;

        Ok(())
    }

    pub fn on_auth(&mut self, params: super::yt_oauth::AuthRequest) {
        match self.csrf_tokens.get(&params.state) {
            Some(user_id) => {
                info!("Saving token for: {}", user_id);
                self.yt_tokens.insert(
                    user_id.clone(),
                    params.access_token.clone(),
                    params.expires_in_duration(),
                );
            }
            None => {
                warn!("No csrf token!");
            }
        };
    }

    pub async fn dispatch(&mut self, message: Message) -> Result<()> {
        if let MessageKind::Text { ref data, .. } = message.kind {
            debug!(
                "<{}>: {} {} {}",
                &message.from.first_name,
                &message.from.id,
                &message.chat.id(),
                data
            );

            match data.as_str() {
                "/backlog" => self.list_backlog(message).await?,
                "/start" => self.handle_start(message).await?,
                _ => {
                    warn!("Unrecognized command: {:?}", message);
                }
            };
        }

        Ok(())
    }

    async fn vote_for_issue(&self, yt: YouTrack, has_vote: bool, id: String) -> Result<bool> {
        let json_has_vote = json!({"hasVote": !has_vote});
        let i = yt.post(json_has_vote).issues();
        let i = i.id(id.as_str());
        let i = i.voters().execute::<Value>()?;

        let (headers, status, json) = i;
        debug!("{:#?}", headers);
        debug!("{}", status);
        debug!("{:?}", json);
        if !status.is_success() {
            if let Ok(err) = serde_json::from_value::<YoutrackError>(json.unwrap()) {
                // TODO: wrap into YoutrackError kind
                bail!(err.error_description);
            } else {
                bail!("Unable to vote for issue");
            }
        };
        Ok(!has_vote)
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
                    let user = cb.from.id;
                    match self.get_youtrack(user).await {
                        Some(yt) => match self.vote_for_issue(yt, p.has_vote, p.id).await {
                            Ok(_) => {
                                self.fetch_issues(user, msg, b).await?;
                            }
                            Err(e) => {
                                warn!("Error occured: {}", e);
                                self.api
                                    .send(msg.text_reply(format!("Error occured: {}", e)))
                                    .await?;
                            }
                        },
                        None => {
                            warn!("No youtrack instance for user {}", user);
                            self.api.send(msg.text_reply(format!("No valid access token founds, use /start command to login in youtrack"))).await?;
                        }
                    }
                }
                CallbackParams::BacklogStop => {
                    let msg = cb.message.unwrap();
                    self.api
                        .send(msg.edit_reply_markup(Some(reply_markup!(inline_keyboard, []))))
                        .await?;
                }
                CallbackParams::BacklogNext(params) | CallbackParams::BacklogPrev(params) => {
                    let msg = cb.message.unwrap();
                    self.fetch_issues(cb.from.id, msg, params).await?;
                }
            },
        }
        Ok(())
    }

    pub async fn dispatch_update(&mut self, update: Update) -> Result<()> {
        debug!("Got update: {:?}", update);
        match update.kind {
            UpdateKind::Message(message) => self.dispatch(message).await?,
            UpdateKind::CallbackQuery(callback_query) => {
                self.dispatch_callback(callback_query).await?;
            }
            _ => warn!("Unsupported update kind"),
        };

        Ok(())
    }
}
