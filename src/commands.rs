use serde::{Deserialize, Serialize};
use std::convert::{From, TryFrom};
use telegram_bot::types::{
    CallbackQuery, InlineKeyboardButton, Message, MessageKind, Update, UpdateKind, User,
};

use crate::errors::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename = "bp")]
pub struct BacklogParams {
    #[serde(rename = "t")]
    pub top: i32,
    #[serde(rename = "s")]
    pub skip: i32,
}

impl BacklogParams {
    pub fn new(top: i32) -> Self {
        Self { top, skip: 0 }
    }

    pub fn new_with_skip(top: i32, skip: i32) -> Self {
        Self { top, skip }
    }

    pub fn next(&self) -> Self {
        Self {
            top: self.top,
            skip: self.skip + self.top,
        }
    }

    pub fn prev(&self) -> Option<Self> {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename = "vfi")]
pub struct VoteForIssueParams {
    #[serde(rename = "i")]
    pub id: String,
    #[serde(rename = "v")]
    pub has_vote: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "_t")]
pub enum CallbackParams {
    #[serde(rename = "bn")]
    BacklogNext(BacklogParams),
    #[serde(rename = "bp")]
    BacklogPrev(BacklogParams),
    #[serde(rename = "vi")]
    VoteForIssue(VoteForIssueParams),
    #[serde(rename = "bs")]
    BacklogStop,
}

impl From<CallbackParams> for InlineKeyboardButton {
    fn from(item: CallbackParams) -> Self {
        let text: String = match &item {
            CallbackParams::BacklogStop => "stop".to_string(),
            CallbackParams::BacklogNext(_) => "next".to_string(),
            CallbackParams::BacklogPrev(_) => "prev".to_string(),
            CallbackParams::VoteForIssue(p) => {
                if p.has_vote {
                    format!("{} {}", emoji!("star2"), p.id)
                } else {
                    p.id.clone()
                }
            }
        };
        let val = serde_json::to_string(&item).unwrap();
        if val.len() > 64 {
            panic!("Callback paramater too big: {}", val);
        }
        InlineKeyboardButton::callback(text, val)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum BotCommand {
    Start(Message),
    Backlog(Message, BacklogParams),
    Login(Message),
    Stop(Message),
    Text(Message),
    NewIssue(Message),
    BacklogStop(CallbackQuery),
    BacklogNext(CallbackQuery, BacklogParams),
    BacklogPrev(CallbackQuery, BacklogParams),
    BacklogVoteForIssue(CallbackQuery, VoteForIssueParams),
    Save(Message),
    Cancel(Message),
}

impl BotCommand {
    pub fn get_message_text(&self) -> Option<String> {
        match self {
            BotCommand::Text(msg) => {
                if let MessageKind::Text { ref data, .. } = msg.kind {
                    Some(data.clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn get_user(&self) -> &User {
        match self {
            BotCommand::Start(m) => &m.from,
            BotCommand::Backlog(m, _) => &m.from,
            BotCommand::Login(m) => &m.from,
            BotCommand::Stop(m) => &m.from,
            BotCommand::Text(m) => &m.from,
            BotCommand::NewIssue(m) => &m.from,
            BotCommand::BacklogStop(m) => &m.from,
            BotCommand::BacklogNext(m, _) => &m.from,
            BotCommand::BacklogPrev(m, _) => &m.from,
            BotCommand::BacklogVoteForIssue(m, _) => &m.from,
            BotCommand::Save(m) => &m.from,
            BotCommand::Cancel(m) => &m.from,
        }
    }
}

impl TryFrom<Message> for BotCommand {
    type Error = Error;

    fn try_from(msg: Message) -> Result<Self> {
        if let MessageKind::Text { ref data, .. } = msg.kind {
            debug!(
                "<{}>: {} {} {}",
                &msg.from.first_name,
                &msg.from.id,
                &msg.chat.id(),
                data
            );
            let cmd = match data.as_str() {
                "/backlog" => BotCommand::Backlog(msg, BacklogParams::new(5)),
                "/start" => BotCommand::Start(msg),
                "/login" => BotCommand::Login(msg),
                "/stop" => BotCommand::Stop(msg),
                "/new_issue" => BotCommand::NewIssue(msg),
                "/save" => BotCommand::Save(msg),
                "/cancel" => BotCommand::Cancel(msg),
                _ => BotCommand::Text(msg),
            };
            Ok(cmd)
        } else {
            bail!("Unsupported message kind")
        }
    }
}

impl TryFrom<CallbackQuery> for BotCommand {
    type Error = Error;

    fn try_from(cb: CallbackQuery) -> Result<Self> {
        if let Some(ref data) = cb.data {
            let params = serde_json::from_str::<CallbackParams>(data)?;
            Ok(match params {
                CallbackParams::BacklogStop => BotCommand::BacklogStop(cb),
                CallbackParams::BacklogNext(p) => BotCommand::BacklogNext(cb, p),
                CallbackParams::BacklogPrev(p) => BotCommand::BacklogPrev(cb, p),
                CallbackParams::VoteForIssue(p) => BotCommand::BacklogVoteForIssue(cb, p),
            })
        } else {
            bail!("No callback query data")
        }
    }
}

impl TryFrom<Update> for BotCommand {
    type Error = Error;

    fn try_from(update: Update) -> Result<Self> {
        match update.kind {
            UpdateKind::Message(msg) => BotCommand::try_from(msg),
            UpdateKind::CallbackQuery(cb) => BotCommand::try_from(cb),
            _ => bail!("Unsupported update type"),
        }
    }
}
