# Privacy

GitNexia has no accounts, servers or cloud service of its own. It talks only to your git remotes,
to the GitHub, GitLab, Forgejo, Bitbucket and Azure DevOps accounts you add, to Gravatar and GitHub's avatar host, to the
AI endpoint or local tool you configure for commit messages, and to GitHub for its own updates.
This page lists every network request the app makes, what each one sends, and how to turn it off.
It also lists what GitNexia keeps on your computer and how to delete it.

## Network requests

All requests come from the Rust core. In `crates/gitnexia-forge` they use the `ureq` HTTP client
(no cookies, `User-Agent: GitNexia/<version>`), and git remotes are reached by the `git` command
line. The UI makes no network requests of its own: every image it shows is handed over by the
core.

### Git remotes

**Background fetch.**

- When: while a repository's tab is shown, GitNexia runs
  `git fetch --all --prune --no-prune-tags --no-write-fetch-head`. It fetches once when the tab
  opens, if the last fetch is older than the interval, and then on every interval: every minute
  by default.
- After it, GitNexia runs `git ls-remote --tags <remote>` for each remote it fetched, to show which
  tags each remote has (git keeps no copy of a remote's tags). This contacts the same remotes in
  the same way, one more round trip each.
- It skips ticks while the window is minimized. Hidden tabs never fetch.
- It never prompts. A fetch that needs credentials is skipped.
- Sends: what `git fetch` sends to each of the repository's remotes, including any credentials
  that your git configuration, credential helper or ssh agent supplies.
- Matching GitLab HTTPS remotes also use the configured account's PAT through a temporary,
  repository-URL-scoped helper for clone and network operations, including background fetch.
  The selected account is used where one is known; no PAT is sent to another host or repository,
  saved in git configuration, or stored by another credential helper. Redirects are disabled for
  these authenticated URLs; SSH continues to use your normal SSH credentials.
- Matching Azure DevOps HTTPS remotes also use the organization's saved PAT through a temporary
  git credential helper. This applies to background fetch and user-started network operations;
  the repository's selected forge account wins, otherwise its organization's first account.
  No PAT is sent to another organization, a non-default port or an SSH remote. Redirects are
  disabled for these authenticated URLs; no credentials are saved in git configuration or by
  another helper.
- Turn off: Settings › Fetch and push › **Background fetch interval: Off**. "Prune deleted remote branches
  on fetch" chooses between `--prune` and `--no-prune`.

**Fetch, pull, push, clone and the rest.**

- When: only when you start one: Fetch, Pull, Push (including tags, deleting a remote branch and
  stack pushes), Clone, adding a remote, checking out an MR/PR from a fork, a stack retarget,
  Submodules: Update (`git submodule update --init --recursive`, which clones or fetches each
  submodule from the URL its `.gitmodules` names), adding a submodule (`git submodule add`, which
  clones the URL you enter), and in a repository that uses Git LFS, LFS:
  Pull, LFS: Fetch and a file's Download (`git lfs`, which asks the remote's LFS server for the
  files, with the same credentials).
- Sends: what the git command sends. A push uploads your commits. A Fetch or Pull is followed by
  `git ls-remote --tags` to each remote it fetched, as for the background fetch; it never asks
  for credentials, so without a credential helper or ssh agent the tags' remote state stays
  unknown.
- Turn off: don't start them.
- Reads never fetch from a remote. They run with `GIT_NO_LAZY_FETCH=1`, so a partial clone
  doesn't download missing objects while GitNexia reads it.
- Signature checks run your own `gpg` or `ssh-keygen` through git, with your configuration.
  GitNexia doesn't ask them to fetch keys, but a gpg set up to retrieve keys automatically will.

### Forge APIs (GitHub, GitLab, Forgejo, Bitbucket and Azure DevOps)

GitNexia only calls a forge API for an account you add in Settings › Accounts. Without one, it
makes no forge requests (apart from images you choose to load, below).

