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

One storey, with a seeded layout family (`corridor_bar`, `central_commons`, `clustered_commons`, or `atrium_commons`), boundary shape
(`rectangle`, `l_shape`, `stepped_bar`, `t_shape`, or `bar_with_wings`) and corridor strategy (`double_loaded_corridor`,
`single_loaded_corridor`, or `offset_corridor`). The family, strategy and boundary shape are chosen from the seed and exported in the
manifest, program graph, topology, building graph and report. Corridor-bar plans keep the older long-bar proportion; open families
use a more compact aspect ratio and a wider shared circulation zone, with `atrium_commons` reserving the largest shared zone. The
program (`program.rs`) grows with the area and takes a few of its counts from the seed; the lobby is the first room with the way in on
the west wall; every room has one door to the shared/circulation zone. Rooms are dealt into the strategy's band(s), in an order the
seed shuffles, each as wide as its least area asks. Non-rectangular boundaries shorten, center, or step room bands against the
shared/circulation bar, and the footprint polygon is used for the exterior walls, floor command and slab boundary.

Gridlines come from the same geometry: footprint extents, corridor/shared-zone edges, boundary vertices and room/module breaks.
Vertical grids are numbered and horizontal grids are lettered, so larger or more subdivided plans carry denser coordination grids in
the script and exported drawings instead of only four outside grids.

## Door and window symbols

Each brief takes an `opening_symbol_profile` from its seed and archetype (`generate/openings.rs`): `classic_punched`,
`storefront_ribbon`, `service_secure` or `slot_clerestory`. It is exported in the manifest, program graph, topology graph, building
graph and report (the manifest and the report also count `opening_symbols`, doors and windows apart). A profile fixes the symbol of
each opening by the room's kind and role:

| Doors | Symbol | Made as |
|---|---|---|
| the way in (lobby, west wall) | `main_entry` | double or single leaf by profile, as wide as the corridor allows |
| an exit from a back-of-house room with no window | `back_door`, `service_exit` | one leaf, 1000 and 1200 wide; every building has at least one |
| a room to the corridor | `single_swing`, `double_swing`, `service`, `secure` | 900, 1600 (public and wide rooms), 1100, 1000 wide |

Windows are `punched`, `ribbon`, `storefront`, `clerestory` or `narrow_slot`, each with its own width, height and sill.
Every building-graph opening carries `role` (`main_entry`, `back_door`, `service_exit`, `room_access`, `window`), `symbol`, `type`,
`width_mm`, `height_mm`, `sill_mm`, `operation` and `flip_hand`. The script makes one `type` command for each symbol used, with the
width, height, sill and operation, then each `door` and `window` names its `type` (`LIFT-007` checks BIMCraft holds what the graph says). Only the
way in and the exits have no topology edge; a room's door is still the lift of its `door` edge, which carries its `opening_symbol`.
BIMCraft's plan draws a door's width and one swing arc, and a window's width: a double leaf, and a clerestory or a slot, differ in
the graphs, the IFC operation and the schedule, not in the picture.

## Refusals

A brief that can't be met writes nothing and the error starts with a code: `invalid_params`, `unknown_archetype`, `invalid_area`
(above 0, at most 5000 m2), `unsatisfiable_area` (the minimum program doesn't fit: the message says it is too small and the least area
that works), `rule_violation` (a check failed: a bug in the generator, not in the brief), `generation_failed`, `output_unwritable`.
The files are made only after every check has passed, and a file that can't be written takes the earlier ones away again.

## The checks

