use super::{BTreeMap, BTreeSet, GroupId, TopicId};
use crate::AppError;
use crate::topics::labels::Terms;

/// Union supporting words before counting a thread once for each parent class.
/// Never substitute a whole-thread word bag or sum child-class frequencies.
pub(super) fn classes(
    ancestors: &BTreeMap<TopicId, BTreeSet<GroupId>>,
    direct: &BTreeMap<String, BTreeSet<TopicId>>,
    supporting: &BTreeMap<String, BTreeMap<TopicId, Terms>>,
) -> Result<BTreeMap<GroupId, Terms>, AppError> {
    let mut classes: BTreeMap<GroupId, Terms> = BTreeMap::new();
    for (thread, topics) in direct {
        let words = supporting
            .get(thread)
            .ok_or_else(|| AppError::usage("parent labels require captured supporting evidence"))?;
        if words.len() != topics.len() || words.keys().any(|id| !topics.contains(id)) {
            return Err(AppError::usage(
                "parent label evidence does not match direct tags",
            ));
        }
        let mut parent_words: BTreeMap<GroupId, BTreeSet<&str>> = BTreeMap::new();
        for (topic, terms) in words {
            let path = ancestors
                .get(topic)
                .ok_or_else(|| AppError::usage("parent label evidence references unknown topic"))?;
            for parent in path {
                parent_words
                    .entry(*parent)
                    .or_default()
                    .extend(terms.keys().map(String::as_str));
            }
        }
        for (parent, words) in parent_words {
            let class = classes.entry(parent).or_default();
            for word in words {
                *class.entry(word.to_owned()).or_default() += 1;
            }
        }
    }
    Ok(classes)
}

#[test]
fn parent_words_are_counted_once_per_thread_and_require_exact_evidence() {
    let ancestors = BTreeMap::from([
        (TopicId(0), BTreeSet::from([GroupId(3), GroupId(4)])),
        (TopicId(1), BTreeSet::from([GroupId(3), GroupId(4)])),
        (TopicId(2), BTreeSet::from([GroupId(4)])),
    ]);
    let direct = BTreeMap::from([
        ("a".into(), BTreeSet::from([TopicId(0), TopicId(1)])),
        ("b".into(), BTreeSet::from([TopicId(2)])),
    ]);
    let words = BTreeMap::from([
        (
            "a".into(),
            BTreeMap::from([
                (
                    TopicId(0),
                    Terms::from([("database".into(), 1), ("shared".into(), 1)]),
                ),
                (
                    TopicId(1),
                    Terms::from([("storage".into(), 1), ("shared".into(), 1)]),
                ),
            ]),
        ),
        (
            "b".into(),
            BTreeMap::from([(
                TopicId(2),
                Terms::from([("network".into(), 1), ("shared".into(), 1)]),
            )]),
        ),
    ]);
    let classes = classes(&ancestors, &direct, &words).unwrap();
    assert_eq!(classes[&GroupId(3)]["shared"], 1);
    assert_eq!(classes[&GroupId(4)]["shared"], 2);
    assert!(!classes[&GroupId(3)].contains_key("network"));
    assert_eq!(classes[&GroupId(4)]["database"], 1);
    assert!(super::names::classes(&ancestors, &direct, &BTreeMap::new()).is_err());
    let mut wrong = words;
    wrong.get_mut("a").unwrap().remove(&TopicId(1));
    assert!(super::names::classes(&ancestors, &direct, &wrong).is_err());
}
