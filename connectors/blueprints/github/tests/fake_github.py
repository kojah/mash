"""An in-memory GitHub REST API behind githubkit's httpx transport."""

from dataclasses import dataclass, field
from datetime import datetime

import httpx
from githubkit import GitHub, TokenAuthStrategy

from mash_github.client import connect

TOKEN = "github_pat_secret-do-not-log"
API = "https://api.github.com"


def user(login: str, id: int, type: str = "User") -> dict:
    url = f"{API}/users/{login}"
    return {
        "login": login,
        "id": id,
        "node_id": f"U_{id}",
        "avatar_url": url,
        "gravatar_id": "",
        "url": url,
        "html_url": f"https://github.com/{login}",
        "followers_url": url,
        "following_url": url,
        "gists_url": url,
        "starred_url": url,
        "subscriptions_url": url,
        "organizations_url": url,
        "repos_url": url,
        "events_url": url,
        "received_events_url": url,
        "type": type,
        "site_admin": False,
    }


ADA = user("ada", 1)
BOT = user("helper[bot]", 2, "Bot")


def issue(
    repository: str,
    number: int,
    *,
    created: str = "2026-10-01T00:00:00Z",
    updated: str = "2026-10-02T00:00:00Z",
    body: str | None = None,
    pull: bool = False,
    merged: bool = False,
) -> dict:
    url = f"{API}/repos/{repository}/issues/{number}"
    html = f"https://github.com/{repository}/{'pull' if pull else 'issues'}/{number}"
    value = {
        "id": 1000 + number,
        "node_id": f"I_{number}",
        "url": url,
        "repository_url": f"{API}/repos/{repository}",
        "labels_url": url,
        "comments_url": url,
        "events_url": url,
        "html_url": html,
        "number": number,
        "state": "closed" if merged else "open",
        "title": f"Title {number}",
        "body": body if body is not None else f"Body of {number}",
        "user": ADA,
        "labels": [{"id": 1, "name": "bug"}],
        "milestone": None,
        "locked": False,
        "comments": 0,
        "closed_at": None,
        "created_at": created,
        "updated_at": updated,
    }
    if pull:
        value["draft"] = False
        value["pull_request"] = {
            "url": url,
            "html_url": html,
            "diff_url": html,
            "patch_url": html,
            "merged_at": updated if merged else None,
        }
    return value


def comment(id: int, body: str, created: str = "2026-10-01T01:00:00Z") -> dict:
    return {
        "id": id,
        "node_id": f"C_{id}",
        "url": f"{API}/comments/{id}",
        "html_url": f"https://github.com/c/{id}",
        "issue_url": f"{API}/issues",
        "body": body,
        "user": BOT,
        "created_at": created,
        "updated_at": created,
    }


def review(id: int, body: str, state: str = "APPROVED") -> dict:
    link = {"href": f"{API}/reviews/{id}"}
    return {
        "id": id,
        "node_id": f"R_{id}",
        "user": ADA,
        "body": body,
        "state": state,
        "html_url": f"https://github.com/r/{id}",
        "pull_request_url": f"{API}/pulls",
        "_links": {"html": link, "pull_request": link},
        "commit_id": "abc",
        "author_association": "OWNER",
        "submitted_at": None if state == "PENDING" else "2026-10-01T02:00:00Z",
    }


def review_comment(id: int, body: str, reply_to: int | None = None) -> dict:
    link = {"href": f"{API}/review-comments/{id}"}
    return {
        "url": link["href"],
        "pull_request_review_id": 7,
        "id": id,
        "node_id": f"RC_{id}",
        "diff_hunk": "@@ -1 +1 @@\n-old\n+new",
        "path": "src/main.rs",
        "line": 1,
        "side": "RIGHT",
        "commit_id": "abc",
        "original_commit_id": "abc",
        "in_reply_to_id": reply_to,
        "user": ADA,
        "body": body,
        "created_at": "2026-10-01T03:00:00Z",
        "updated_at": "2026-10-01T03:00:00Z",
        "html_url": f"https://github.com/rc/{id}",
        "pull_request_url": f"{API}/pulls",
        "author_association": "OWNER",
        "_links": {"self": link, "html": link, "pull_request": link},
    }


def instant(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


@dataclass
class FakeGitHub:
    issues: dict[str, list[dict]] = field(default_factory=dict)
    comments: dict[str, list[dict]] = field(default_factory=dict)
    reviews: dict[str, list[dict]] = field(default_factory=dict)
    review_comments: dict[str, list[dict]] = field(default_factory=dict)
    # Responses to return, in order, before serving a path normally.
    failures: dict[str, list[httpx.Response]] = field(default_factory=dict)
    calls: list[str] = field(default_factory=list)
    # What GET /user reports as the token's login.
    principal: str = "test"

    def add(self, repository: str, *items: dict) -> None:
        self.issues.setdefault(repository, []).extend(items)

    def github(self) -> GitHub[TokenAuthStrategy]:
        return connect(TOKEN, httpx.MockTransport(self.handle))

    def handle(self, request: httpx.Request) -> httpx.Response:
        assert request.method == "GET"
        assert request.headers["authorization"] == f"token {TOKEN}"
        path = request.url.path
        # Identity probes are not part of the listing call sequence the tests
        # assert on.
        if path == "/user":
            return httpx.Response(200, json={"id": 1, "login": self.principal})
        self.calls.append(str(request.url).removeprefix(API))
        pending = self.failures.get(path)
        if pending:
            return pending.pop(0)
        parts = path.split("/")  # "", repos, owner, name, kind, ...
        repository = f"{parts[2]}/{parts[3]}"
        query = request.url.params
        if parts[4:] == ["issues"]:
            found = self.issues.get(repository, [])
            if "since" in query:
                since = instant(query["since"])
                found = [i for i in found if instant(i["updated_at"]) >= since]
            found = sorted(found, key=lambda i: (i["updated_at"], i["id"]))
        else:
            key = f"{repository}#{parts[5]}"
            table = {
                "comments": self.comments,
                "reviews": self.reviews,
            }[parts[6]]
            if parts[4] == "pulls" and parts[6] == "comments":
                table = self.review_comments
            found = table.get(key, [])
        size = int(query.get("per_page", 30))
        number = int(query.get("page", 1))
        headers = {}
        if len(found) > number * size:
            following = request.url.copy_merge_params({"page": number + 1})
            headers["link"] = f'<{following}>; rel="next"'
        return httpx.Response(
            200, json=found[(number - 1) * size : number * size], headers=headers
        )