- **Where:**
  - GitHub: `https://api.github.com`.
  - GitLab: `https://<your host>/api/v4`.
  - Forgejo: `https://<your host>/api/v1`.
  - Bitbucket Data Center / Server: your configured HTTPS installation base, including its
    port and context path, under `/rest/api/1.0` and `/rest/build-status/1.0`. Bitbucket Cloud
    is not contacted by this integration.
  - Azure DevOps Services: `https://dev.azure.com/<your organization>`, for authenticated
    identity and accessible Git repositories. No organization enumeration, Azure DevOps Server,
    Microsoft Entra/OAuth endpoint or separate avatar service is contacted.
- **Sends:**
  - your token, in the `Authorization` header and only to that account's host;
    Azure PATs use Basic auth with an empty username, confined to the configured organization;
  - the project paths of your repositories' remotes on that host, and branch names, to find
    their MRs/PRs. With several accounts on a host, a repository's requests go with the account
    Settings › Repository › Forge account picks (the host's first otherwise); "Use SSH instead"
    on a credential prompt may ask with each of them in turn;
  - what you ask for: MR/PR reads, comments, approvals, edits, new MRs/PRs, merges, retargets,
    and the people or labels you search for.
  - Bitbucket merges open the PR's page in your system browser instead of sending a merge
    request with the token. This sends the PR URL to your browser, not the token; the browser's
    own Bitbucket login is independent of GitNexia.
  - Azure DevOps currently sends repository searches/lookups only, not PR or work-item writes.
    Searches filter accessible repositories locally by project/repository name. Adding an
    account validates identity and repository read access; opening a repository resolves its
    remote. Matching HTTPS git remotes also use the saved PAT, as described above; SSH remotes
    continue to use your existing SSH credentials.
- **When:**
  - for the shown tab's repository, when the tab opens and after GitNexia writes something to the
    forge;
  - when the window gets the focus back, at most once every 10 seconds;
  - on the background fetch timer, never more often than once a minute, and only while the
    window has the focus. The timer runs faster while a visible MR/PR's pipeline is running.
  - Conditional requests (ETags) make an unchanged poll cheap, and GitNexia follows the forges'
    rate-limit and poll-interval headers.
- **Turn off:**
  - Remove an account (Settings › Accounts) to stop its requests. A host with several accounts
    gets requests until its last one is removed.
  - Setting the background fetch interval to Off stops the timed polls. Polls when a tab opens or
    the window gets the focus still happen.

**Which forge a host runs.** When you enter a host in Settings › Accounts › Add account that its
name doesn't place (not github.com, gitlab.com, codeberg.org or a name with gitlab, forgejo,
gitea or bitbucket in it, or a Stash host label), that no Hosts setting covers, and whose forge you haven't picked, GitNexia asks that
host once which forge it runs: `GET https://<host>/api/v1/version` (Forgejo, Gitea), then, if it
answered but not as one, `GET https://<host>/api/v4/version` (GitLab). It asks when you leave the
Host field or click Create token or Add account, never while you type, so a half-typed name isn't
asked. No token, no cookie, and no redirect is followed; only the host you entered is asked.
For Bitbucket on an arbitrary hostname or below a context path, select Bitbucket explicitly;
GitNexia does not infer a provider from a generic authentication error.

### AI commit messages (optional)

Setting: Settings › AI › **Generate commit messages with AI** (off by default).

- **When:** only when you click the sparkles button beside the commit summary. Nothing is sent in
  the background, at startup or while you type.
- **Sends:** the staged diff (`git diff --cached`) and the last few commit subjects, to the
  OpenAI-compatible endpoint you entered, or to the local command you configured. With a local
  command, GitNexia starts that program in the repository and gives it the prompt as an argument;
  it never goes through a shell.
- **Token:** optional, kept in the system keyring (else an owner-only file) and sent only in the
  `Authorization: Bearer` header to that endpoint. It is never in the settings file, a log line or
  an error message.
- **Local endpoints** (`http://localhost:11434/v1` for Ollama, `http://127.0.0.1:1234/v1` for LM
  Studio, and the other common ports the Detect button checks) stay on this machine.
- **Turn it off:** switch it off in Settings › AI; the button disappears from the commit box.

### Update check (GitHub Releases)

