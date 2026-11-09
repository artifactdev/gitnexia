# Azure DevOps Services

GitNexia connects to Azure DevOps Services organizations at `dev.azure.com` using personal
access tokens (PATs). Azure DevOps Server and Microsoft Entra sign-in are not included.

## Connect and clone

1. In your Azure DevOps organization, open **User settings > Personal access tokens**.
   Create a token scoped to that organization with **Code (Read)** permission for browsing.
   PR creation, comments and merges need **Code (Read & write)**. The token cannot grant
   repository access your user does not already have.
2. In GitNexia, open **Settings > Accounts > Add account** and select **Azure DevOps**.
3. Enter your organization URL, for example `https://dev.azure.com/orbital`, not a project
   URL, clone URL or API endpoint.
4. Paste the PAT and add the account. It is kept in the existing system keyring/token store,
   with the normal warning if only file storage is available.
5. Open **Clone** and search by project or repository name. Choose a repository and a
   destination folder to clone it through the normal git CLI workflow.

Add each organization separately. Accounts and tokens are isolated by organization; multiple
users in the same organization can also have separate accounts. An expired, rejected or
insufficiently scoped PAT produces an error rather than a successful connection.

For matching HTTPS remotes, GitNexia also uses the account's PAT for clone, fetch, pull and push,
including background fetch. Existing repositories use the account selected in **Settings >
Repository > Forge account**, or the organization's first account otherwise. Push requires
**Code (Read & write)** permission.

This uses a temporary, repository-URL-scoped credential helper for the running git command.
The PAT stays in the token store: it is not placed in command arguments, environment variables,
clone URLs or saved git configuration, or saved by another credential helper. Redirects are
disabled for these authenticated HTTPS remotes. Other remotes still use your configured
credentials. SSH remotes use your normal SSH keys or agent, not the PAT.

## Existing repositories

GitNexia recognizes current HTTPS and SSH remotes:

- `https://dev.azure.com/orbital/Space%20Tools/_git/Orbit`
- `git@ssh.dev.azure.com:v3/orbital/Space%20Tools/Orbit`

Legacy `orbital.visualstudio.com` HTTPS remotes (including `DefaultCollection`) and
`vs-ssh.visualstudio.com:v3/...` SSH remotes resolve to the same organization and repository.
New clone and browser URLs use `dev.azure.com`; existing git remote URLs are not rewritten.
The account key is `dev.azure.com/orbital`, including the organization, rather than the shared
`dev.azure.com` authority. Repository and user GUIDs are preserved, not shortened to numeric IDs.

## Supported workflows and limits

- Search accessible Git repositories across the organization's projects.
- Clone repositories, recognize existing remotes, and open repository, branch, commit and file
  pages in the system browser.
- List and view pull requests, including their descriptions, existing reviewers and merge status.
  **Compare** shows the pull request's net changes from its common base to its current head;
  inline comments can be posted on lines in that diff.
- Create ready or draft pull requests from a branch in the same repository, and add discussion
  comments or replies. Inline comments post immediately; composing a batch of draft review comments
  and submitting them together is not supported.
- Merge a pull request using the chosen merge strategy. GitNexia sends the source commit it read
  with the merge request so Azure DevOps can refuse a stale merge.
- PR reviewer votes already on Azure DevOps are displayed, but requesting reviewers, voting,
  approvals and changes-requested actions are not available in GitNexia.
- Pipelines/checks, cross-repository PR creation, work-item integration, and Azure DevOps Server
  are not included.
- Disabled repositories are excluded from discovery. TFVC is not a Git repository and is not
  included.
- No organization enumeration or third-party OAuth access is required: you enter the organization.
- Contract tests use a local fake HTTP server, not a live Azure DevOps organization.

The API contract follows Azure DevOps REST 7.1: [repository discovery](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/repositories/list?view=azure-devops-rest-7.1),
[pull request creation](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/create?view=azure-devops-rest-7.1),
[pull request iterations](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-iterations/list?view=azure-devops-rest-7.1),
[iteration changes](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-iteration-changes/get?view=azure-devops-rest-7.1),
[repository items](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/items/get?view=azure-devops-rest-7.1),
[pull request updates and merge](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-requests/update?view=azure-devops-rest-7.1),
[pull request thread creation](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-threads/create?view=azure-devops-rest-7.1),
and [pull request threads](https://learn.microsoft.com/en-us/rest/api/azure/devops/git/pull-request-threads/list?view=azure-devops-rest-7.1).
Removing an account deletes its locally stored token, but does not revoke it in Azure DevOps.
Revoke unused PATs there too. See [Security](../SECURITY.md) and [Privacy](../PRIVACY.md).
