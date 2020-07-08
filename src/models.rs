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

impl Bundle {
    pub fn has_value<T>(&self, name: T) -> bool
    where
        T: ToString,
    {
        let name = name.to_string();
        if let Some(values) = &self.values {
            values.iter().any(|x| x.name == name)
        } else {
            false
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ProjectCustomField {
    pub id: String,
    pub field: CustomField,
    pub ordinal: i32,
    #[serde(rename = "canBeEmpty")]
    pub can_be_emtpy: bool,
    pub bundle: Option<Bundle>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ProjectId {
    pub id: String,
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
            .fields("id,name,short_name,fields(id,field(id,name,fieldType(id)),canBeEmpty,ordinal,bundle(id))")
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

    pub fn get_project_custom_field<T>(&self, field_name: T) -> Option<&ProjectCustomField>
    where
        T: ToString,
    {
        let field_name = field_name.to_string();
        self.fields.iter().find(|&x| x.field.name == field_name)
    }

    pub async fn get_bundle<T, B>(&self, yt: &YouTrack, field_name: T) -> Result<B>
    where
        T: ToString,
        B: std::fmt::Debug + Send + Sync + for<'de> Deserialize<'de>,
    {
        match self.get_project_custom_field(field_name) {
            Some(field) => {
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
                    .execute::<B>()
                    .await?;

                let (headers, status, json) = vals;
                println!("{:#?}", headers);
                println!("{}", status);
                println!("{:?}", json);
                Ok(json.unwrap())
            }
            None => bail!("No such bundle"),
        }
    }

    pub async fn streams(&self, yt: &YouTrack) -> Result<Bundle> {
        self.get_bundle(yt, "Stream").await
    }

    pub async fn types(&self, yt: &YouTrack) -> Result<Bundle> {
        self.get_bundle(yt, "Type").await
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct IssueDraftCustomFieldValue {
    pub name: String,
}

impl IssueDraftCustomFieldValue {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct IssueDraftCustomField {
    pub value: IssueDraftCustomFieldValue,
    pub name: String,
    pub id: String,
    #[serde(rename = "$type")]
    pub type_: String,
}

impl IssueDraftCustomField {
    pub fn new(id: String, name: String, value: String) -> Self {
        Self {
            value: IssueDraftCustomFieldValue::new(value),
            name,
            id,
            type_: "SingleEnumIssueCustomField".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct IssueDraft {
    pub summary: String,
    pub description: String,
    pub project: Option<ProjectId>,
    #[serde(rename = "customFields")]
    pub custom_fields: Vec<IssueDraftCustomField>,
}

impl IssueDraft {
    pub fn new() -> Self {
        Self {
            summary: "".to_string(),
            description: "".to_string(),
            project: None,
            custom_fields: Vec::new(),
        }
    }

    pub fn summary(&mut self, summary: String) -> &mut Self {
        self.summary = summary;
        self
    }

    pub fn desc(&mut self, desc: String) -> &mut Self {
        self.description = desc;
        self
    }

    pub fn project(&mut self, project: ProjectId) -> &mut Self {
        self.project = Some(project);
        self
    }

    pub fn custom_field(&mut self, id: String, name: String, value: String) -> &mut Self {
        self.custom_fields
            .push(IssueDraftCustomField::new(id, name, value));
        self
    }
}