Setting: Settings › General › **Check for updates automatically** (on by default).

- **When:** 10 seconds after startup, then once a day while GitNexia runs, and whenever you click
  Help › **Check for updates** (or the button in About), even with the setting off.
- **Sends:** `GET https://api.github.com/repos/artifactdev/gitnexia/releases?per_page=30`, with no
  token (not even a GitHub account's you added), no cookie and nothing about you or your
  repositories. GitHub sees your IP address and `User-Agent: GitNexia/<version>`. A repeated check
  is conditional (an ETag), so an unchanged list is a cheap 304.
- **Download:** only when you click the update. GitNexia downloads that release's package for
  your install (`.deb`, `.rpm`, Arch package, Windows setup `.exe` or `.msi`, macOS `.dmg`) and its
  `SHA256SUMS` from `github.com/artifactdev/gitnexia/releases/download/…`, following GitHub's
  redirect to its asset storage (`objects.githubusercontent.com` or
  `release-assets.githubusercontent.com`), over https and without a token. No other host is followed.
- **Include pre-releases** (off by default) only changes which releases are offered.
- **Turn off:** uncheck the setting. GitNexia then asks GitHub only when you click Check for
  updates.

### Avatars

Avatars are looked up in this order: the forge that hosts the repository, then Gravatar, then the
repository's own forge project again, then, for a GitHub noreply address with a user id, GitHub's
public avatar host. If none of these finds one, GitNexia shows the author's initials. Results are cached on disk under the SHA-256 hash of the email: an avatar
that was found for 7 days, a "no avatar" for 1 day.

**From your forge accounts.** Setting: Settings › Graph › Avatars › **From your forge
accounts** (on by default).

- **Which forge:** a commit author's email or name goes only to the forge that hosts the
  repository you are viewing: an account whose host is the host of one of the repository's
  remotes. Your other forge accounts are never asked. A repository with no remote on a host where
  you have an account gets no forge lookup; Gravatar and initials are used instead.
- **GitLab:** GitNexia calls `GET /avatar?email=<the author's email>` on that GitLab account. The
  email is sent in plain text, with your token.
- **GitHub:** GitHub's noreply addresses are looked up on `avatars.githubusercontent.com` by the
  user id or login in the address, without the token. For any other email, when the repository's
  forge project is on GitHub, GitNexia asks
  `GET /repos/<owner>/<repo>/commits?author=<email>&per_page=1`. That sends the email to GitHub
  with your token. GitNexia then fetches the picture from the avatar host without the token. It
  skips this lookup when the rate-limit budget runs low.
- **Forgejo:** its API can't look an email up, so no email is sent. A commit author whose email
  is the public email of a user GitNexia has already seen gets that user's picture, fetched from
  the Forgejo host without the token.
- **By name:** if nothing else found an avatar, GitNexia may look the author's name up on the
  repository's forge account. On GitLab that is a user search (`GET /users?search=<name>`). On
  GitHub and Forgejo it uses only people GitNexia has already seen, so no request is made.
- **Pictures the forge links:** project owners and MR/PR participants. These are fetched from
  that forge's own hosts, or from Gravatar when the forge links there and the Gravatar setting is
  on. No email is sent for them.
- **Turn off:** uncheck the setting. GitNexia then makes no avatar request to your forges.

**From Gravatar.** Setting: Settings › Graph › Avatars › **From Gravatar** (on by default).

- GitNexia requests `https://gravatar.com/avatar/<SHA-256 of the trimmed, lowercased
  email>?s=80&d=404`. The email itself is never sent, and GitHub noreply addresses are never
  looked up.
- A hash isn't anonymous. Anyone who already knows an email can compute the same hash, and
  Gravatar sees your IP address with each request.
- After a network error, GitNexia makes no Gravatar request for 3 minutes.
- **Turn off:** uncheck the setting. GitNexia then makes no request to Gravatar, including for
  Gravatar links a forge returns.

**GitHub noreply addresses, without an account.** Settings: both **From your forge accounts**
and **From Gravatar** (on by default).

- **When:** a commit author's email is `<id>+<login>@users.noreply.github.com` (GitHub's "Keep
  my email addresses private") and no step above found an avatar, even if you have no GitHub
  account in GitNexia.
- **What is sent:** GitNexia requests `https://avatars.githubusercontent.com/u/<id>?s=80&v=4`.
  Only the numeric user id goes out, in the URL path; never the email or the login, and never a
  token or cookie, even when you have a GitHub account. GitHub sees your IP address.
- The older `<login>@users.noreply.github.com` form has no id, so it gets no request here; only
  a GitHub account on the repository's forge looks it up (above).
- The answer is cached like a Gravatar one. After a network error, GitNexia makes no request to
  this host for 3 minutes.
- **Turn off:** uncheck either setting.

### Images and videos in rendered Markdown

These are the images in MR/PR descriptions and comments, and in Markdown files and diffs. A video
(an image link to an `.mp4`, `.m4v`, `.mov`, `.webm` or `.ogv` file, which GitLab shows as a
video) follows the same rules, up to 100 MB, and is never cached on disk.

- **The forge's own hosts:** when the repository has a forge account, the core fetches the image
  automatically.
  - GitLab: images on your GitLab host, fetched with your token. Uploads are read through its
    API, or, on a GitLab older than 17.4 (which has no uploads API), from the upload's own
    address on the same host.
  - Forgejo: none. Images on a Forgejo host load on a click, like any other host's.
  - GitHub: images on `github.com`, `user-images.githubusercontent.com`,
    `private-user-images.githubusercontent.com`, `raw.githubusercontent.com` and
    `avatars.githubusercontent.com`. These are fetched without the token, which only ever goes to
    `api.github.com`.
- **Any other host:** nothing is fetched until you click **Load image from `<host>`** (or **Load video from**). The core
  then fetches that image over https, without any token. The choice lasts for that image until
  GitNexia restarts.
- **No request:**
  - relative images in a Markdown file are read from the repository;
  - inline `data:` images (PNG, GIF, JPEG, WebP) are shown as they are.
- Found images are cached on disk (see below).
- **Open with default app**, offered for a video the app can't play: the core fetches the video
  again, saves it in `~/.cache/gitnexia/open/` and opens it with your default video player.

### Rendered HTML files

An HTML file in File View or Diff View shows with nothing fetched: its relative images and
stylesheets are read from the repository, and its scripts and anything on http(s) (a CDN library,
web fonts) don't load. A bar names those hosts; **Open in browser** opens the file in the browser
you pick, which then loads them.

### Links

Clicking a link (`http`, `https` or `mailto`) opens it in your default browser. GitNexia itself
makes no request for it.

### Open in browser

**Open in browser** on an HTML file (**Open in default app** on any other file) hands the file to
your system's default app for it. The browser then loads whatever the page refers to (images,
scripts, fonts, trackers) from wherever it points, outside GitNexia and its rules. GitNexia makes
no request for it.

