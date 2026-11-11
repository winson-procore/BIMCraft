# Rule pack seed prompt

You are one of several models asked, independently, to propose candidate design rules for generating building plans as graphs of
spaces. Your proposals are raw material: a person distils them into a versioned rule pack, and deterministic code checks the
pack. Nothing you write is trusted as true; say how sure you are.

## Archetypes

`office`, `clinic`, `hospital`, `datacenter`, `airport`.

## What to cover for each archetype

- office: public entry and reception, shared circulation access, meeting rooms, private or focus rooms, restrooms, support spaces.
- clinic and hospital: public, staff and service flow separation; waiting, exam and support relationships; clean and dirty
  separation; nurse and support adjacency; patient privacy.
- datacenter: secure entry and mantrap, data halls, electrical rooms, cooling and mechanical spaces, loading and service access,
  redundant A/B path diversity.
- airport: landside and airside split, security screening, check-in, baggage, arrivals and departures separation, gates and
  concourse relationships.

## What a proposal is

Propose 12 to 16 rules for each archetype, in your own words, from your own knowledge of how such buildings are planned. Each
rule is a statement about spaces and the relations between them, as the edges of a graph: a subject space role, a predicate,
an object space role. Write the rationale in original prose; do not quote or paraphrase text from standards, codes, vendor guides
or any proprietary material. Where you know a rule is contested, or has real exceptions, say so in `exceptions` and lower your
confidence: do not smooth it over.

Each rule has:

- `key`: a short unique slug within the archetype, lower case, for example `reception-at-entry`.
- `category`: one of `program`, `access`, `adjacency`, `flow`, `separation`, `security`, `systems`, `dimensional`, `equipment`.
- `subject` and `object`: space roles in snake_case (`reception`, `exam_room`, `mantrap`).
- `predicate`: preferably one of `adjacent_to`, `separated_from`, `directly_accessible_from`, `not_directly_accessible_from`,
  `reached_only_through`, `requires`, `has_minimum`, `shares_route_with`, `must_not_share_route_with`; or your own snake_case word
  when none fits.
- `condition`: optional, when the rule applies only in some cases.
- `checkability`: `deterministic` (a program over the graph could check it), `heuristic` or `human_review`.
- `confidence`: a number from 0 to 1.
- `rationale`: one or two sentences.
- `exceptions`: a list of strings, possibly empty.

## Output

One JSON document and nothing else:

```json
{
  "archetypes": {
    "office": [ { "key": "...", "category": "...", "subject": "...", "predicate": "...", "object": "...",
                  "checkability": "...", "confidence": 0.7, "rationale": "...", "exceptions": [] } ]
  }
}
```

with all five archetypes. Work alone: do not read other proposal files or any rule pack in the repository.
