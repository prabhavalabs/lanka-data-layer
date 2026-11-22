//! Data scope and principals (12 §2, §4).
//!
//! Every app-plane query that can touch workspace data takes a [`Scope`];
//! there is no code path that builds a workspace query without one.
//! Row-level security on `workspace_id` (WP-31/WP-32) is the second barrier.

use crate::rate::RateIdentity;

/// Which data a request may read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Scope {
    /// Public data only (`catalog`, `geo`, `data`, `serve`).
    #[default]
    Public,
    /// Public data plus the listed workspaces (ids of `app.workspaces`).
    Workspace(Vec<i64>),
}

impl Scope {
    /// Workspaces readable in this scope (empty for [`Scope::Public`]).
    #[must_use]
    pub fn workspaces(&self) -> &[i64] {
        match self {
            Self::Public => &[],
            Self::Workspace(ids) => ids,
        }
    }

    #[must_use]
    pub fn can_read_workspace(&self, id: i64) -> bool {
        self.workspaces().contains(&id)
    }

    /// The `SET LOCAL app.workspaces` value for RLS (`{1,2}`; `{}` = none).
    #[must_use]
    pub fn rls_setting(&self) -> String {
        let ids: Vec<String> = self.workspaces().iter().map(ToString::to_string).collect();
        format!("{{{}}}", ids.join(","))
    }
}

/// Who is asking. Stage 0 has anonymous principals only; sessions and keys
/// (WP-31) fill `user_id` and widen `scope`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub scope: Scope,
    pub rate: RateIdentity,
    pub user_id: Option<i64>,
}

impl Principal {
    #[must_use]
    pub fn anonymous(rate: RateIdentity) -> Self {
        Self {
            scope: Scope::Public,
            rate,
            user_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_scope_reads_no_workspace() {
        let p = Principal::anonymous(RateIdentity::Ip([0; 32]));
        assert_eq!(p.scope, Scope::Public);
        assert!(!p.scope.can_read_workspace(1));
        assert_eq!(p.scope.rls_setting(), "{}");
        let w = Scope::Workspace(vec![3, 7]);
        assert!(w.can_read_workspace(7) && !w.can_read_workspace(4));
        assert_eq!(w.rls_setting(), "{3,7}");
    }
}
