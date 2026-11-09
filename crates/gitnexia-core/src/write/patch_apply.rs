//! `git apply`: a patch from a file (or the clipboard) into the working tree. Not journaled;
//! `git apply` refuses a patch that doesn't fit rather than leaving conflict markers.

use crate::error::{GbError, GbErrorKind};
use crate::events::{ChangeKind, OpKind};
use crate::write::{Plan, Pre, Staging, WriteCx, WriteIntent};

pub(crate) struct ApplyPatch {
    pub patch: String,
}

impl WriteIntent for ApplyPatch {
    type Outcome = ();

    fn kind(&self) -> OpKind {
        OpKind::Stage
    }

    fn label(&self) -> String {
        "apply patch".into()
    }

    fn undo(&self) -> Option<crate::journal::UndoKind> {
        None
    }

    fn staging(&self) -> Staging {
        Staging::Keep
    }

    async fn plan(&self, _pre: &Pre<'_>) -> Result<Plan, GbError> {
        if self.patch.trim().is_empty() {
            return Err(GbError::new(
                GbErrorKind::InvalidInput,
                "The patch is empty",
            ));
        }
        Ok(Plan::default())
    }

    async fn run(&self, cx: &mut WriteCx<'_>) -> Result<(), GbError> {
        let inv = cx
            .git(["apply", "--whitespace=nowarn", "--binary", "-"])
            .stdin(self.patch.clone().into_bytes());
        cx.run_git(inv).await?;
        cx.touch(ChangeKind::Worktree);
        Ok(())
    }
}
