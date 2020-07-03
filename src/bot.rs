use oauth2::basic::BasicClient;
use oauth2::{CsrfToken, Scope};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::TryInto;
use telegram_bot::prelude::*;
use telegram_bot::types::*;
use telegram_bot::{Api, UpdatesStream};
use tera::{Context, Tera};
use ttl_cache::TtlCache;
use youtrack_rs::client::{Executor, YouTrack};

use super::commands::*;
use super::errors::*;
use super::models::*;
use super::opts::*;
use super::states::*;

fn backlog_keyboard(issues: &Issues, params: &BacklogParams) -> InlineKeyboardMarkup {
    let mut kb = InlineKeyboardMarkup::new();
    let mut row: Vec<InlineKeyboardButton> = Vec::new();

    let mut issues_buttons: Vec<InlineKeyboardButton> = Vec::new();
    for issue in issues.iter() {
        issues_buttons.push(
            CallbackParams::VoteForIssue(VoteForIssueParams {
                id: issue.id_readable.clone(),
                has_vote: issue.voters.has_vote,
            })
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

pub struct Bot {
    api: Api,
    yt: YouTrack,
    pub templates: Tera,
    pub yt_oauth: BasicClient,
    backlog_query: String,
    csrf_tokens: HashMap<String, UserId>,
    yt_tokens: TtlCache<UserId, YouTrack>,
    states: HashMap<UserId, UserState>,
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
            yt: opts.youtrack_api()?,
            templates,
            backlog_query: byte_serialize(opts.youtrack_backlog.as_bytes()).collect(),
            yt_oauth: opts.oauth_client(),
            csrf_tokens: HashMap::new(),
            yt_tokens: TtlCache::new(100),
            states: HashMap::new(),
        })
    }

    pub fn stream(&self) -> UpdatesStream {
        self.api.stream()
    }

    pub async fn get_youtrack(&self, user: UserId) -> Option<&YouTrack> {
        self.yt_tokens.get(&user)
    }

    pub async fn list_backlog(&self, message: &Message) -> Result<()> {
        self.fetch_issues(message.from.id, message, BacklogParams::new(5))
            .await?;
        Ok(())
    }

    async fn _fetch_issues(&self, yt: &YouTrack, top: i32, skip: i32) -> Result<Issues> {
        let issues = yt
            .get()
            .issues()
            .query(self.backlog_query.as_str())
            .top(top.to_string().as_str())
            .skip(skip.to_string().as_str())
            .fields("idReadable,summary,votes,voters(hasVote)")
            .execute::<Issues>()
            .await?;

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
        msg: &Message,
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
                            context.insert("youtrack_url", &self.yt.get_uri());
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
                        "No valid access token founds, use /login command to login in youtrack"
                    )))
                    .await?;
            }
        }
        Ok(())
    }

    async fn handle_start(&self, msg: &Message) -> Result<()> {
        let mut context = Context::new();
        context.insert("name", &msg.from.first_name);
        let txt_msg = self.templates.render("start.md", &context).unwrap();
        self.api
            .send(msg.text_reply(txt_msg).parse_mode(ParseMode::Markdown))
            .await?;

        Ok(())
    }

    async fn handle_login(&mut self, msg: &Message) -> Result<()> {
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
            ["Log into YouTrack" url auth_url]);
        self.api
            .send(
                msg.text_reply("Use this button to launch login process in the browser")
                    .reply_markup(kb),
            )
            .await?;

        Ok(())
    }

    pub async fn on_auth(&mut self, params: super::yt_oauth::AuthRequest) {
        match self.csrf_tokens.get(&params.state) {
            Some(user_id) => {
                info!("Saving token for: {}", user_id);
                let mut yt = self.yt.clone();
                yt.set_token(params.access_token.clone());

                let me = yt.get().users().me().fields("fullName").execute::<Value>();

                match me.await {
                    Ok((_, _, v)) => {
                        let me = v.unwrap();

                        self.yt_tokens
                            .insert(user_id.clone(), yt, params.expires_in_duration());
                        self.api
                            .spawn(user_id.text(format!("Hello, {}!", me["fullName"])));
                    }
                    Err(e) => warn!("YouTrack API request failed: {}", e),
                }
            }
            None => {
                warn!("No csrf token!");
            }
        };
    }

    async fn vote_for_issue(&self, yt: &YouTrack, has_vote: bool, id: String) -> Result<bool> {
        let json_has_vote = json!({"hasVote": !has_vote});
        let i = yt.post(json_has_vote).issues();
        let i = i.id(id.as_str());
        let i = i.voters().execute::<Value>().await?;

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

    fn get_state(&mut self, uid: &UserId) -> UserState {
        let idle = UserState::idle();
        self.states.get(uid).unwrap_or(&idle).clone()
    }

    fn get_state_by_update(&mut self, update: &Update) -> Result<(UserId, UserState)> {
        let uid = match &update.kind {
            UpdateKind::Message(m) => m.from.id,
            UpdateKind::CallbackQuery(cb) => cb.from.id,
            _ => bail!("Unsupported update type"),
        };
        Ok((uid, self.get_state(&uid)))
    }

    async fn handle_command(&mut self, state: UserState, cmd: BotCommand) -> Result<UserState> {
        match &state {
            UserState::Idle(..) => match &cmd {
                BotCommand::Backlog(msg) => self.list_backlog(msg).await?,
                BotCommand::Start(msg) => self.handle_start(msg).await?,
                BotCommand::Login(msg) => self.handle_login(msg).await?,
                _ => {}
            },
            UserState::InBacklog(state) => match &cmd {
                BotCommand::BacklogStop(cb) => {
                    let msg = cb.message.clone().unwrap();
                    self.api
                        .send(msg.edit_reply_markup(Some(reply_markup!(inline_keyboard, []))))
                        .await?;
                }
                BotCommand::BacklogNext(cb, p) | BotCommand::BacklogPrev(cb, p) => {
                    let msg = cb.message.clone().unwrap();
                    self.fetch_issues(cb.from.id, &msg, p.clone()).await?;
                }
                BotCommand::BacklogVoteForIssue(cb, p) => {
                    let msg = cb.message.clone().unwrap();
                    let user = cb.from.id;
                    match self.get_youtrack(user).await {
                        Some(yt) => match self.vote_for_issue(yt, p.has_vote, p.id.clone()).await {
                            Ok(_) => {
                                self.fetch_issues(
                                    user,
                                    &msg,
                                    BacklogParams::new_with_skip(state.top, state.skip),
                                )
                                .await?;
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
                            self.api.send(msg.text_reply(format!("No valid access token founds, use /login command to login in youtrack"))).await?;
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Ok(state.on_bot_command(cmd))
    }

    pub async fn dispatch_update(&mut self, update: Update) -> Result<()> {
        debug!("Got update: {:?}", update);
        let (uid, state) = self.get_state_by_update(&update)?;
        let command: BotCommand = update.try_into()?;

        match self.handle_command(state, command.clone()).await {
            Ok(new_state) => {
                self.states.insert(uid, new_state);
            }
            Err(e) => {
                warn!("Could not handle command {:?}: {}", command, e);
            }
        }

        Ok(())
    }
}
