//! Canonical Markdown artefacts. Generated files are never part of this model.
use crate::{documents, ResourceKey, StorageError, StorageResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum DesignTargetKind {
    Element,
    Relationship,
    Uml,
    Mockup,
    Markdown,
}

impl DesignTargetKind {
    pub fn resource_kind(self) -> &'static str {
        match self {
            Self::Element => "design.element",
            Self::Relationship => "design.relationship",
            Self::Uml => "design.uml",
            Self::Mockup => "mockup.accepted",
            Self::Markdown => "design.markdown",
        }
    }
}

/// Array position defines association order. Links do not imply ownership.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesignAssociation {
    pub target_type: DesignTargetKind,
    pub design_external_id: String,
}

impl DesignAssociation {
    pub fn resource_key(&self) -> ResourceKey {
        ResourceKey {
            kind: self.target_type.resource_kind().into(),
            id: self.design_external_id.clone(),
        }
    }
}

/// Complete editable content. The enclosing snapshot supplies documentId/readToken.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkdownDesignDocument {
    pub external_id: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub design_links: Vec<DesignAssociation>,
}

/// Inventory/search records deliberately omit the authored body.
#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkdownSummary {
    pub external_id: String,
    pub title: String,
    pub design_links: Vec<DesignAssociation>,
    pub read_token: String,
}

/// Filters intersect. Association lookup is one hop; callers bound any traversal.
/// Empty filters list project documents, ordered by externalId, with keyset paging.
#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkdownQuery {
    #[serde(default)]
    pub ids: Vec<String>,
    pub linked_to: Option<DesignAssociation>,
    pub file: Option<String>,
    pub symbol: Option<String>,
    /// Literal case-insensitive title/body substring, not a regular expression.
    pub query: Option<String>,
    pub after_id: Option<String>,
    #[serde(default = "default_limit")]
    #[schemars(range(min = 1, max = 100))]
    pub limit: u32,
}
fn default_limit() -> u32 {
    25
}
impl Default for MarkdownQuery {
    fn default() -> Self {
        Self {
            ids: vec![],
            linked_to: None,
            file: None,
            symbol: None,
            query: None,
            after_id: None,
            limit: default_limit(),
        }
    }
}
impl MarkdownQuery {
    pub fn validate(&self) -> StorageResult<()> {
        if !(1..=100).contains(&self.limit) {
            return Err(invalid("Markdown limit must be 1..100"));
        }
        for id in &self.ids {
            validate_identity(id)?;
        }
        if let Some(link) = &self.linked_to {
            validate_identity(&link.design_external_id)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkdownPage {
    /// Number matching this query after the cursor, before the page limit.
    pub total_count: usize,
    pub documents: Vec<MarkdownSummary>,
    pub next_after_id: Option<String>,
}

/// Incoming references include task, QA, bindings and other design artefacts.
/// Delete validation uses ALL remaining edges in the final transaction graph,
/// never a paginated presentation of these backlinks.
#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkdownBacklink {
    pub source_kind: String,
    pub source_id: String,
    pub title: String,
}

fn invalid(message: &str) -> StorageError {
    StorageError::Validation(message.into())
}
pub fn validate_identity(id: &str) -> StorageResult<()> {
    if id.is_empty() || id.trim() != id || id.chars().any(char::is_control) {
        return Err(invalid(
            "Markdown identity must be nonempty, trimmed and contain no control characters",
        ));
    }
    Ok(())
}
impl MarkdownDesignDocument {
    pub fn validate(&self) -> StorageResult<()> {
        validate_identity(&self.external_id)?;
        if self.title.trim().is_empty() {
            return Err(invalid("Markdown title is required"));
        }
        let mut unique = BTreeSet::new();
        for link in &self.design_links {
            validate_identity(&link.design_external_id)?;
            if !unique.insert(link) {
                return Err(invalid("Duplicate Markdown design association"));
            }
        }
        Ok(())
    }
    /// Project isolation is supplied by the owning snapshot/transaction, never a
    /// caller-provided project field. Preserve body whitespace byte-for-byte.
    pub fn snapshot(&self) -> StorageResult<documents::DesignDocument> {
        self.validate()?;
        let document_id = format!("markdown:{}", self.external_id);
        let document = serde_json::to_value(self).map_err(StorageError::backend)?;
        let read_token =
            documents::document_token(&document_id, &document).map_err(StorageError::backend)?;
        Ok(documents::DesignDocument {
            document_id,
            document,
            read_token,
        })
    }
    pub fn summary(&self) -> StorageResult<MarkdownSummary> {
        Ok(MarkdownSummary {
            external_id: self.external_id.clone(),
            title: self.title.clone(),
            design_links: self.design_links.clone(),
            read_token: self.snapshot()?.read_token,
        })
    }
    /// Drivers resolve these edges only within the owning project's final graph.
    pub fn references(&self) -> Vec<crate::MissingReference> {
        self.design_links
            .iter()
            .map(|link| crate::MissingReference {
                source: ResourceKey {
                    kind: "design.markdown".into(),
                    id: self.external_id.clone(),
                },
                target: link.resource_key(),
            })
            .collect()
    }
}
