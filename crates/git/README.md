# git

Role: Git repository operations.

Owns:
- Repository discovery and summary data.
- Branch, remote, tag, stash, and changed-file domain models.
- Git actions such as stage, unstage, stash, branch checkout, and branch creation.
- Translation of `gix` and `git` command output into Kosmos data structures.

Does Not Own:
- Git tab rendering.
- Workspace or file tree state.
- Application-level error presentation.