### The embedded Chromium

**Experimental system-webview beta:** the following CEF-specific network restrictions and
measurements apply only to regular releases. Beta Actions packages use the system's WebView2
or WKWebView; their runtime/OS networking, spelling and update behavior has not been audited
to the same guarantees. The app CSP remains enabled and app-managed network requests still
go through Rust. The beta has a separate app identifier, settings/data/cache folders and
keyring service, imports no existing profile, and disables GitNexia's automatic updater.
Use disposable test repositories; review the `BETA.txt` shipped with the package.

The UI is GitNexia's own bundled files, served to the embedded Chromium through custom schemes; it
needs no network. Left alone, Chromium still contacts Google services by itself. In an idle run
of an unmodified build (two minutes, one repository open, background fetch Off, no forge
accounts) it fetched a spell-check dictionary (`redirector.gvt1.com`), checked for a Google
account (`accounts.google.com/ListAccounts`), asked whether "AI Mode" is available
(`www.google.com/async/folae`), queried the time (`clients2.google.com/time`) and downloaded a
Translate model (`www.gstatic.com`). GitNexia now turns all of that off
(`cef_runtime` in `crates/gitnexia-app/src/lib.rs`):

- Chromium resolves no host name except `localhost` (`--host-resolver-rules`), so whatever it
  would fetch by itself fails inside Chromium before any DNS query or connection. It still
  tries the account check and the "AI Mode" check at startup, since no switch or preference
  stops them, but nothing leaves the machine. GitNexia's own network
  traffic (git, forge APIs, avatars and Markdown images) runs in the Rust core, which this
  doesn't affect.
