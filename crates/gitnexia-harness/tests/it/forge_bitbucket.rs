use gitnexia_core::api::Request;
use gitnexia_core::error::GbErrorKind;
use gitnexia_core::forge::*;
use gitnexia_core::redact::Secret;
use gitnexia_core::testing::TestRepo;
use gitnexia_forge::bitbucket::BitbucketProvider;
use gitnexia_forge::endpoints::HostEndpoints;
use gitnexia_harness::Harness;
use gitnexia_harness::fake_forge::FakeForge;
use gitnexia_harness::fake_forge::bitbucket::{BITBUCKET_HOST, BITBUCKET_TOKEN};
use serde_json::json;

fn provider(f: &FakeForge) -> BitbucketProvider {
    BitbucketProvider::new(
        BITBUCKET_HOST,
        &HostEndpoints {
            api: f.bitbucket_api(),
            web: f.bitbucket_web(),
            avatars: None,
        },
        Secret::new(BITBUCKET_TOKEN),
        Some(f.change_counter()),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn bitbucket_create_context_api_works_with_context_remote() {
    let h = Harness::for_tests().await;
    let add: Request = serde_json::from_value(json!({"method":"addForgeAccount","params":{"host":"https://bitbucket.example.com/context","kind":"bitbucket","token":BITBUCKET_TOKEN}})).unwrap();
    h.api.dispatch(add).await.unwrap();
    let repo = TestRepo::new();
    repo.commit("Initial");
    repo.git(&["checkout", "-b", "feature"]);
    repo.commit("Feature");
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://bitbucket.example.com/context/scm/TEAM/widget.git",
    ]);
    let open: Request =
        serde_json::from_value(json!({"method":"openRepo","params":{"path":repo.path()}})).unwrap();
    let id = h.api.dispatch(open).await.unwrap()["id"].as_u64().unwrap();
    let request: Request = serde_json::from_value(json!({"method":"forgeCreateContext","params":{"repo":id,"remote":"origin","sourceRemote":"origin","branch":"feature","target":"main"}})).unwrap();
    let context = h.api.dispatch(request).await.unwrap();
    assert_eq!(context["project"]["path"], "TEAM/widget");
    assert_eq!(context["sourceProject"], "TEAM/widget");
    assert_eq!(context["settings"]["mergeMethods"], json!([]));
    assert_eq!(context["firstCommit"]["summary"], "Feature");
    let create: Request = serde_json::from_value(json!({"method":"forgeCreateMr","params":{"repo":id,"remote":"origin","req":{"source":{"project":context["sourceProject"],"branch":"feature"},"targetBranch":"main","title":"Created through API","description":"","draft":false,"reviewers":[],"assignees":[],"labels":[],"squash":null,"deleteSourceBranch":null}}})).unwrap();
    assert_eq!(
        h.api.dispatch(create).await.unwrap()["mr"]["title"],
        "Created through API"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bitbucket_account_context_is_persisted_and_conflicts_are_rejected() {
    let h = Harness::for_tests().await;
    let req: Request = serde_json::from_value(json!({"method":"addForgeAccount","params":{"host":BITBUCKET_HOST,"kind":"bitbucket","token":"invalid-fictional-pat"}})).unwrap();
    assert!(
        h.api.dispatch(req).await.is_err(),
        "identity must be verified before storing an account"
    );
    let req: Request = serde_json::from_value(json!({"method":"forgeAccounts"})).unwrap();
    assert!(
        h.api
            .dispatch(req)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    let req: Request = serde_json::from_value(json!({"method":"addForgeAccount","params":{"host":"https://bitbucket.example.com/context","kind":"bitbucket","token":BITBUCKET_TOKEN}})).unwrap();
    let account = h.api.dispatch(req).await.unwrap();
    assert_eq!(account["account"]["host"], BITBUCKET_HOST);
    assert_eq!(
        account["account"]["baseUrl"],
        "https://bitbucket.example.com/context"
    );
    let store = h.api.store();
    let stale = store.active_profile();
    let mut stale = stale;
    stale.bitbucket_bases = None;
    stale.host_overrides.clear();
    store.save_profile(stale).unwrap();
    assert_eq!(
        store.active_profile().bitbucket_bases.unwrap()[BITBUCKET_HOST],
        "https://bitbucket.example.com/context"
    );
    let req: Request = serde_json::from_value(json!({"method":"addForgeAccount","params":{"host":BITBUCKET_HOST,"baseUrl":"https://bitbucket.example.com/other","kind":"bitbucket","token":BITBUCKET_TOKEN}})).unwrap();
    assert_eq!(
        h.api.dispatch(req).await.unwrap_err().kind,
        GbErrorKind::InvalidInput
    );
    let project = provider(&h.forge)
        .with_installation_base("https://bitbucket.example.com/context")
        .project("context/scm/TEAM/widget")
        .await
        .unwrap()
        .value;
    assert_eq!(project.path, "TEAM/widget");
    assert_eq!(
        project.web_url,
        "https://bitbucket.example.com/context/projects/TEAM/repos/widget"
    );
    let requests = serde_json::to_string(&h.forge.requests()).unwrap();
    assert!(!requests.contains(BITBUCKET_TOKEN) && !requests.contains("invalid-fictional-pat"));
}

#[tokio::test(flavor = "multi_thread")]
async fn bitbucket_account_repository_and_pr_lifecycle() {
    let f = FakeForge::start().await;
    let p = provider(&f);
    assert_eq!(p.check_token().await.unwrap().user.username, "reviewer");
    let project = p.project("TEAM/widget").await.unwrap().value;
    assert_eq!(project.default_branch.as_deref(), Some("main"));
    assert_eq!(p.search_repos("widget").await.unwrap().len(), 1);
    let list = p.open_mrs(&project, MrFilter::All).await.unwrap().value;
    assert_eq!(list[0].number, 12);
    assert_eq!(
        list[0].pipeline.as_ref().unwrap().status,
        PipelineStatus::Success
    );
    let pr = p.mr_detail(&project, 12).await.unwrap().value;
    assert_eq!(pr.description, "Details");
    let updated = p
        .edit(
            &project,
            12,
            &MrEdit {
                title: Some("Changed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.title, "Changed");
    let updated = p.retarget(&project, 12, "release").await.unwrap();
    assert_eq!(updated.target_branch, "release");
    assert_eq!(
        p.set_closed(&project, 12, true).await.unwrap().state,
        MrState::Closed
    );
    assert_eq!(
        p.set_closed(&project, 12, false).await.unwrap().state,
        MrState::Open
    );
    let updated = p
        .edit(
            &project,
            12,
            &MrEdit {
                reviewers: Some(PeopleEdit {
                    add: vec![7],
                    remove: vec![],
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.review.reviews.len(), 1);
    let updated = p
        .edit(
            &project,
            12,
            &MrEdit {
                reviewers: Some(PeopleEdit {
                    add: vec![],
                    remove: vec![7],
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(updated.review.reviews.is_empty());
    assert!(p.set_subscribed(&project, 12, true).await.unwrap());
    assert!(!p.set_subscribed(&project, 12, false).await.unwrap());
    let made = p
        .create_mr(
            &project,
            &CreateMr {
                source: SourceRef {
                    project: "TEAM/widget".into(),
                    branch: "feature".into(),
                },
                target_branch: "main".into(),
                title: "New".into(),
                description: "".into(),
                draft: false,
                reviewers: vec![],
                assignees: vec![],
                labels: vec![],
                squash: None,
                delete_source_branch: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(made.mr.title, "New");
    assert!(
        p.merge(&project, 12, &MergeOptions::default())
            .await
            .unwrap_err()
            .message
            .contains("browser")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bitbucket_comments_and_partial_local_review() {
    let f = FakeForge::start().await;
    let p = provider(&f);
    let project = p.project("TEAM/widget").await.unwrap().value;
    let note = p
        .reply(
            &project,
            12,
            &NewNote {
                discussion: None,
                body: "Hello".into(),
            },
        )
        .await
        .unwrap();
    let reference = NoteRef {
        discussion: note.id.clone(),
        note: note.id.clone(),
    };
    let reply = p
        .reply(
            &project,
            12,
            &NewNote {
                discussion: Some(note.id.clone()),
                body: "Reply".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(reply.body, "Reply");
    assert_eq!(
        p.edit_note(&project, 12, &reference, "Updated")
            .await
            .unwrap()
            .body,
        "Updated"
    );
    let discussions = p.discussions(&project, 12).await.unwrap().value;
    assert_eq!(discussions[0].notes[0].body, "Updated");
    assert_eq!(discussions[0].notes[1].body, "Reply");
    assert!(p.delete_note(&project, 12, &reference).await.is_err());
    let reply_ref = NoteRef {
        discussion: note.id.clone(),
        note: reply.id,
    };
    assert_eq!(
        p.edit_note(&project, 12, &reply_ref, "Updated reply")
            .await
            .unwrap()
            .body,
        "Updated reply"
    );
    p.delete_note(&project, 12, &reply_ref).await.unwrap();
    p.delete_note(&project, 12, &reference).await.unwrap();
    let disposable = p
        .reply(
            &project,
            12,
            &NewNote {
                discussion: None,
                body: "Temporary".into(),
            },
        )
        .await
        .unwrap();
    p.delete_note(
        &project,
        12,
        &NoteRef {
            discussion: disposable.id.clone(),
            note: disposable.id,
        },
    )
    .await
    .unwrap();
    let diff = p.review_diff(&project, 12).await.unwrap();
    let comment = NewReviewComment {
        anchor: ReviewAnchor {
            path: "README.md".into(),
            old_path: "README.md".into(),
            start: None,
            end: diff.files[0].lines[0],
        },
        body: "Consider this".into(),
        refs: diff.refs,
    };
    p.add_draft(&project, 12, &comment).await.unwrap();
    p.add_draft(&project, 12, &comment).await.unwrap();
    let mut seed = f.current_seed();
    seed.bitbucket.fail_comment_at = Some(seed.bitbucket.comment_attempts + 2);
    f.seed(seed);
    let result = p
        .submit_review(
            &project,
            12,
            &ReviewSubmit {
                event: ReviewEvent::Comment,
                body: String::new(),
            },
        )
        .await
        .unwrap();
    assert_eq!(result.published, 1);
    assert!(result.event_error.is_some());
    assert_eq!(p.review_drafts(&project, 12).await.unwrap().drafts.len(), 1);
    let result = p
        .submit_review(
            &project,
            12,
            &ReviewSubmit {
                event: ReviewEvent::Comment,
                body: String::new(),
            },
        )
        .await
        .unwrap();
    assert_eq!(result.published, 1);
    assert!(result.event_error.is_none());
    assert!(
        p.review_drafts(&project, 12)
            .await
            .unwrap()
            .drafts
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bitbucket_rejects_stale_pull_version() {
    let f = FakeForge::start().await;
    let first = provider(&f);
    let second = provider(&f);
    let project = first.project("TEAM/widget").await.unwrap().value;
    first.mr_detail(&project, 12).await.unwrap();
    second.mr_detail(&project, 12).await.unwrap();
    first
        .edit(
            &project,
            12,
            &MrEdit {
                title: Some("First".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    second.expire_fresh();
    assert_eq!(
        second.mr_detail(&project, 12).await.unwrap().value.mr.title,
        "First"
    );
    let error = second
        .edit(
            &project,
            12,
            &MrEdit {
                title: Some("Stale".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, GbErrorKind::InvalidInput);
    assert!(error.message.contains("stale"));
    first.expire_fresh();
    assert_eq!(
        first.mr_detail(&project, 12).await.unwrap().value.mr.title,
        "First"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bitbucket_comment_refresh_does_not_replace_pinned_version() {
    let f = FakeForge::start().await;
    let first = provider(&f);
    let second = provider(&f);
    let project = first.project("TEAM/widget").await.unwrap().value;
    let note = first
        .reply(
            &project,
            12,
            &NewNote {
                discussion: None,
                body: "Original".into(),
            },
        )
        .await
        .unwrap();
    let reference = NoteRef {
        discussion: note.id.clone(),
        note: note.id,
    };
    second.discussions(&project, 12).await.unwrap();
    first
        .edit_note(&project, 12, &reference, "Remote change")
        .await
        .unwrap();
    second.expire_fresh();
    assert_eq!(
        second.discussions(&project, 12).await.unwrap().value[0].notes[0].body,
        "Remote change"
    );
    let error = second
        .edit_note(&project, 12, &reference, "Stale edit")
        .await
        .unwrap_err();
    assert!(error.message.contains("stale"));
    first.expire_fresh();
    assert_eq!(
        first.discussions(&project, 12).await.unwrap().value[0].notes[0].body,
        "Remote change"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bitbucket_resolves_only_tasks() {
    let f = FakeForge::start().await;
    let mut seed = f.current_seed();
    seed.bitbucket.comments.push(json!({"id":88,"version":0,"pullRequestId":12,"text":"Task","severity":"BLOCKER","state":"OPEN","author":{"id":7,"name":"reviewer","displayName":"Reviewer"},"comments":[]}));
    f.seed(seed);
    let p = provider(&f);
    let project = p.project("TEAM/widget").await.unwrap().value;
    let threads = p.discussions(&project, 12).await.unwrap().value;
    assert!(threads[0].resolvable);
    assert!(p.resolve(&project, 12, "88", true).await.unwrap().resolved);
    assert!(!p.resolve(&project, 12, "88", false).await.unwrap().resolved);
    let ordinary = p
        .reply(
            &project,
            12,
            &NewNote {
                discussion: None,
                body: "Ordinary".into(),
            },
        )
        .await
        .unwrap();
    assert!(p.resolve(&project, 12, &ordinary.id, true).await.is_err());
}
