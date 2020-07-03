use crate::commands::BotCommand;

use serde::{Deserialize, Serialize};

machine!(
    #[derive(Clone, Debug, Deserialize, Serialize)]
    enum UserState {
        Idle,
        InBacklog { pub top: i32, pub skip: i32 },
    }
);

transitions!(UserState, [
    (Idle, BotCommand) => [InBacklog, Idle],
    (InBacklog, BotCommand) => [InBacklog, Idle]
]);

impl Idle {
    pub fn on_bot_command(&self, cmd: BotCommand) -> UserState {
        match cmd {
            BotCommand::Backlog(_, p) => UserState::in_backlog(p.top, p.skip),
            _ => UserState::idle(),
        }
    }
}

impl InBacklog {
    pub fn on_bot_command(&self, cmd: BotCommand) -> UserState {
        match cmd {
            BotCommand::BacklogStop(_) => UserState::idle(),
            BotCommand::BacklogNext(_, p) | BotCommand::BacklogPrev(_, p) => {
                UserState::in_backlog(p.top, p.skip)
            }
            _ => UserState::in_backlog(self.top, self.skip),
        }
    }
}

impl redis::FromRedisValue for UserState {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
        match v {
            redis::Value::Status(ref s) => serde_json::from_str(s)
                .map_err(|_| (redis::ErrorKind::TypeError, "Unable to parse value").into()),
            redis::Value::Data(ref bytes) => serde_json::from_slice(bytes)
                .map_err(|_| (redis::ErrorKind::TypeError, "Unable to parse value").into()),
            _ => Err((redis::ErrorKind::TypeError, "Unable to parse value").into()),
        }
    }
}

impl redis::ToRedisArgs for UserState {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + redis::RedisWrite,
    {
        let v = serde_json::to_string(self).unwrap();
        out.write_arg(v.as_bytes());
    }
}
