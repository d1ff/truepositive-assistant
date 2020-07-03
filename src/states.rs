use crate::commands::BacklogParams;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq)]
pub struct StartBacklog(pub BacklogParams);

#[derive(Clone, Debug, PartialEq)]
pub struct BacklogPage(pub BacklogParams);

#[derive(Clone, Debug, PartialEq)]
pub struct StopBacklog;

#[derive(Clone, Debug, PartialEq)]
pub struct Noop;

machine!(
    #[derive(Clone, Debug, Deserialize, Serialize)]
    enum UserState {
        Idle,
        InBacklog { pub top: i32, pub skip: i32 },
    }
);

transitions!(UserState, [
    (Idle, StartBacklog) => InBacklog,
    (InBacklog, StopBacklog) => Idle,
    (InBacklog, BacklogPage) => InBacklog,
    (Idle, Noop) => Idle
]);

impl Idle {
    pub fn on_start_backlog(&self, m: StartBacklog) -> InBacklog {
        let StartBacklog(p) = m;
        InBacklog {
            top: p.top,
            skip: p.skip,
        }
    }

    pub fn on_noop(&self, _: Noop) -> Idle {
        Idle {}
    }
}

impl InBacklog {
    pub fn on_stop_backlog(&self, _: StopBacklog) -> Idle {
        Idle {}
    }

    pub fn on_backlog_page(&self, p: BacklogPage) -> InBacklog {
        let BacklogPage(p) = p;
        InBacklog {
            top: p.top,
            skip: p.skip,
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
