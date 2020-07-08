use crate::commands::BacklogParams;

use serde::{Deserialize, Serialize};

use crate::models::Project;

#[derive(Clone, Debug, PartialEq)]
pub struct StartBacklog(pub BacklogParams);

#[derive(Clone, Debug, PartialEq)]
pub struct BacklogPage(pub BacklogParams);

#[derive(Clone, Debug, PartialEq)]
pub struct StopBacklog;

#[derive(Clone, Debug, PartialEq)]
pub struct Save;

#[derive(Clone, Debug, PartialEq)]
pub struct Cancel;

macro_rules! on_cancel {
    () => {
        pub fn on_cancel(&self, _: Cancel) -> Idle {
            Idle {}
        }
    };
}

#[derive(Clone, Debug, PartialEq)]
pub struct Noop;

macro_rules! on_noop {
    () => {
        pub fn on_noop(&self, _: Noop) -> Self {
            self.clone()
        }
    };
}

#[derive(Clone, Debug, PartialEq)]
pub struct CreateNewIssue;

#[derive(Clone, Debug, PartialEq)]
pub struct IssueSummary(pub String);

#[derive(Clone, Debug, PartialEq)]
pub struct IssueSummaryProject(pub String, pub Project);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IssueStream(pub String, pub String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IssueType(pub String, pub String);

#[derive(Clone, Debug, PartialEq)]
pub struct IssueSummaryProjectStream(pub String, pub Project, pub IssueStream);

#[derive(Clone, Debug, PartialEq)]
pub struct IssueSummaryProjectStreamType(pub String, pub Project, pub IssueStream, pub IssueType);

#[derive(Clone, Debug, PartialEq)]
pub struct IssueSummaryProjectStreamTypeDesc(
    pub String,
    pub Project,
    pub IssueStream,
    pub IssueType,
    pub String,
);

machine!(
    #[derive(Clone, Debug, Deserialize, Serialize)]
    enum UserState {
        Idle,
        InBacklog {
            pub top: i32,
            pub skip: i32,
        },
        NewIssue,
        NewIssueSummary {
            pub summary: String,
        },
        NewIssueSummaryProject {
            pub summary: String,
            pub project: Project,
        },
        NewIssueSummaryProjectStream {
            pub summary: String,
            pub project: Project,
            pub stream: IssueStream,
        },
        NewIssueSummaryProjectStreamType {
            pub summary: String,
            pub project: Project,
            pub stream: IssueStream,
            pub issue_type: IssueType,
        },
        NewIssueSummaryProjectStreamTypeDesc {
            pub summary: String,
            pub project: Project,
            pub stream: IssueStream,
            pub issue_type: IssueType,
            pub desc: String,
        },
    }
);

transitions!(UserState, [
    (Idle, StartBacklog) => InBacklog,
    (InBacklog, StopBacklog) => Idle,
    (InBacklog, BacklogPage) => InBacklog,
    (Idle, Noop) => Idle,
    (Idle, CreateNewIssue) => NewIssue,
    (NewIssue, IssueSummary) => NewIssueSummary,
    (NewIssue, Cancel) => Idle,
    (NewIssue, Noop) => NewIssue,
    (NewIssueSummary, IssueSummaryProject) => NewIssueSummaryProject,
    (NewIssueSummary, Cancel) => Idle,
    (NewIssueSummary, Noop) => NewIssueSummary,
    (NewIssueSummaryProject, IssueSummaryProjectStream) => NewIssueSummaryProjectStream,
    (NewIssueSummaryProject, Cancel) => Idle,
    (NewIssueSummaryProject, Noop) => NewIssueSummaryProject,
    (NewIssueSummaryProjectStream, IssueSummaryProjectStreamType) => NewIssueSummaryProjectStreamType,
    (NewIssueSummaryProjectStream, Cancel) => Idle,
    (NewIssueSummaryProjectStream, Noop) => NewIssueSummaryProjectStream,
    (NewIssueSummaryProjectStreamType, IssueSummaryProjectStreamTypeDesc) => NewIssueSummaryProjectStreamTypeDesc,
    (NewIssueSummaryProjectStreamType, Cancel) => Idle,
    (NewIssueSummaryProjectStreamType, Noop) => NewIssueSummaryProjectStreamType,
    (NewIssueSummaryProjectStreamTypeDesc, Save) => Idle,
    (NewIssueSummaryProjectStreamTypeDesc, Cancel) => Idle,
    (NewIssueSummaryProjectStreamTypeDesc, Noop) => NewIssueSummaryProjectStreamTypeDesc
]);

impl Idle {
    pub fn on_start_backlog(&self, m: StartBacklog) -> InBacklog {
        let StartBacklog(p) = m;
        InBacklog {
            top: p.top,
            skip: p.skip,
        }
    }

    pub fn on_create_new_issue(&self, _: CreateNewIssue) -> NewIssue {
        NewIssue {}
    }

    on_noop!();
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

macro_rules! on_issue_message {
    ($msg:tt, $($f:ident),+) => {
        paste::item! {
            pub fn [<on_ $msg:snake>](&self, m: $msg) -> [<New $msg>] {
                let $msg($($f),+) = m;
                [<New $msg>] { $($f),+ }
            }
        }
    };
}

macro_rules! make_forward {
    ($msg:tt, $n:ident, $t:ty) => {
        pub fn $n(&self, $n: $t) -> UserStateMessages {
            UserStateMessages::$msg($msg($n))
        }
    };
    ($msg:tt, $n:ident, $t:ty, $($f:ident),*) => {
        pub fn $n(&self, $n: $t) -> UserStateMessages {
            UserStateMessages::$msg($msg($(self.$f.clone()),*, $n))
        }
    };
}

macro_rules! impl_new_issue_state {
    ($t:ty, $msg:tt, $n:ident, $nt:ty) => {
        impl $t {
            on_issue_message!($msg, $n);
            on_cancel!();
            on_noop!();
            make_forward!($msg, $n, $nt);
        }
    };
    ($t:ty, $msg:tt, $n:ident, $nt:ty, $($f:ident),*) => {
        impl $t {
            on_issue_message!($msg, $($f),*, $n);
            on_cancel!();
            on_noop!();
            make_forward!($msg, $n, $nt, $($f),*);
        }
    };
}

impl_new_issue_state!(NewIssue, IssueSummary, summary, String);
impl_new_issue_state!(
    NewIssueSummary,
    IssueSummaryProject,
    project,
    Project,
    summary
);
impl_new_issue_state!(
    NewIssueSummaryProject,
    IssueSummaryProjectStream,
    stream,
    IssueStream,
    summary,
    project
);
impl_new_issue_state!(
    NewIssueSummaryProjectStream,
    IssueSummaryProjectStreamType,
    issue_type,
    IssueType,
    summary,
    project,
    stream
);
impl_new_issue_state!(
    NewIssueSummaryProjectStreamType,
    IssueSummaryProjectStreamTypeDesc,
    desc,
    String,
    summary,
    project,
    stream,
    issue_type
);

impl NewIssueSummaryProjectStreamTypeDesc {
    pub fn on_save(&self, _: Save) -> Idle {
        Idle {}
    }

    on_cancel!();
    on_noop!();
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
