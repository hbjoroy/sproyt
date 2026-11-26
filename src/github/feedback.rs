use super::*;
use reqwest::Method;

const INITIAL_STATUS: &str = "Backlog";

#[derive(serde::Serialize)]
pub(crate) struct Candidate {
    pub number: i64,
    pub title: String,
    pub body: String,
    pub eligible: bool,
}

pub(crate) struct FeedbackStatus {
    pub issue: Issue,
    pub state: String,
    pub status_name: Option<String>,
    pub reason: Option<String>,
    pub proposed: bool,
    pub eligible: bool,
}

fn status(value: &Value, target: &Destination) -> Result<FeedbackStatus, &'static str> {
    let number = value["number"]
        .as_i64()
        .filter(|n| *n > 0)
        .ok_or("missing issue number")?;
    let id = value["id"]
        .as_i64()
        .filter(|n| *n > 0)
        .ok_or("missing issue id")?;
    let url = format!("https://github.com/{}/issues/{number}", target.repository);
    if value["html_url"] != url || !value["pull_request"].is_null() {
        return Err("issue destination mismatch");
    }
    let state = value["state"]
        .as_str()
        .filter(|s| matches!(*s, "closed" | "open"))
        .ok_or("missing state")?;
    let category = value["status"]["category"].as_str();
    let proposed = state == "open" && category == Some("backlog");
    let eligible = state == "open" && matches!(category, Some("backlog" | "planned"));
    Ok(FeedbackStatus {
        issue: Issue { id, number, url },
        state: state.to_owned(),
        status_name: value["status"]["name"].as_str().map(str::to_owned),
        reason: value["state_reason"].as_str().map(str::to_owned),
        proposed,
        eligible,
    })
}

