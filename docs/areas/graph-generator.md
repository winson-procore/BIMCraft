# The graph building generator

`bimcraft-cli generate --archetype office|clinic|hospital --seed N (--area M2 | --area-sqft SQFT) --out DIR` (the `generate`
command, `crates/core/src/generate.rs`)
turns a brief into a run directory. Deterministic code owns the geometry and the pass/fail; nothing in it is asked of a model. The
spec is `specs/graph-building-generator.md`; the first shipped slices cover office, clinic and a larger hospital archetype.

## What a run writes

| File | What it holds |
|---|---|
| `program-graph.json` | the spaces the building must hold (kind, role, least and target area) and the access each needs |
| `topology-graph.json` | nodes (id, kind, role, gross `area_m2`) and edges: `door` (a room to the corridor) and `adjacent` (neighbours in a band) |
| `building-ir.json` | the placed building in millimetres: footprint, corridor, each space's rectangle, walls and openings |
| `script.bimcraft.txt` | the building-ir compiled to one command to a line, for `bimcraft-cli run --script` (no file path in it) |
| `report.json` | the checks (stable ids), warnings, capability notes, scores and each room's net area as BIMCraft measured it |
| `manifest.json` | generator version, archetype, integer seed, requested and gross area, each file's name and FNV-1a-64 hash, the topology hash |

Only `door` and `open` edges are access; `adjacent` and `near` are softer and no rule reads them. The same brief gives the same bytes:
whole millimetres, integer seeds, a splitmix `Rng` (`generate::Rng`), no maps with undefined order, no times or paths in a file.

## The layout

One storey, one rectangle with one of three layout strategies: `double_loaded_corridor`, `single_loaded_corridor`, or
`offset_corridor`. The strategy is chosen from the seed and exported in the manifest, topology, building graph and report. The depth
is the square root of the area over 3, held between 12 and 24 m, so a very large or small area lands off the request and the report
warns (`GM-001`, over 5%). The program (`program.rs`) grows with the area and takes a few of its counts from the seed; the lobby is
the first room with the way in on the west wall; every room has one door to the corridor. Rooms are dealt into the strategy's band(s),
in an order the seed shuffles, each as wide as its least area asks.

## Refusals

A brief that can't be met writes nothing and the error starts with a code: `invalid_params`, `unknown_archetype`, `invalid_area`
(above 0, at most 5000 m2), `unsatisfiable_area` (the minimum program doesn't fit: the message says it is too small and the least area
that works), `rule_violation` (a check failed: a bug in the generator, not in the brief), `generation_failed`, `output_unwritable`.
The files are made only after every check has passed, and a file that can't be written takes the earlier ones away again.

## The checks

Before anything is written the script is run on a scratch session: every command must run, every room must be bounded (a `room`
reply with a `warning` fails `RT-002`) and `Model::check` must pass. `GR-001..006` and `PR-001` are the rules on the graph (reach, no
private to private, no clinical to clinical, no private to clinical, connected from the entry, the minimum program); `SC-` the
script; `RT-` what BIMCraft did with it; `GM-` the geometry. The rules are hard-coded (`slice1-hard-rules/1`) over roles and edge
kinds, which is the shape a versioned rule pack will load.

## Looking at the result

```sh
bimcraft-cli generate --archetype hospital --seed 10 --area 1800 --out /tmp/hospital-1
cd /tmp/hospital-1 && bimcraft-cli run --script script.bimcraft.txt --save model.bimcraft --save plan.png --save plan.dxf
bimcraft-cli generate --archetype office --seed 2 --area-sqft 10000 --out /tmp/office-10000sf
```

Open `plan.png`: named, numbered rooms with their areas, a door in each room's wall to the corridor, windows on the outside wall,
tags, three dimensions and a note. The sheet `A101` holds the plan (`--save` a `.pdf` for it).

## Not yet

No stairs, shafts, structure or MEP; one storey; door swing and hand are BIMCraft's defaults; no datacenters or airports;
no batch runs, and the rules are still hard-coded: the seed rule packs of [rule packs](rule-packs.md) are written but not read. `report.json` says so in `capability_notes`.
