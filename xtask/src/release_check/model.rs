//! The plain model `release-check` runs its rules over: the workspace's
//! members and their in-workspace dependency edges, built once from
//! `cargo metadata --no-deps` and the root manifest, so the rules can be
//! unit-tested with hand-built values.

use std::collections::BTreeSet;
use std::fmt;

use cargo_metadata::semver::VersionReq;
use cargo_metadata::{DependencyKind, Metadata};

/// The kind of a dependency edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Kind {
    Normal,
    Build,
    Dev,
}

impl Kind {
    /// Whether the edge is part of the published graph (normal or build).
    pub(super) fn is_published(self) -> bool {
        matches!(self, Self::Normal | Self::Build)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Normal => "normal",
            Self::Build => "build",
            Self::Dev => "dev",
        })
    }
}

/// One dependency edge from a member to another member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Dep {
    /// The depended-on member's package name.
    pub(super) to: String,
    pub(super) kind: Kind,
    /// The requirement is `*`: a `path` dependency without a `version`,
    /// which `cargo publish` strips (dev) or rejects (normal, build).
    pub(super) path_only: bool,
}

/// One workspace member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Member {
    pub(super) name: String,
    pub(super) version: String,
    /// `publish` is not `Some([])` (`publish = false`).
    pub(super) published: bool,
    pub(super) description: Option<String>,
    pub(super) license: Option<String>,
    pub(super) repository: Option<String>,
    /// Edges to other members only; external dependencies are dropped.
    pub(super) deps: Vec<Dep>,
}

/// The workspace as `release-check` sees it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Workspace {
    /// W: `[workspace.package] version` of the root manifest.
    pub(super) version: Option<String>,
    /// U: `[workspace.package] repository` of the root manifest.
    pub(super) repository: Option<String>,
    /// Members sorted by name.
    pub(super) members: Vec<Member>,
}

impl Workspace {
    /// Builds the model from `cargo metadata --no-deps` and the parsed root
    /// manifest (`None` when it could not be read).
    pub(super) fn from_metadata(metadata: &Metadata, manifest: Option<&toml::Table>) -> Self {
        let package_field = |key: &str| {
            manifest?
                .get("workspace")?
                .get("package")?
                .get(key)?
                .as_str()
                .map(str::to_owned)
        };
        let members: Vec<_> = metadata.workspace_packages();
        let names: BTreeSet<&str> = members.iter().map(|p| p.name.as_str()).collect();
        let mut members: Vec<Member> = members
            .iter()
            .map(|package| Member {
                name: package.name.to_string(),
                version: package.version.to_string(),
                published: package.publish.as_ref().is_none_or(|r| !r.is_empty()),
                description: package.description.clone(),
                license: package.license.clone(),
                repository: package.repository.clone(),
                deps: package
                    .dependencies
                    .iter()
                    .filter(|dep| dep.path.is_some() && names.contains(dep.name.as_str()))
                    .map(|dep| Dep {
                        to: dep.name.clone(),
                        kind: match dep.kind {
                            DependencyKind::Development => Kind::Dev,
                            DependencyKind::Build => Kind::Build,
                            _ => Kind::Normal,
                        },
                        path_only: dep.req == VersionReq::STAR,
                    })
                    .collect(),
            })
            .collect();
        members.sort_by(|a, b| a.name.cmp(&b.name));
        Self {
            version: package_field("version"),
            repository: package_field("repository"),
            members,
        }
    }

    /// The member called `name`.
    pub(super) fn member(&self, name: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.name == name)
    }

    /// P: the published members.
    pub(super) fn published(&self) -> impl Iterator<Item = &Member> {
        self.members.iter().filter(|m| m.published)
    }

    /// Whether `name` is a published member.
    pub(super) fn is_published(&self, name: &str) -> bool {
        self.member(name).is_some_and(|m| m.published)
    }
}