- Off as well:
  - Chromium's component updater, background networking and Safe Browsing
    (`--disable-component-update`, `--disable-background-networking`, `safebrowsing.enabled`);
  - network time queries and the Translate ranker (Chromium features
    `NetworkTimeServiceQuerying`, `TranslateRankerQuery`, `TranslateRankerEnforcement`);
  - preconnects and DNS prefetching (`net.network_prediction_options`);
  - the Chrome profile features that call Google services: the password manager and its leak
    detection, autofill, Translate, alternate error pages, search suggestions, and the Privacy
    Sandbox APIs (`vendor/tauri-runtime-cef/src/cef_impl/preferences.rs`).
- **Spell check works offline.** GitNexia ships Chromium's English (US) dictionary
  (`/usr/share/GitNexia/dictionaries/en-US-10-2.bdic`) and copies it into Chromium's profile at
  startup, so Chromium never downloads one. Only English (US) is checked
  (`spellcheck.dictionaries`). Chromium's "enhanced" spell check, which sends the text to
  Google's spelling service, is off (`spellcheck.use_spelling_service`) and removed from the
  right-click menu, along with "Language settings".
- **Measured:** the same idle run of a release build, with Chromium's network log on and
  GitNexia's sockets sampled every half second for two minutes, showed no DNS query and no
  connection from Chromium (apart from its IPv6 reachability probe, a UDP `connect()` that sends
  no packet). The only other request was the core's Gravatar lookup for the
  repository's commit author, which the Gravatar setting allows. With the bundled dictionary, a
  run of about two minutes that also typed misspelled words into the commit message (which
  Chromium underlined) showed no request for a dictionary in the network log, and no socket
  but the measurement's own local debugging port. If you see Chromium make a
  request, please open an issue.

### What isn't there

GitNexia has no telemetry, analytics or crash reporting. The update check (above) is the only
request GitNexia makes on its own that isn't about your repositories or accounts.

- The Rust code has no HTTP client besides the forge, Gravatar and update code above.
- The UI code makes no requests of its own. Its WebSocket client is used only by the test
  harness, outside the app.
- The packages ship no `crash_reporter.cfg`, the file CEF needs before it will upload crash
  reports.
- Crashes and errors go only to the local log files.
- The packages' AppStream metadata names a screenshot on `raw.githubusercontent.com`. Software
  centres (GNOME Software, Discover) may download it to show GitNexia's page; that's the software
  centre's request, not GitNexia's.

## Data on your computer

Paths are the XDG defaults. `$XDG_CONFIG_HOME`, `$XDG_CACHE_HOME` and `$XDG_DATA_HOME` move them.

