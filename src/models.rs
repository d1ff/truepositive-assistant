use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct IssueVoters {
    #[serde(alias = "hasVote")]
    pub has_vote: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Issue {
    #[serde(alias = "idReadable")]
    pub id_readable: String,
    pub summary: String,
    pub votes: i32,
    pub voters: IssueVoters,
}

pub type Issues = Vec<Issue>;

#[derive(Serialize, Deserialize)]
pub struct YoutrackError {
    pub error: String,
    pub error_description: String,
    pub error_developer_message: String,
}
