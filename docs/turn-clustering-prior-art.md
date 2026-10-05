# Prior art for clustering turns inside agent sessions

Literature review, 5 October 2026. This concerns the current experiment: grouping
user/assistant turns within one session, including nonadjacent returns to a
subject. It does not reinstate global topic modelling or a topic hierarchy.

| Study | Relevant evidence | Consequence for our experiment |
| --- | --- | --- |
| [Perkins and Yang, EMNLP 2019](https://aclanthology.org/D19-1413/) | Intent induction with separate initial-query and remaining-dialogue views, jointly learned with alternating-view k-means. | Compare separate prompt/answer views against concatenation; response length should not automatically determine the representation. A fixed pooling experiment would be an adaptation, not a replication of their learned method. |
| [Park et al., DSTC 2023](https://aclanthology.org/2023.dstc-1.8/) | Evaluation of embedding and clustering combinations for task-oriented intent induction; MiniLM with agglomerative clustering performed well. | Keep compact embeddings and agglomerative methods among the candidates. Their MiniLM variants, corpus and metrics differ from ours; their scores cannot serve as our acceptance evidence. |
| [Zhou et al., NAACL 2022](https://aclanthology.org/2022.naacl-main.55/) | Dialogue Sentence Embedding trains with consecutive utterances as positive contrastive pairs and evaluates several dialogue tasks. | Context-sensitive representations merit consideration. Consecutive turns are not necessarily related in our evaluation: users can change subjects. This study requires training, rather than offering a free preprocessing improvement. |
| [Liu et al., EMNLP 2021](https://aclanthology.org/2021.emnlp-main.185/) | DialogueCSE learns context-aware/context-free response representations using contrastive learning, evaluated on three dialogue datasets. | Assistant responses and dialogue context can carry useful signal. Its retrieval/correlation evaluation does not establish clustering quality on our sessions. |
| [Xing and Carenini, SIGDIAL 2021](https://aclanthology.org/2021.sigdial-1.18/) | Utterance-pair coherence scoring for unsupervised dialogue topic segmentation. | Retain a contiguous segmentation/window baseline, but distinguish segmentation from clustering: returning to a subject later should remain possible. |
| [Tamkin et al., Clio, 2024](https://arxiv.org/abs/2412.13678) | A deployed system analyzes Claude conversations by extracting facets with an LLM, then embedding and clustering those facets with k-means. | Direct precedent for mining real LLM conversations, but conversation-level analytics and generative facet extraction differ from our within-thread, non-generative task. |
| [Mysore et al., EMNLP 2025](https://aclanthology.org/2025.emnlp-main.852/) | Studies writing sessions from Bing Copilot and WildChat. GPT-4o classifies follow-ups and intents; PCA of TF-IDF-weighted follow-up-type frequencies identifies collaboration patterns. | Distinguish a turn's subject from its conversational action, such as revising a request. Sharing an action is insufficient evidence to cluster two turns by subject. This is not plain semantic-vector clustering. |

The closest established clustering task is dialogue intent induction. Much of
the representation research concerns narrower task-oriented conversations.
Clio and the collaboration-pattern study directly analyze LLM sessions, but
use different units and generative stages. None of these sources establishes
performance on our complete, branching coding-agent histories with tools,
autonomous assistant activity and multiple subjects per user-boundary turn.
They support testing ideas rather than assuming transfer or claiming an existing
production system already implements our entire pipeline.

An additional [author-published session-clustering study by Abhishek
Verma](https://verma7.com/research/clustering/) reports a broad pipeline comparison
and warns that aggregate metrics can hide one dominant cluster. Its page does
not establish peer-reviewed status, and we have not reproduced its results.
The useful experimental precaution is to report cluster-size distributions,
assignment coverage, and agreement on shared samples. Its whole-session units,
commercial embeddings and LLM judging are different from our experiment. A
fixed largest-cluster-share cutoff should not automatically reject a within-thread
result: a genuinely single-subject thread can legitimately have one large group.

For the next comparison, keep turn extraction source-preserving and compare
representations and clustering together. Blind judgments should concern actual
turn pairs, include vocabulary-sharing negatives and nonadjacent related pairs,
and count false positives in threads without any positive judgment. Separate
annotation agents see randomized text and a rubric, without algorithm names or
scores. Agent-authored labels support a provisional decision; they are not human
validation. Keep old reserved corpora closed and reserve fresh independent
components where available. A provider without fresh components must be reported
as a confirmation-coverage limitation, rather than weakening leakage safeguards.

No new model downloads, fine-tuning, or LLM inference in the clustering pipeline
are implied by this review. Additional representation experiments must have a
frozen procedure before their results are inspected.

## Expanded sampling procedure

The user subsequently requested 200 threads. The local feasibility capture
contains 35 independent histories with at least eight prompted turns and 69 with
at least two; neither is the requested 200-thread experiment. To obtain more
data, capture the development MacBook's SQLite database using SQLite's backup
API, including committed WAL contents, and copy native sources to a private
research directory. Native files are captured later than the database and still
require ancestry verification; this is not a claim of a filesystem-wide atomic
snapshot. The research capture does not alter production data or trigger
indexing. The personal MacBook was unreachable during the check.

Combine the development MacBook and Xenia pools before copy/lineage grouping,
retain both previous reserved sets, and require 200 independent components.
Histories must have at least two and at most 250 prompted turns. Within each
provider, favor histories with at least eight turns, then fill from shorter
multi-turn histories in deterministic seeded order. Reserve up to twenty fresh
components per provider for confirmation, then alternate providers for
development until one provider's pool is exhausted. Report provider, node and
turn-count strata separately; a two-turn thread cannot test distant returning
subjects. If there are fewer than 200 eligible independent components, fail
with the available count instead of silently meeting the target using copies or
single-turn controls.

`snapshot_node.py` records source paths and checksums; `turn_study_corpus.py`
verifies these captures before selection. Private captures contain unredacted
history and remain outside Git. No new algorithm results or blind annotations
have been obtained at this sampling stage.

The final capture contains 200 independent histories and 5,082 prompted turns:
160 development histories (91 Claude Code, 69 Codex) and forty fresh confirmation
histories (twenty per provider). Of the selected histories, 140 come from the
development MacBook and sixty from Xenia; 102 have at least eight prompted turns
and 98 have two to seven. There are 178 histories not marked previously used.
Both earlier reserved sets remain excluded.

The combined pool contains 3,804 unique conversation identities. Eligibility
excluded 2,957 histories outside the prompted-turn range, 221 linked to reserved
components, and one native source with conflicting parent links for one UUID.
The first fleet capture stopped on that ambiguity; the incomplete capture is
retained privately, and native checks now run before selection. The validated
pool has 237 independent eligible components, sufficient for the target without
weakening ancestry checks. The selected capture records no native-parent
mismatches; Codex presence-check limitations still apply.

The frozen cohort is stored in
`/home/james/scratch/mash-artifacts/turn-study-fleet-verified-20261005`, SHA-256
`d779a9a1850cb29558987e510ddd2452878aca20ebb57dd12ee5c657d5192805`.
[The bounded comparison protocol](turn-clustering-study.md) defines the next
annotation, finite candidate comparison and stopping rules. This is collection
evidence, not a claim that clustering quality has improved.