| Where | What |
|---|---|
| `~/.config/gitnexia/settings.json` | App settings and the window's last position. |
| `~/.config/gitnexia/profiles/<id>/profile.json` | Each profile: open tabs and recent repositories (their paths), repository folders, editor choice, extra git config path, per-repository settings, and the forge accounts (host, user name, where the token is kept, never the token). |
| System keyring, items with `service` = `gitnexia` | Forge tokens, one per profile and host. |
| `~/.local/share/gitnexia/forge-tokens` | Forge tokens, only when no keyring was available. |
| `~/.local/share/gitnexia/forge-cache/` | The last MR/PR lists and project data per account, so a restart shows them at once. Removing an account deletes its host's folder. |
| `~/.local/share/gitnexia/journal/`, `rewrites/`, `tmp/` | The undo journal per worktree, the notes Push uses after a rewrite, and temporary index files. |
| `~/.local/share/gitnexia/remote-tags/` | Per repository, the tag names and commit ids each remote listed at the last fetch, so the graph's tag chips show where each tag is after a restart. |
| `~/.cache/gitnexia/logs/` | Log files: daily, the newest 7 kept, at most 50 MB in all, redacted. They contain repository paths, git commands and error messages. |
| `~/.cache/gitnexia/avatars/`, `forge-avatars/` | Avatar and Markdown image caches, named by SHA-256 hashes. |
| `~/.cache/gitnexia/updates/` | The downloaded update package, while it waits to be installed (0700). Each download replaces the last. |
| `~/.cache/gitnexia/open/` | Old file versions written for "Open in…", and videos opened with the default app. Copies older than a week are removed at startup. |
| `~/.cache/com.gitnexia.desktop/cef/` | Chromium's profile for the UI: its local storage (UI preferences, and unsent commit-message and MR/PR drafts), the words you add to the spell-check dictionary, a copy of the bundled dictionary (`Dictionaries/`) and `cef.log`. |
| `$XDG_RUNTIME_DIR/gitnexia-*` | The askpass and single-instance sockets. Removed when GitNexia exits. |
| Inside your repositories | Undo snapshots are stored as unreferenced commits in the repository's object store. `git gc` removes them in time. |

The font pickers (Settings › Appearance and Diffs and editors) read the names of the installed
fonts from the system's font folders. The list stays in memory for the session: it isn't written
anywhere or sent anywhere. Only the font you pick is saved, in `settings.json`.

GitNexia was called GitNex until 1.6.0, GitVolt before 1.5.1 (GitBolt before 0.7.0). On first
start GitNexia copies GitNex's and GitVolt's config, data and Chromium profile to its own folders,
and their keyring items (`gitnex`, `gitvolt`) to its service; the old folders and items stay as
they are, so a downgrade still finds them. GitBolt's aren't read: its folders (the same paths with
`gitbolt` and `dev.gitbolt.desktop`) and its keyring items (`gitbolt`) stay until you remove them.

### Deleting it all

1. Remove each forge account in Settings › Accounts, and revoke its token on the forge. GitNexia
   doesn't revoke tokens.
2. Quit GitNexia, then run:

   ```sh
   rm -rf ~/.config/gitnexia ~/.cache/gitnexia ~/.local/share/gitnexia ~/.cache/com.gitnexia.desktop
   secret-tool clear service gitnexia
   ```

   A move that failed also leaves `~/.config/gitnexia.not-moved-<time>`, and the same beside
   `~/.local/share/gitnexia`, `~/.cache/gitnexia` and `~/.cache/com.gitnexia.desktop` (the profile):
   remove those with `rm -rf` too.

   To remove what GitNex and GitVolt left behind as well (a downgrade would still find them):

   ```sh
   rm -rf ~/.config/gitnex ~/.cache/gitnex ~/.local/share/gitnex ~/.cache/com.gitnex.desktop
   rm -rf ~/.config/gitvolt ~/.cache/gitvolt ~/.local/share/gitvolt ~/.cache/com.gitvolt.desktop
   secret-tool clear service gitnex
   secret-tool clear service gitvolt
   ```

   `secret-tool` comes with libsecret, packaged as `libsecret-tools` on Debian and Ubuntu.

## The website

The website, [artifactdev.github.io/gitnexia](https://artifactdev.github.io/gitnexia/), is static
files on GitHub Pages, and its live demo runs in your browser. GitHub serves the files, so it sees
your IP address and browser as for any site it hosts (GitHub's
[privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement)).
Beyond that:

- **Nothing from other sites.** The pages, the demo and their stylesheets load everything from the
  site itself, fonts included: nothing from another host. The deploy checks this
  (`ui/scripts/check-site.mjs`), including each page's Content-Security-Policy, and stops if a
  page would.
- **No cookies, analytics or tracking.**
- **The demo stays in the tab.** Its repositories download with the page and live in memory.
  Nothing you do in it leaves the tab, and it's gone when you reload or reset the demo.
- **Browser storage.** Like the app, the demo's UI keeps preferences and unsent drafts in your
  browser's storage for the site. Clearing the site's data in your browser deletes them.
- **Downloads** link to the release's files on GitHub (github.com), which serves them.
