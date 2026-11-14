use super::*;

struct Row {
    role: Role,
    text: &'static str,
    parent: Option<i64>,
}

fn row(role: Role, text: &'static str) -> Row {
    Row {
        role,
        text,
        parent: None,
    }
}

fn branch(role: Role, text: &'static str, parent: i64) -> Row {
    Row {
        role,
        text,
        parent: Some(parent),
    }
}

fn turns(rows: &[Row]) -> Vec<Turn> {
    let ids: Vec<String> = (0..rows.len()).map(|i| format!("m{i}")).collect();
    let sources: Vec<Source<'_>> = rows
        .iter()
        .zip(&ids)
        .enumerate()
        .map(|(ordinal, (row, id))| Source {
            id,
            ordinal: i64::try_from(ordinal).unwrap(),
            role: &row.role,
            projection: row.text,
            context_parent: row.parent,
        })
        .collect();
    extract("chat", &sources)
}

fn shape(turns: &[Turn]) -> Vec<(Vec<usize>, Vec<usize>, Boundary, bool)> {
    turns
        .iter()
        .map(|t| (t.prompts.clone(), t.answers.clone(), t.boundary, t.closed))
        .collect()
}

#[test]
fn identities_and_revisions_match_the_research_extractor() {
    let linear = turns(&[
        row(Role::User, "fix the bug"),
        row(Role::Assistant, "done"),
        row(Role::User, "thanks"),
        row(Role::Assistant, "ok"),
    ]);
    assert_eq!(
        shape(&linear),
        [
            (vec![0], vec![1], Boundary::Start, true),
            (vec![2], vec![3], Boundary::UserReply, false),
        ]
    );
    assert_eq!(
        linear[0].id,
        "300604d22357f0ae7b47b9d79e71711f8251f8c604eba9cdaaffb0f31d295c4f"
    );
    assert_eq!(
        linear[0].revision,
        "1c81edcc23fac6196e8c22f1674f4aa16a52ed1e07add7de4a7edd143bc2e269"
    );
    assert_eq!(
        linear[1].id,
        "e33defca95d55e83d4408662548f72519cc289bee65db6e2ad5eb338fde02111"
    );
    assert_eq!(
        linear[1].revision,
        "fc5684e7bd1858a34cdea418697600d0a206a6e58d0a2e44154718010afdd91f"
    );
    let branched = turns(&[
        branch(Role::User, "root", -1),
        branch(Role::Assistant, "a", 0),
        branch(Role::User, "branch", -1),
        branch(Role::Assistant, "b", 2),
    ]);
    assert_eq!(branched[1].boundary, Boundary::Branch);
    assert_eq!(
        branched[0].revision,
        "197b059891268dc56a3eb8164a3900b0cddbe66f39ac8eac538cf1c491ce45eb"
    );
    assert_eq!(
        branched[1].revision,
        "865274b8cae06afe89bc80e46c1116946c8a673b9d3e3d365e59350bc00555aa"
    );
}

#[test]
fn consecutive_prompts_and_answers_share_a_turn_until_the_user_replies() {
    let text = "useful source text with enough words";
    let result = turns(&[
        row(Role::User, text),
        row(Role::User, text),
        row(Role::Assistant, text),
        row(Role::Assistant, text),
        row(Role::User, text),
        row(Role::Assistant, text),
    ]);
    assert_eq!(
        shape(&result),
        [
            (vec![0, 1], vec![2, 3], Boundary::Start, true),
            (vec![4], vec![5], Boundary::UserReply, false),
        ]
    );
}

#[test]
fn roots_branch_leading_answers_orphan_and_injected_records_are_skipped() {
    let text = "useful source text with enough words";
    let result = turns(&[
        row(Role::Assistant, text),
        row(Role::User, text),
        row(Role::Assistant, text),
        branch(Role::User, text, -1),
        row(Role::User, "# AGENTS.md injected"),
    ]);
    assert_eq!(result[0].boundary, Boundary::Orphan);
    assert_eq!(result.last().unwrap().boundary, Boundary::Branch);
    assert!(!result.iter().flat_map(Turn::members).any(|i| i == 4));
}

#[test]
fn edits_change_the_revision_but_not_the_identity() {
    let old = turns(&[row(Role::User, "ask"), row(Role::Assistant, "answer")]);
    let new = turns(&[
        row(Role::User, "ask"),
        row(Role::Assistant, "edited answer"),
    ]);
    assert_eq!(old[0].id, new[0].id);
    assert_ne!(old[0].revision, new[0].revision);
    let reduced = turns(&[row(Role::User, "ask")]);
    assert_eq!(reduced[0].prompts, [0]);
}

#[test]
fn assistant_activity_without_visible_text_still_ends_the_prompt() {
    // A tool-only assistant record has no dialogue, yet it responded.
    let result = turns(&[
        row(Role::User, "run the tests"),
        row(Role::Assistant, ""),
        row(Role::User, "now deploy"),
    ]);
    assert_eq!(
        shape(&result),
        [
            (vec![0], vec![], Boundary::Start, true),
            (vec![2], vec![], Boundary::UserReply, false),
        ]
    );
    // Without activity, two prompts in a row form one turn.
    let joined = turns(&[row(Role::User, "one"), row(Role::User, "two")]);
    assert_eq!(
        shape(&joined),
        [(vec![0, 1], vec![], Boundary::Start, false)]
    );
}

#[test]
fn parents_through_skipped_records_resolve_to_the_nearest_eligible_one() {
    // The answer's native parent is an injected record whose parent is the
    // prompt, so the answer stays on the prompt's branch.
    let result = turns(&[
        branch(Role::User, "ask", -1),
        branch(Role::User, "<skill> instructions", 0),
        branch(Role::Assistant, "answer", 1),
    ]);
    assert_eq!(shape(&result), [(vec![0], vec![2], Boundary::Start, false)]);
}
