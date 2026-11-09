//! Isolated Bitbucket Data Center REST responses for provider tests.
use super::{FakeRequest, FakeToken, FakeUser, ForgeState, Reply};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const BITBUCKET_HOST: &str = "bitbucket.example.com";
pub const BITBUCKET_TOKEN: &str = "bitbucket-FAKE-e2e-token";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BitbucketSeed {
    pub tokens: Vec<FakeToken>,
    pub repositories: Vec<Value>,
    pub pulls: Vec<Value>,
    pub comments: Vec<Value>,
    pub statuses: Vec<Value>,
    pub next_comment: u64,
    pub comment_attempts: u32,
    pub fail_comment_at: Option<u32>,
}

fn account(user: &FakeUser) -> Value {
    json!({"id":user.id,"name":user.username,"displayName":user.name,"emailAddress":user.email,"type":"NORMAL","active":true})
}

pub fn default_seed(base: &str) -> BitbucketSeed {
    let user = FakeUser {
        id: 7,
        username: "reviewer".into(),
        name: "Reviewer".into(),
        email: Some("reviewer@example.com".into()),
        avatar_url: None,
    };
    let repo = json!({"id":42,"slug":"widget","name":"Widget","project":{"key":"TEAM"},"links":{"self":[{"href":format!("{base}/bitbucket/projects/TEAM/repos/widget/browse")}],"clone":[{"name":"http","href":format!("{base}/bitbucket/scm/TEAM/widget.git")},{"name":"ssh","href":"ssh://git@bitbucket.example.com:7999/TEAM/widget.git"}]}});
    let pull = json!({"id":12,"version":0,"title":"Improve widget","description":"Details","state":"OPEN","updatedDate":1700000000000i64,"author":{"user":account(&user)},"reviewers":[],"fromRef":{"id":"refs/heads/feature","displayId":"feature","latestCommit":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","repository":repo},"toRef":{"id":"refs/heads/main","displayId":"main","latestCommit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","repository":repo},"links":{"self":[{"href":format!("{base}/bitbucket/projects/TEAM/repos/widget/pull-requests/12")}] }});
    BitbucketSeed {
        tokens: vec![FakeToken {
            token: BITBUCKET_TOKEN.into(),
            user,
            scopes: vec![],
            fine_grained: false,
        }],
        repositories: vec![repo],
        pulls: vec![pull],
        comments: vec![],
        statuses: vec![json!({"state":"SUCCESSFUL","url":"https://ci.example.com/build/1"})],
        next_comment: 10,
        comment_attempts: 0,
        fail_comment_at: None,
    }
}

fn page(items: Vec<Value>, r: &FakeRequest) -> Reply {
    let start = r
        .query
        .get("start")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = r
        .query
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(25)
        .clamp(1, 100);
    let values: Vec<Value> = items.iter().skip(start).take(limit).cloned().collect();
    let next = start.saturating_add(values.len());
    Reply::json(
        json!({"values":values,"start":start,"limit":limit,"size":values.len(),"isLastPage":next >= items.len(),"nextPageStart":next}),
    )
}

fn comment_mut(comments: &mut [Value], id: u64) -> Option<&mut Value> {
    for comment in comments {
        if comment["id"] == id {
            return Some(comment);
        }
        if let Some(children) = comment["comments"].as_array_mut()
            && let Some(found) = comment_mut(children, id)
        {
            return Some(found);
        }
    }
    None
}

fn remove_comment(comments: &mut Vec<Value>, id: u64) -> bool {
    if let Some(index) = comments.iter().position(|comment| comment["id"] == id) {
        comments.remove(index);
        return true;
    }
    comments.iter_mut().any(|comment| {
        comment["comments"]
            .as_array_mut()
            .is_some_and(|children| remove_comment(children, id))
    })
}

pub fn route(st: &mut ForgeState, r: &FakeRequest) -> Reply {
    let seed = &mut st.seed.bitbucket;
    if r.path == "/rest/api/1.0/application-properties" && r.method == "GET" {
        let reply = Reply::json(json!({"version":"7.21.0","displayName":"Bitbucket"}));
        return match &r.token {
            Some(t) => reply
                .header("X-AUSERNAME", &t.user.username)
                .header("X-AUSERID", &t.user.id.to_string()),
            None => reply,
        };
    }
    let Some(token) = &r.token else {
        return Reply::status(
            401,
            json!({"errors":[{"message":"Authentication required"}]}),
        );
    };
    let parts: Vec<&str> = r.segments.iter().map(String::as_str).collect();
    if parts == ["rest", "api", "1.0", "users", &token.user.username] && r.method == "GET" {
        return Reply::json(account(&token.user));
    }
    if parts == ["rest", "api", "1.0", "users"] && r.method == "GET" {
        return page(vec![account(&token.user)], r);
    }
    if parts == ["rest", "api", "1.0", "repos"] && r.method == "GET" {
        return page(seed.repositories.clone(), r);
    }
    if parts.starts_with(&["rest", "build-status", "1.0", "commits"]) && r.method == "GET" {
        return page(seed.statuses.clone(), r);
    }
    if parts.len() < 7 || parts[..4] != ["rest", "api", "1.0", "projects"] || parts[5] != "repos" {
        return Reply::status(404, json!({}));
    }
    let repo = seed
        .repositories
        .iter()
        .find(|p| p["project"]["key"] == parts[4] && p["slug"] == parts[6])
        .cloned();
    let Some(repo) = repo else {
        return Reply::status(404, json!({}));
    };
    let tail = &parts[7..];
    if tail.is_empty() && r.method == "GET" {
        return Reply::json(repo);
    }
    if tail == ["branches", "default"] && r.method == "GET" {
        return Reply::json(json!({"displayId":"main"}));
    }
    if tail == ["forks"] && r.method == "GET" {
        return page(vec![], r);
    }
    if tail == ["pull-requests"] {
        if r.method == "GET" {
            return page(
                seed.pulls
                    .iter()
                    .filter(|v| v["state"] == "OPEN")
                    .cloned()
                    .collect(),
                r,
            );
        }
        if r.method == "POST" {
            let Ok(body) = serde_json::from_slice::<Value>(r.body) else {
                return Reply::status(400, json!({}));
            };
            let id = seed
                .pulls
                .iter()
                .filter_map(|v| v["id"].as_u64())
                .max()
                .unwrap_or(0)
                + 1;
            let mut pull = body.clone();
            pull["id"] = json!(id);
            pull["version"] = json!(0);
            pull["state"] = json!("OPEN");
            pull["author"] = json!({"user":account(&token.user)});
            pull["fromRef"]["repository"] = repo.clone();
            pull["toRef"]["repository"] = repo.clone();
            pull["fromRef"]["latestCommit"] = json!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
            pull["toRef"]["latestCommit"] = json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
            seed.pulls.push(pull.clone());
            return Reply::json(pull);
        }
    }
    if tail.len() < 2 || tail[0] != "pull-requests" {
        return Reply::status(404, json!({}));
    }
    let Some(id) = tail[1].parse::<u64>().ok() else {
        return Reply::status(404, json!({}));
    };
    let Some(index) = seed.pulls.iter().position(|v| v["id"].as_u64() == Some(id)) else {
        return Reply::status(404, json!({}));
    };
    let path = &tail[2..];
    if path.is_empty() && r.method == "GET" {
        return Reply::json(seed.pulls[index].clone());
    }
    if path.is_empty() && r.method == "PUT" {
        let Ok(body) = serde_json::from_slice::<Value>(r.body) else {
            return Reply::status(400, json!({}));
        };
        let pull = &mut seed.pulls[index];
        if body["version"] != pull["version"] {
            return Reply::status(
                409,
                json!({"errors":[{"message":"Pull request version is stale"}]}),
            );
        }
        for key in ["title", "description"] {
            if !body[key].is_null() {
                pull[key] = body[key].clone();
            }
        }
        if let Some(reviewers) = body["reviewers"].as_array() {
            pull["reviewers"] = json!(
                reviewers
                    .iter()
                    .map(|r| {
                        if r["user"]["name"] == token.user.username {
                            json!({"user":account(&token.user),"status":"UNAPPROVED"})
                        } else {
                            r.clone()
                        }
                    })
                    .collect::<Vec<_>>()
            );
        }
        if let Some(target) = body["toRef"]["id"].as_str() {
            pull["toRef"]["id"] = json!(target);
            pull["toRef"]["displayId"] = json!(target.trim_start_matches("refs/heads/"));
        }
        pull["version"] = json!(pull["version"].as_u64().unwrap_or(0) + 1);
        return Reply::json(pull.clone());
    }
    if matches!(path, ["decline"] | ["reopen"]) && r.method == "POST" {
        let pull = &mut seed.pulls[index];
        if r.query.get("version").and_then(|v| v.parse::<u64>().ok()) != pull["version"].as_u64() {
            return Reply::status(
                409,
                json!({"errors":[{"message":"Pull request version is stale"}]}),
            );
        }
        pull["state"] = json!(if path[0] == "decline" {
            "DECLINED"
        } else {
            "OPEN"
        });
        pull["version"] = json!(pull["version"].as_u64().unwrap_or(0) + 1);
        return Reply::json(pull.clone());
    }
    if path == ["diff"] && r.method == "GET" {
        return Reply::json(
            json!({"fromHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","toHash":seed.pulls[index]["fromRef"]["latestCommit"],"truncated":false,"diffs":[{"source":{"toString":"README.md"},"destination":{"toString":"README.md"},"truncated":false,"hunks":[{"truncated":false,"segments":[{"type":"ADDED","truncated":false,"lines":[{"source":1,"destination":1,"line":"hello","truncated":false}]}]}]}]}),
        );
    }
    if path == ["activities"] && r.method == "GET" {
        let activities = seed.comments.iter().filter(|c| c["pullRequestId"] == id && c["parent"].is_null()).map(|c| json!({"action":"COMMENTED","commentAction":"ADDED","comment":c,"commentAnchor":c["anchor"]})).collect();
        return page(activities, r);
    }
    if path == ["comments"] && r.method == "POST" {
        seed.comment_attempts += 1;
        if seed.fail_comment_at == Some(seed.comment_attempts) {
            return Reply::status(403, json!({"errors":[{"message":"refused"}]}));
        }
        let Ok(body) = serde_json::from_slice::<Value>(r.body) else {
            return Reply::status(400, json!({}));
        };
        let comment = json!({"id":seed.next_comment,"version":0,"text":body["text"],"author":account(&token.user),"createdDate":1700000000000i64,"comments":[],"anchor":body["anchor"],"parent":body["parent"],"pullRequestId":id});
        seed.next_comment += 1;
        if let Some(parent) = body["parent"]["id"].as_u64() {
            let Some(p) = comment_mut(&mut seed.comments, parent) else {
                return Reply::status(404, json!({}));
            };
            p["comments"]
                .as_array_mut()
                .expect("comments")
                .push(comment.clone());
        } else {
            seed.comments.push(comment.clone());
        }
        return Reply::json(comment);
    }
    if path.len() == 2 && path[0] == "comments" {
        let Some(comment_id) = path[1].parse::<u64>().ok() else {
            return Reply::status(404, json!({}));
        };
        let Some(comment) = comment_mut(&mut seed.comments, comment_id) else {
            return Reply::status(404, json!({}));
        };
        let current = comment["version"].as_u64().unwrap_or(0);
        if r.method == "DELETE" {
            if r.query.get("version").and_then(|v| v.parse::<u64>().ok()) != Some(current) {
                return Reply::status(
                    409,
                    json!({"errors":[{"message":"Comment version is stale"}]}),
                );
            }
            if comment["comments"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
            {
                return Reply::status(409, json!({"errors":[{"message":"Comment has replies"}]}));
            }
            remove_comment(&mut seed.comments, comment_id);
            return Reply::no_content();
        }
        if r.method == "PUT" {
            let Ok(body) = serde_json::from_slice::<Value>(r.body) else {
                return Reply::status(400, json!({}));
            };
            if body["version"].as_u64() != Some(current) {
                return Reply::status(
                    409,
                    json!({"errors":[{"message":"Comment version is stale"}]}),
                );
            }
            if !body["text"].is_null() {
                comment["text"] = body["text"].clone();
            }
            if !body["state"].is_null() {
                comment["state"] = body["state"].clone();
            }
            comment["version"] = json!(current + 1);
            return Reply::json(comment.clone());
        }
    }
    if (matches!(path, ["approve"] | ["review"] | ["watch"])
        || path.first() == Some(&"participants"))
        && matches!(r.method, "POST" | "PUT" | "DELETE")
    {
        return Reply::json(json!({}));
    }
    Reply::status(404, json!({}))
}
