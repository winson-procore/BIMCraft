# The rule pack seed library

`bimcraft-cli rule-pack seed --out DIR` (the `rule_pack` command, `crates/core/src/rulepack.rs`) writes the frontier-model seed
library: design rules for five building types (office, clinic, hospital, datacenter, airport) that three models proposed and an agent
curated. The spec is `specs/graph-building-generator.md`, slice 2. Nothing here is a code requirement and the generator does not read
it yet; it is the seed a later slice loads as rule packs.

## What a run writes

| File | What it holds |
|---|---|
| `rule-pack-manifest.json` | `schema_version`, `library_id` (`frontier-model-seed-library`), the models, each pack's path, counts and FNV-1a-64 hash, the report's hash |
| `validation-report.json` | the `RP-` checks (PASS or WARN: a FAIL writes nothing), their summary, and `capability_notes` saying what the library cannot vouch for |
| `packs/<archetype>.json` | `status: "seed"`, `origin: "frontier_model_seed"`, `models_consulted`, `prompt_hash`, the rules, the `conflicts` and a `validation` object of counts |

A rule is `id` (`<archetype>.<slug>`, unique across the library), `category`, `subject`, `predicate`, `object`, an optional `condition`,
`classification`, `checkability`, `confidence`, `rationale`, `exceptions` and `provenance` (`model_refs`, `source_proposals`). The
same files come out every time: the data is embedded in the binary, there are no maps of undefined order, and no time or path is written.

## Classification

Every rule is `exploratory` or `soft`, never `hard`: a model's opinion is not a requirement. A rule is `soft` only when a generator check
enforces it, and then it names that check as `deterministic_checker` (`KNOWN_CHECKERS`, today `GR-002`; a test runs the generator and
reads `report.json` to see that every named check is really run). At least 85% of a pack must stay exploratory (the gate's line is 80%).
`checkability` says how a rule could be checked (`deterministic`, `heuristic`, `human_review`), which is not the same thing: most
deterministic rules have no checker yet, and `RP-013` warns of the count.

## Where the rules came from

`crates/core/data/rule-packs/` holds, in the order they were made:

1. `seed-prompt.md`, the prompt given to each of three models, alone, with no sight of the others' work.
2. `proposals/claude-{opus,sonnet,haiku}.json`, what each model's run wrote to its own file; the curator did not edit them. Their keys are the
   proposal names: a reference is `claude-opus#airport/landside-airside-split`, model, archetype and key.
3. `seed-prompt-followup.md`, a second round to each model: the first round gave the hospital nothing on outpatient exam rooms,
   so each was asked for three to five more. They were appended to the model's own file, not written by the curator.
4. `<archetype>.json`, the curated seeds: an agent merged proposals that say the same thing, cited each of them in
   `source_proposals`, capped a rule's `confidence` at the highest of its sources, and kept the disagreements as `conflicts`.

The library is built from these files and nothing else. It does not call a model, so a build is the same any day.

**Provenance caveat.** The three "models" are all Anthropic models, asked by the aliases opus, sonnet and haiku in three separate
runs; the exact versions were not recorded, and the labels `claude-*` are the alias. Their agreement is therefore weak evidence:
they share a provider, probably training data and some of the same blind spots. The curator was an AI agent too. No rule was reviewed by
a person or checked against a published standard, and none has corpus evidence. `capability_notes` says all of this in every report.
`prompt_hash` is the FNV-1a-64 of the two prompts and the archetype: it detects a changed prompt, not a changed model.

## Conflicts

Where models disagreed the pack keeps both positions (`positions`, each a proposal ref and what it said), the rules concerned and a
`status`: `resolved_in_pack` with the reason, or `open` when both stay as separate rules. Examples: the airport's security predicate
(one model had it running the wrong way, and its own rationale disagreed), how strictly a hospital separates clean from dirty,
where a datacenter's operations centre sits. Open conflicts show as a `WARN` in `RP-011`.

## The checks

`RP-001` inputs parse strictly (`deny_unknown_fields`; a bad file is `invalid_seed`, naming the file) · `002` three models per pack ·
`003` size (8, 8, 10, 10, 10 rules) and topics (five words a pack must speak of) · `004` ids · `005` classification · `006` fields ·
`007` exploratory by default · `008` sources exist, for the pack's own archetype · `009` confidence capped · `010` conflicts name rules
and proposals that exist · `011` open conflicts (WARN) · `012` every model contributes (WARN) · `013` deterministic rules without a
checker (WARN). A failure refuses the library with `rule_violation` and the first failing check's id and why; nothing is written.
The other codes are `invalid_params` and `output_unwritable`; a file that can't be written takes the earlier ones away again, and the
manifest is written last.

## Looking at the result

```sh
bimcraft-cli rule-pack seed --out /tmp/rule-packs
jq '.checks[] | select(.status != "PASS")' /tmp/rule-packs/validation-report.json
jq '.rules[] | {id, classification, confidence}' /tmp/rule-packs/packs/airport.json
```

Run it twice into two directories and `diff -r` them: there is no difference. To change a rule, edit the curated seed and its
`source_proposals`; to change what models said, a new run of them is a new proposals file, which this slice does not make.