Before anything is written the script is run on a scratch session: every command must run, every room must be bounded (a `room`
reply with a `warning` fails `RT-002`) and `Model::check` must pass. `GR-001..006` and `PR-001` are the rules on the graph (reach, no
private to private, no clinical to clinical, no private to clinical, connected from the entry, the minimum program); `SC-` the
script; `RT-` what BIMCraft did with it; `GM-` the geometry. The code owns each predicate (`slice1-hard-rules/1`) over roles and edge
kinds; since Slice 11 the parameters (the role pairs, the minimum program's groups) are read from the packs in
`crates/core/data/graph-rules/` ([graph rule packs](rule-packs.md#graph-rule-packs-and-the-checker)), and `report.json` is byte for
byte what it was (its `capability_notes` still say "hard-coded"; they are part of the pinned bytes).

## Looking at the result

```sh
bimcraft-cli generate --archetype hospital --seed 10 --area 1800 --out /tmp/hospital-1
cd /tmp/hospital-1 && bimcraft-cli run --script script.bimcraft.txt --save model.bimcraft --save plan.png --save plan.dxf
bimcraft-cli generate --archetype office --seed 2 --area-sqft 10000 --out /tmp/office-10000sf
```

Open `plan.png`: named, numbered rooms with their areas, a door in each room's wall to the corridor, windows on the outside wall,
tags, three dimensions and a note. The sheet `A101` holds the plan (`--save` a `.pdf` for it).

## The topology extractor

`topology` (`topology.rs`; `bimcraft-cli topology --model FILE.bimcraft --out FILE.json`) is the reverse: a model's 2D graph from its
geometry alone, in the shape of `topology-graph.json` with `graph_kind: "extracted"`. Nodes are the enclosed rooms (id: the room
number if unique, else `room-<element id>`; boundary, holes, area, level). Every edge is found by sampling points either side of a
line and asking which room holds each (`Spaces::room_at`):

- `door`: the rooms either side of a door's host wall (at the wall's centre ± half its thickness + 150 mm), with the door's family
  (a typed door's type), type, `operation` and `width_mm` from `Model::opening_dims`. A room on one side only is an exterior door
  (`to: "exterior"`). `entry` is the room of the exterior door that is a swing door, then the widest, then the lowest id.
- `open`: rooms sharing at least `MIN_SHARED` (300 mm) of room separation line; a shorter touch (a separator's end grazing a third
  room) is no link, and a separator with a room on one side only gives nothing.
- `adjacent`: rooms sharing at least 300 mm of wall, with no door or open edge between them.
- `footprint`: the largest outline of the lowest level's bounding lines at the wall centres, dangling stubs trimmed, corners only.
- `diagnostics`, each with a `code`: `room_not_enclosed` and `room_shares_space` (the room is no node: no boundary is made up),
  `door_without_room`, `door_type_unknown`. Written even then. Nodes carry no kind: the house rule checker will need one.

It gives the same graph measured at the wall centres or faces, and the same bytes for the same model.

## The trace contract

`topology --expect FILE.json` (`topology/expect.rs`; `bimcraft-cli topology --model M --expect E [--out G]`) compares the extracted
graph with a traced plan's expected file, the same for any building type. The file is `rooms` (`{id, name?, kind}`; `id` is the room
number), `links` (`{from, to, via}`, an unordered pair; `via` is `open` or a door operation) and `entry` (`{room, via?}`), with
optional `building`, `review` and `source`. It is read strictly (unknown keys, a duplicate or unknown room, an unknown `via`, an empty
`kind` and an oversized file are errors that name the field). Compared: the room ids, the `door` and `open` edges that aren't exterior
(by pair and via), `entry.room`, and that `diagnostics` is empty. Not compared: kinds, `building`, `review`, `source`, `entry.via`.

The fields are named from the model's side, which reads backwards at first: `missing_links` (and `missing_rooms`) are in the model and
not in the expected file; `extra_links` (and `extra_rooms`) are in the file and not in the model. `entry` is null, or
`{expected, extracted}`. The command replies with these under `comparison` (with `agree`); a disagreement is still an `Ok` reply
there, but the CLI exits non-zero with `{"error": "topology_mismatch", ...six fields}` on stderr. `path` is written before the
comparison and never holds it, so `--out` gives the same bytes with or without `--expect`.

## Not yet

No stairs, shafts, structure or MEP; one storey; no datacenters or airports;
no batch runs, and the rules are still hard-coded: the seed rule packs of [rule packs](rule-packs.md) are written but not read. `report.json` says so in `capability_notes`.
