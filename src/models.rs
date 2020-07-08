use serde::{Deserialize, Serialize};
use youtrack_rs::client::{Executor, YouTrack};

use super::errors::*;

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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct FieldType {
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CustomField {
    pub id: String,
    pub name: String,
    #[serde(rename = "fieldType")]
    pub field_type: FieldType,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct BundleElement {
    pub id: String,
    pub name: String,
}

pub type BundleElements = Vec<BundleElement>;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Bundle {
    pub id: String,
    pub values: Option<BundleElements>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ProjectCustomField {
    pub field: CustomField,
    pub ordinal: i32,
    #[serde(rename = "canBeEmpty")]
    pub can_be_emtpy: bool,
    pub bundle: Option<Bundle>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Project {
    pub id: String,
    /// From IssueFolder
    pub name: Option<String>,
    #[serde(rename = "shortName")]
    pub short_name: Option<String>,
    pub fields: Vec<ProjectCustomField>,
}

pub type Projects = Vec<Project>;

impl Project {
    pub async fn list(yt: &YouTrack) -> Result<Projects> {
        let projects = yt
            .get()
            .admin()
            .projects()
            .top("-1")
            .skip("0")
            .fields("id,name,short_name,fields(field(id,name,fieldType(id)),canBeEmpty,ordinal,bundle(id))")
            .execute::<Projects>()
            .await?;
        let (headers, status, projects) = projects;

        debug!("{:#?}", headers);
        debug!("{}", status);

        if !status.is_success() {
            bail!("Unable to fetch issues from youtrack")
        };
        if let Some(mut projects) = projects {
            projects.sort_by_cached_key(|k| k.name.clone());
            Ok(projects)
        } else {
            bail!("Unable to parse issues list")
        }
    }

    pub async fn streams(&self, yt: &YouTrack) -> Result<Bundle> {
        for field in self.fields.iter() {
            let f = &field.field;
            if f.name != "Stream" {
                continue;
            }
            println!("{:?}", f);
            let ref bundle = field.bundle;
            let bundle = bundle.clone().unwrap();
            let vals = yt
                .get()
                .admin()
                .custom_field_settings()
                .bundles()
                .enum_()
                .id(bundle.id.as_str())
                .fields("id,name,values(id,name)")
                .execute::<Bundle>()
                .await
                .unwrap();

            let (headers, status, json) = vals;
            println!("{:#?}", headers);
            println!("{}", status);
            println!("{:?}", json);
            return Ok(json.unwrap());
        }
        bail!("Not found")
    }
}