impl GitHub {
    pub(crate) async fn feedback_status(
        &self,
        target: &Destination,
        token: &str,
        number: i64,
    ) -> Result<FeedbackStatus, &'static str> {
        let value = Self::response(self.request(
            Method::GET,
            &format!("/repos/{}/issues/{number}", target.repository),
            token,
        ))
        .await?;
        let result = status(&value, target)?;
        if result.issue.number != number {
            return Err("issue number mismatch");
        }
        Ok(result)
    }

    pub(crate) async fn feedback_candidate(
        &self,
        target: &Destination,
        token: &str,
        number: i64,
    ) -> Result<Option<Candidate>, &'static str> {
        let response = self
            .request(
                Method::GET,
                &format!("/repos/{}/issues/{number}", target.repository),
                token,
            )
            .send()
            .await
            .map_err(|_| "GitHub transport unavailable")?;
        if matches!(response.status().as_u16(), 404 | 410) {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err("GitHub rejected the request");
        }
        let value = Self::decode_response(response).await?;
        if !value["pull_request"].is_null() {
            return Ok(None);
        }
        let checked = status(&value, target)?;
        if checked.issue.number != number {
            return Err("issue number mismatch");
        }
        let title = value["title"].as_str().ok_or("missing title")?;
        let body = value["body"].as_str().unwrap_or("");
        if title.len() + body.len() > 65000 {
            return Ok(None);
        }
        Ok(Some(Candidate {
            number,
            title: title.to_owned(),
            body: body.to_owned(),
            eligible: checked.eligible,
        }))
    }

    // None signals that a complete comparison exceeds the model's budget.
    pub(crate) async fn feedback_candidates(
        &self,
        target: &Destination,
        token: &str,
    ) -> Result<Option<Vec<Candidate>>, &'static str> {
        let mut result = Vec::new();
        let mut bytes = 0;
        for page in 1..=4 {
            let value = Self::response(
                self.request(
                    Method::GET,
                    &format!("/repos/{}/issues", target.repository),
                    token,
                )
                .query(&[
                    ("state", "open"),
                    ("per_page", "50"),
                    ("page", &page.to_string()),
                ]),
            )
            .await?;
            let rows = value.as_array().ok_or("invalid issue list")?;
            for row in rows.iter().filter(|row| row["pull_request"].is_null()) {
                let issue = status(row, target)?;
                let title = row["title"].as_str().ok_or("missing title")?.to_owned();
                let body = row["body"].as_str().unwrap_or("").to_owned();
                bytes += title.len() + body.len();
                if result.len() >= 40 || bytes > 65000 {
                    return Ok(None);
                }
                result.push(Candidate {
                    number: issue.issue.number,
                    eligible: issue.eligible,
                    title,
                    body,
                });
            }
            if rows.len() < 50 {
                return Ok(Some(result));
            }
        }
        Ok(None)
    }

    pub(crate) async fn feedback_comments(
        &self,
        target: &Destination,
        token: &str,
        number: i64,
    ) -> Result<Option<Vec<String>>, &'static str> {
        let rows = self.comment_page(target, token, number, 1).await?;
        let bodies = rows
            .iter()
            .map(|v| {
                v["body"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or("missing comment")
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(
            (bodies.len() < 50 && bodies.iter().map(String::len).sum::<usize>() <= 32000)
                .then_some(bodies),
        )
    }

    async fn comment_page(
        &self,
        target: &Destination,
        token: &str,
        number: i64,
        page: i32,
    ) -> Result<Vec<Value>, &'static str> {
        let response = Self::response(
            self.request(
                Method::GET,
                &format!("/repos/{}/issues/{number}/comments", target.repository),
                token,
            )
            .query(&[("per_page", 50), ("page", page)]),
        )
        .await?;
        response.as_array().cloned().ok_or("invalid comments list")
    }

    pub(crate) async fn feedback_write(
        &self,
        target: &Destination,
        token: &str,
        title: Option<&str>,
        number: Option<i64>,
        body: &str,
    ) -> Result<Issue, CreateError> {
        let base = format!("/repos/{}/issues", target.repository);
        let (path, payload, existing) = match (title, number) {
            (Some(title), None) => (
                base,
                json!({"title":title,"body":body,"status":INITIAL_STATUS}),
                None,
            ),
            (None, Some(number)) => {
                let current = self
                    .feedback_status(target, token, number)
                    .await
                    .map_err(|_| CreateError::Rejected)?;
                if !current.eligible {
                    return Err(CreateError::Rejected);
                }
                (
                    format!("{base}/{number}/comments"),
                    json!({"body":body}),
                    Some(current.issue),
                )
            }
            _ => return Err(CreateError::Rejected),
        };
        let response = self
            .request(Method::POST, &path, token)
            .json(&payload)
            .send()
            .await
            .map_err(|_| CreateError::Uncertain)?;
        if matches!(
            response.status().as_u16(),
            400 | 401 | 403 | 404 | 410 | 415 | 422 | 429
        ) {
            return Err(CreateError::Rejected);
        }
        if response.status() != reqwest::StatusCode::CREATED {
            return Err(CreateError::Uncertain);
        }
        let value = Self::decode_response(response)
            .await
            .map_err(|_| CreateError::Uncertain)?;
        if let Some(issue) = existing {
            if !checked_comment(&value, &issue, target, body) {
                return Err(CreateError::Uncertain);
            }
            Ok(issue)
        } else {
            if !status(&value, target)
                .map_err(|_| CreateError::Uncertain)?
                .proposed
            {
                return Err(CreateError::Uncertain);
            }
            checked_issue(&value, target, title.ok_or(CreateError::Uncertain)?, body)
                .map_err(|_| CreateError::Uncertain)
        }
    }

    pub(crate) async fn find_feedback_comment(
        &self,
        target: &Destination,
        token: &str,
        number: i64,
        marker: &str,
        body: &str,
    ) -> Result<Option<Issue>, &'static str> {
        let current = self.feedback_status(target, token, number).await?;
        let mut found = false;
        for page in 1..=10 {
            let rows = self.comment_page(target, token, number, page).await?;
            for row in &rows {
                if row["body"].as_str().is_some_and(|s| s.contains(marker)) {
                    if found || !checked_comment(row, &current.issue, target, body) {
                        return Err("ambiguous comment");
                    }
                    found = true;
                }
            }
            if rows.len() < 50 {
                return Ok(found.then_some(current.issue));
            }
        }
        Err("comment reconciliation incomplete")
    }
}
fn checked_comment(value: &Value, issue: &Issue, target: &Destination, body: &str) -> bool {
    value["id"].as_i64().filter(|id| *id > 0).is_some_and(|id| {
        value["html_url"] == format!("{}#issuecomment-{id}", issue.url)
            && value["body"] == body
            && value["user"]["login"] == target.bot_login
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comment_requires_open_issue_with_explicit_unstarted_status() {
        let target = Destination {
            installation: 1,
            repository_id: 2,
            repository: "a/b".into(),
            bot_login: "feedback[bot]".into(),
        };
        let mut value = json!({"number":1,"id":10,"html_url":"https://github.com/a/b/issues/1","state":"open","status":{"category":"backlog"}});
        assert!(status(&value, &target).unwrap().eligible);
        for category in [json!(null), json!("started"), json!("unknown")] {
            value["status"]["category"] = category;
            assert!(!status(&value, &target).unwrap().eligible);
        }
        value["status"]["category"] = json!("planned");
        assert!(status(&value, &target).unwrap().eligible);
        value["state"] = json!("closed");
        assert!(!status(&value, &target).unwrap().eligible);
    }
}
