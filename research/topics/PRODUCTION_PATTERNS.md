# Production precedents and proposed scheduling boundary

Reviewed official product documentation on 2 October 2026. These are related
commercial capabilities, not reproductions or independent quality evaluations.

| System | Documented behavior | Limitation for mash |
| --- | --- | --- |
| [Intercom Topics Explorer](https://www.intercom.com/help/en/articles/11390087-use-the-topics-explorer-to-see-what-s-driving-volume) | Key questions form subtopics and broader groups; daily assignment supports multiple tags. | Summary/model mechanics are not disclosed enough to reproduce a non-LLM pipeline. |
| [LangSmith Insights](https://docs.langchain.com/langsmith/insights) | An LLM summarizes traces; another categorizes them hierarchically in scheduled/background reports. | Explicit generative inference conflicts with mash's constraint. |
| [Google CX Insights](https://docs.cloud.google.com/gemini-enterprise-cx/insights/topic-modeling-basics) | Discovers topic groups, permits curation, and deploys LLM-based topic inference for new conversations. | Real-time assignment does not prove online taxonomy discovery or non-LLM compatibility. |

The cited documentation does not establish BIRCH/GRINCH as the implementation,
nor demonstrate quality or speed on coding-agent histories. Do not infer these
details from the products' tree-shaped interfaces. No vendor service was called
with project/user data and no LLM experiment was run.

## Proposed architecture direction, not algorithm selection

Separate bounded topic evidence/assignment from taxonomy maintenance. Ingestion
can attach evidence to existing topics and keep unsupported evidence provisional.
A coalesced background job within the existing process can refine labels and
topic groups under explicit budgets. It need not block searchable message
publication. Discovery may still be incremental; it need not restructure the
entire semantic taxonomy on every arriving message. This is our design inference
from the documented schedules, not a claim that all products share one algorithm.

Preserve these requirements whichever candidate wins:

- Original source evidence and explicit unresolved cases; no generated summaries.
- Existing local embeddings plus bounded measured nongenerative work only.
- Multiple topic memberships per thread with representative source links.
- Internal clustering topology kept distinct from a semantic taxonomy. Ancestor
  inheritance needs validated parent relationships.
- Versioned topic/tag snapshots so background revisions do not mix generations;
  edits/deletions must retract evidence and stale inherited tags.
- Local offline behavior and one shared topic namespace per node group, with
  provider-independent identities. Exact placement/protocol are not implemented
  by this document.

Before Rust promotion, one explicit candidate must pass source/coverage,
independent semantic assignment/label/ancestor and guided-search checks; matched
complete ingestion must meet the provisional <=10% throughput-loss target;
deterministic shared Python/Rust stage fixtures must gate CI. Tiny accepted
prefixes, coarse intent proxies, low ECE or a tree visualization cannot pass.

Current evidence rejects F1-driven cosine/lexical splitting as a reliable
automatic segmentation policy at the frozen precision gate. Do not spend more
iterations tuning those cutoffs. Next prototype selection should compare a
bounded source-evidence representation against message-only evidence with
clustering held fixed, using the QMSum membership/recurrence source audit and
separate technical-session evidence. This is a concrete next comparison, not
permission to use gold topic names/answers as representation inputs.
