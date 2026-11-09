# Bitbucket Data Center / Server

GitNexia supports self-hosted Bitbucket Data Center / Server, not Bitbucket Cloud
(`bitbucket.org`). Cloud has a different API and authentication model.

## Connect

1. In Bitbucket, open **Manage account > HTTP access tokens** and create a **user**
   personal access token. Give it repository read permission for browsing and the write
   permissions required for the PR actions you intend to use. The token cannot grant access
   its user does not already have.
2. In GitNexia, open **Settings > Accounts > Add account** and choose **Bitbucket**.
3. Enter the installation's HTTPS address, including any custom port and context path,
   for example `https://code.example.com:8443/bitbucket`. Do not enter a clone URL or a REST
   endpoint.
4. Paste the token and add the account. GitNexia keeps it in the system keyring, with the
   existing explicit warning if only file storage is available.

Use user personal access tokens, also called user HTTP access tokens. OAuth, app passwords,
and project/repository service tokens are not substitutes for a user account in this
integration. Tokens are not copied into git configuration or clone URLs: git fetch/push
continues using your normal terminal credentials.

Custom HTTPS ports and installation context paths are preserved separately from project
identities. An HTTPS clone such as
`https://code.example.com:8443/bitbucket/scm/ORBIT/widget.git` identifies `ORBIT/widget`.
Personal repositories use `~username/repository`; their browser links use
`/users/username/repos/repository`, not `/projects/~username/repos/repository`.
An SSH remote does not describe the HTTPS installation's port or context path; configure the
account's HTTPS address rather than inferring it from the SSH port.
One installation context is supported per HTTPS authority in a profile. Accounts on that
authority share its installation base; a conflicting context is rejected rather than silently
routing a token to a different installation.

## Supported workflows and limits

- Repository discovery, forks, PR lists/details and branch badges use the Bitbucket REST API.
- Create, edit, retarget, close/reopen, comments and reviews stay in the normal GitNexia PR UI.
- Build/check status means statuses already reported to Bitbucket. GitNexia does not connect to,
  launch or retry Jenkins/Bamboo jobs.
- PR creation supports reviewers. This integration does not offer native draft PRs, assignees,
  labels or reactions. A saved create form containing unsupported choices must be explicitly
  cleared before submission; its title and description are preserved.
- Pending inline review drafts are local to the running session. Submit them before restarting
  GitNexia; they are not server-side pending reviews or durable saved drafts.
- Merging opens the PR in your system browser. This avoids relying on merge operations
  restricted by HTTP token authentication. GitNexia never bypasses merge checks with a local
  merge and push. Auto-merge and branch-update controls are not offered for this integration.
- `#12` means PR #12, not an issue. Bitbucket Data Center / Server has no built-in issue route;
  Jira integration is not included.
- Resolve/unresolve applies to Bitbucket task comments (severity `BLOCKER`), not ordinary
  discussion comments. Watch/unwatch uses the token user's PR notifications.

## Compatibility and errors

Contract tests use a local fake Bitbucket, not a live company server. They do not establish that
every Bitbucket version, plugin or reverse-proxy configuration has been tested. Check your
server's permissions and API availability if an operation is refused; GitNexia shows the error
instead of reporting an unsupported write as successful.
The portable API contract is based on the Bitbucket Server 7.21 REST documentation, not the
unrelated Cloud API. This is a documented contract baseline, not a claim that a live 7.21 server
was tested or that every earlier release is supported.

Bitbucket uses versions to protect PR and comment edits against concurrent changes. A
conflict requires refreshing and reviewing the new state, not blindly repeating the write
against a newer version.
GitNexia pins mutation versions so background polls cannot silently overwrite a concurrent edit.
After a conflict, reconnect the account to obtain a new writable version, then reopen the PR
and review its current contents before editing again.

### Endpoint and capability matrix

Paths below are relative to the configured HTTPS installation base. All writes still require
the user's repository/PR permissions; token validation does not certify write access.

| Workflow | API path under `/rest/api/1.0` unless noted | Behavior |
|---|---|---|
| Identity/version | `/application-properties`, `/users?filter=…` | Requires an authenticated username header and an exact normal user record; validates the authenticated ID header when supplied |
| Repository/default branch | `/projects/{key}/repos/{slug}`, `/branches/default` | Read |
| Discovery/forks | `/repos?name=…`, repository `/forks` | Read, paginated |
| PRs | repository `/pull-requests`, `/{id}` | Read/create/versioned edit and retarget |
| Close/reopen | PR `/decline`, `/reopen` | Versioned write |
| Reviewers/review state | PR edit and participant endpoints | Reviewer updates, approve/withdraw/request changes |
| Activity/comments | PR `/activities`, `/comments` | Threaded reads, replies, versioned edit/delete |
| Task resolution | PR `/comments/{id}` | Versioned task state only |
| Notifications | PR `/watch` | Watch/unwatch |
| Inline reviews | PR `/diff` and `/comments` | Local drafts, incremental submission; successful comments are not resent |
| Build statuses | `/rest/build-status/1.0/commits/{sha}` | Read statuses already reported to Bitbucket |
| Merge | PR web page | Browser handoff; no API merge or local git workaround |

Reference: [Bitbucket Server 7.21 REST](https://docs.atlassian.com/bitbucket-server/rest/7.21.0/bitbucket-rest.html),
[build-status REST](https://docs.atlassian.com/bitbucket-server/rest/7.21.0/bitbucket-build-rest.html)
and [HTTP access tokens](https://confluence.atlassian.com/bitbucketserver/http-access-tokens-939515499.html).

Removing the account deletes its locally stored token but does not revoke it in Bitbucket.
Revoke the token in Bitbucket as well when it is no longer needed. See [Security](../SECURITY.md)
and [Privacy](../PRIVACY.md) for storage and network guarantees.
