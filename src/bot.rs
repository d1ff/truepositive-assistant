use oauth2::basic::BasicClient;
use oauth2::{CsrfToken, Scope};
use redis;
use redis::Commands;
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

fn make_reply_keyboard<T>(values: Vec<T>, f: fn(&T) -> String) -> ReplyKeyboardMarkup {
    let mut kb = ReplyKeyboardMarkup::new();
    kb.one_time_keyboard().resize_keyboard();

    for chunk in values.chunks(3) {
        let mut row: Vec<KeyboardButton> = Vec::new();
        for val in chunk.iter() {
            row.push(KeyboardButton::new(f(val)));
        }
        kb.add_row(row);
    }
    kb
}

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

macro_rules! match_user_state {
    ($s:ty, $var:ident, $($value:path),+) => {
        paste::expr! {
            match $var {
                $($s::$value(state) => self.[<handle_command_ $value:snake>](&state, cmd).await?),+,
                $s::Error => self.handle_command_error(cmd).await?,
            }
        }
    };
}

pub struct Bot {
    api: Api,
    yt: YouTrack,
    pub templates: Tera,
    pub yt_oauth: BasicClient,
    backlog_query: String,
    csrf_tokens: HashMap<String, UserId>,
    yt_tokens: TtlCache<UserId, YouTrack>,
    redis: redis::Client,
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
            redis: redis::Client::open(opts.redis_url)?,
        })
    }

    pub fn stream(&self) -> UpdatesStream {
        self.api.stream()
    }

    pub async fn get_youtrack(&self, user: UserId) -> Option<&YouTrack> {
        self.yt_tokens.get(&user)
    }

    pub async fn list_backlog(
        &self,
        message: &Message,
        b: &BacklogParams,
    ) -> Result<UserStateMessages> {
        self.fetch_issues(message.from.id, message, b).await
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

    async fn get_projects(&self) -> Result<Projects> {
        Project::list(&self.yt).await
    }

    async fn get_project(&self, name: String) -> Result<Project> {
        let projects = self.get_projects().await?;
        let name = Some(name);
        match projects.binary_search_by_key(&name, |p| p.name.clone()) {
            Ok(r) => Ok(projects.get(r).unwrap().clone()),
            Err(_) => bail!("No such project"),
        }
    }

    pub async fn fetch_issues(
        &self,
        user: UserId,
        msg: &Message,
        params: &BacklogParams,
    ) -> Result<UserStateMessages> {
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
                        if params.skip == 0 {
                            Ok(UserStateMessages::StartBacklog(StartBacklog(
                                params.clone(),
                            )))
                        } else {
                            Ok(UserStateMessages::BacklogPage(BacklogPage(params.clone())))
                        }
                    }
                    Err(e) => {
                        warn!("Error occured: {}", e);
                        self.api
                            .spawn(msg.text_reply(format!("Error occured: {}", e)));
                        Ok(UserStateMessages::Noop(Noop {}))
                    }
                }
            }
            None => {
                warn!("No token found for user: {}", user);
                self.api.spawn(msg.text_reply(format!(
                    "No valid access token founds, use /login command to login in youtrack"
                )));
                Ok(UserStateMessages::Noop(Noop {}))
            }
        }
    }

    async fn handle_start(&self, msg: &Message) -> Result<UserStateMessages> {
        let mut context = Context::new();
        context.insert("name", &msg.from.first_name);
        let txt_msg = self.templates.render("start.md", &context).unwrap();
        self.api
            .send(msg.text_reply(txt_msg).parse_mode(ParseMode::Markdown))
            .await?;

        Ok(UserStateMessages::Noop(Noop {}))
    }

    async fn handle_login(&mut self, msg: &Message) -> Result<UserStateMessages> {
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

        Ok(UserStateMessages::Noop(Noop {}))
    }

    async fn handle_new_issue(&self, msg: &Message) -> Result<UserStateMessages> {
        let kb = reply_markup!(force_reply);
        self.api
            .send(
                msg.text_reply("Creating new issue. Please, enter issue summary.")
                    .reply_markup(kb),
            )
            .await?;
        Ok(UserStateMessages::CreateNewIssue(CreateNewIssue {}))
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

    fn get_state(&mut self, uid: UserId) -> Result<UserState> {
        let mut con = self.redis.get_connection()?;
        let key = format!("state:{}", uid);
        match con.get(key)? {
            Some(state) => Ok(state),
            None => Ok(UserState::idle()),
        }
    }

    fn get_state_by_update(&mut self, update: &Update) -> Result<(UserId, UserState)> {
        let uid = match &update.kind {
            UpdateKind::Message(m) => m.from.id,
            UpdateKind::CallbackQuery(cb) => cb.from.id,
            _ => bail!("Unsupported update type"),
        };
        let state = self.get_state(uid)?;
        Ok((uid, state))
    }

    async fn handle_command_idle(
        &mut self,
        _state: &Idle,
        cmd: BotCommand,
    ) -> Result<UserStateMessages> {
        Ok(match &cmd {
            BotCommand::Backlog(msg, p) => self.list_backlog(msg, p).await?,
            BotCommand::Start(msg) => self.handle_start(msg).await?,
            BotCommand::Login(msg) => self.handle_login(msg).await?,
            BotCommand::NewIssue(msg) => self.handle_new_issue(msg).await?,
            _ => UserStateMessages::Noop(Noop {}),
        })
    }

    async fn handle_command_in_backlog(
        &mut self,
        state: &InBacklog,
        cmd: BotCommand,
    ) -> Result<UserStateMessages> {
        let msg = match &cmd {
            BotCommand::BacklogStop(cb) => {
                let msg = cb.message.clone().unwrap();
                self.api
                    .send(msg.edit_reply_markup(Some(reply_markup!(inline_keyboard, []))))
                    .await?;
                UserStateMessages::StopBacklog(StopBacklog {})
            }
            BotCommand::BacklogNext(cb, p) | BotCommand::BacklogPrev(cb, p) => {
                let msg = cb.message.clone().unwrap();
                self.fetch_issues(cb.from.id, &msg, p).await?
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
                                &BacklogParams::new_with_skip(state.top, state.skip),
                            )
                            .await?
                        }
                        Err(e) => {
                            warn!("Error occured: {}", e);
                            self.api
                                .spawn(msg.text_reply(format!("Error occured: {}", e)));
                            UserStateMessages::Noop(Noop {})
                        }
                    },
                    None => {
                        warn!("No youtrack instance for user {}", user);
                        self.api
                            .spawn(msg.edit_reply_markup(Some(reply_markup!(inline_keyboard, []))));
                        self.api.spawn(msg.text_reply(format!(
                            "No valid access token founds, use /login command to login in youtrack"
                        )));
                        UserStateMessages::StopBacklog(StopBacklog {})
                    }
                }
            }
            _ => UserStateMessages::Noop(Noop {}),
        };
        Ok(msg)
    }

    async fn handle_command_new_issue(
        &mut self,
        state: &NewIssue,
        cmd: BotCommand,
    ) -> Result<UserStateMessages> {
        let res = match &cmd {
            BotCommand::Text(msg) => {
                if let Some(summary) = cmd.get_message_text() {
                    let projects = self.get_projects().await?;
                    let kb = make_reply_keyboard(projects, |s| s.name.clone().unwrap());
                    self.api.spawn(
                        msg.from
                            .text(format!("Got it. Now select project for the issue."))
                            .reply_markup(kb),
                    );
                    state.summary(summary)
                } else {
                    UserStateMessages::Noop(Noop {})
                }
            }
            BotCommand::Cancel(msg) => {
                self.api.spawn(msg.from.text("cancel"));
                UserStateMessages::Cancel(Cancel {})
            }
            _ => UserStateMessages::Noop(Noop {}),
        };
        Ok(res)
    }

    async fn handle_command_new_issue_summary(
        &mut self,
        state: &NewIssueSummary,
        cmd: BotCommand,
    ) -> Result<UserStateMessages> {
        let res = match &cmd {
            BotCommand::Text(msg) => {
                if let Some(project) = cmd.get_message_text() {
                    match self.get_project(project).await {
                        Ok(project) => {
                            let stream = project.streams(&self.yt).await?;
                            let values = stream.values.unwrap();
                            let kb = make_reply_keyboard(values, |s| s.name.clone());
                            self.api.spawn(
                                msg.from
                                    .text("Got it. Now select stream for the issue")
                                    .reply_markup(kb),
                            );
                            state.project(project)
                        }
                        Err(_) => UserStateMessages::Noop(Noop {}),
                    }
                } else {
                    UserStateMessages::Noop(Noop {})
                }
            }
            BotCommand::Cancel(msg) => {
                self.api.spawn(msg.from.text("cancel"));
                UserStateMessages::Cancel(Cancel {})
            }
            _ => UserStateMessages::Noop(Noop {}),
        };
        Ok(res)
    }

    async fn handle_command_new_issue_summary_project(
        &mut self,
        state: &NewIssueSummaryProject,
        cmd: BotCommand,
    ) -> Result<UserStateMessages> {
        let res = match &cmd {
            BotCommand::Text(msg) => {
                if let Some(stream) = cmd.get_message_text() {
                    // TODO: parse stream
                    let stream_bundle = state.project.streams(&self.yt).await?;
                    if stream_bundle.has_value(&stream) {
                        let type_bundle = state.project.types(&self.yt).await?;
                        let values = type_bundle.values.unwrap();
                        let kb = make_reply_keyboard(values, |s| s.name.clone());
                        self.api.spawn(
                            msg.from
                                .text("Got it. Now select issue type.")
                                .reply_markup(kb),
                        );
                        let field = state.project.get_project_custom_field("Stream").unwrap();
                        state.stream(IssueStream(field.id.clone(), stream))
                    } else {
                        UserStateMessages::Noop(Noop {})
                    }
                } else {
                    UserStateMessages::Noop(Noop {})
                }
            }
            BotCommand::Cancel(msg) => {
                self.api.spawn(msg.from.text("cancel"));
                UserStateMessages::Cancel(Cancel {})
            }
            _ => UserStateMessages::Noop(Noop {}),
        };
        Ok(res)
    }

    async fn handle_command_new_issue_summary_project_stream(
        &mut self,
        state: &NewIssueSummaryProjectStream,
        cmd: BotCommand,
    ) -> Result<UserStateMessages> {
        let res = match &cmd {
            BotCommand::Text(msg) => {
                if let Some(issue_type) = cmd.get_message_text() {
                    // TODO: parse type
                    //
                    let type_bundle = state.project.types(&self.yt).await?;
                    if type_bundle.has_value(&issue_type) {
                        self.api
                            .spawn(msg.from.text("Got it. Now type in issue description."));

                        let field = state.project.get_project_custom_field("Type").unwrap();
                        state.issue_type(IssueType(field.id.clone(), issue_type))
                    } else {
                        UserStateMessages::Noop(Noop {})
                    }
                } else {
                    UserStateMessages::Noop(Noop {})
                }
            }
            BotCommand::Cancel(msg) => {
                self.api.spawn(msg.from.text("cancel"));
                UserStateMessages::Cancel(Cancel {})
            }
            _ => UserStateMessages::Noop(Noop {}),
        };
        Ok(res)
    }

    async fn handle_command_new_issue_summary_project_stream_type(
        &mut self,
        state: &NewIssueSummaryProjectStreamType,
        cmd: BotCommand,
    ) -> Result<UserStateMessages> {
        Ok(match &cmd {
            BotCommand::Text(msg) => {
                if let Some(desc) = cmd.get_message_text() {
                    let kb = reply_markup!(
                        reply_keyboard,
                        selective,
                        one_time,
                        resize,
                        ["/save", "/cancel"]
                    );

                    let mut context = Context::new();
                    context.insert("issue", &state);
                    context.insert("desc", &desc);
                    let txt_msg = self.templates.render("new_issue.md", &context).unwrap();

                    self.api.spawn(
                        msg.from
                            .text(txt_msg)
                            .reply_markup(kb)
                            .parse_mode(ParseMode::Markdown),
                    );
                    state.desc(desc)
                } else {
                    UserStateMessages::Noop(Noop {})
                }
            }
            BotCommand::Cancel(msg) => {
                self.api.spawn(msg.from.text("cancel"));
                UserStateMessages::Cancel(Cancel {})
            }
            _ => UserStateMessages::Noop(Noop {}),
        })
    }

    async fn handle_command_new_issue_summary_project_stream_type_desc(
        &mut self,
        state: &NewIssueSummaryProjectStreamTypeDesc,
        cmd: BotCommand,
    ) -> Result<UserStateMessages> {
        let res = match &cmd {
            BotCommand::Save(msg) => {
                self.api.spawn(msg.from.text("save"));
                let mut new_issue = IssueDraft::new();
                let new_issue = new_issue
                    .summary(state.summary.clone())
                    .desc(state.desc.clone())
                    .project(ProjectId {
                        id: state.project.id.clone(),
                    })
                    .custom_field(
                        state.stream.0.clone(),
                        "Stream".to_string(),
                        state.stream.1.clone(),
                    )
                    .custom_field(
                        state.issue_type.0.clone(),
                        "Type".to_string(),
                        state.issue_type.1.clone(),
                    );
                let i = self.yt.post(new_issue).issues().fields("idReadable");
                let (headers, status, json) = i.execute::<Value>().await?;

                debug!("{:#?}", headers);
                debug!("{}", status);
                debug!("{:?}", json);
                if status.is_success() {
                    let issue_id = json.unwrap();
                    let issue_id = issue_id.get("idReadable").unwrap().as_str().unwrap();
                    self.api
                        .spawn(msg.from.text(format!("Issue {} created", issue_id)))
                } else {
                    if let Ok(err) = serde_json::from_value::<YoutrackError>(json.unwrap()) {
                        // TODO: wrap into YoutrackError kind
                        bail!(err.error_description);
                    } else {
                        bail!("Unable to create issue");
                    }
                };
                UserStateMessages::Save(Save {})
            }
            BotCommand::Cancel(msg) => {
                self.api.spawn(msg.from.text("cancel"));
                UserStateMessages::Cancel(Cancel {})
            }
            _ => UserStateMessages::Noop(Noop {}),
        };
        Ok(res)
    }
    async fn handle_command_error(&mut self, _cmd: BotCommand) -> Result<UserStateMessages> {
        Ok(UserStateMessages::Noop(Noop {}))
    }

    async fn handle_command(&mut self, state: UserState, cmd: BotCommand) -> Result<UserState> {
        let state_copy = state.clone();
        let state_cmd = match_user_state!(
            UserState,
            state_copy,
            Idle,
            InBacklog,
            NewIssue,
            NewIssueSummary,
            NewIssueSummaryProject,
            NewIssueSummaryProjectStream,
            NewIssueSummaryProjectStreamType,
            NewIssueSummaryProjectStreamTypeDesc
        );
        let new_state = state.execute(state_cmd);
        if let UserState::Error = new_state {
            bail!("Invalid transition")
        }
        Ok(new_state)
    }

    pub async fn dispatch_update(&mut self, update: Update) -> Result<()> {
        debug!("Got update: {:?}", update);
        let (uid, state) = self.get_state_by_update(&update)?;
        debug!("UID: {}, STATE: {:?}", uid, state);
        let command: BotCommand = update.try_into()?;

        match self.handle_command(state, command.clone()).await {
            Ok(new_state) => {
                let mut con = self.redis.get_connection()?;
                let key = format!("state:{}", uid);
                con.set(key, new_state)?;
            }
            Err(e) => {
                warn!("Could not handle command: {}", e);
            }
        }

        Ok(())
    }
}
