# Importing, linking and exporting CAD drawings

On the roadmap under 0.9 Interoperability, as Revit's Import CAD and Link CAD: a consultant's or a surveyor's DXF drawing
brought into a project and drawn under the model in the plans of a level, to trace walls over and to check against. A project
holds each drawing once, an import element places it, plans draw and snap to it, and the desktop brings one in (Insert > Import
CAD, Link CAD). A DWG comes in as the DXF it becomes (below), and a level's plan goes out as DXF or DWG the same way backwards
([Export](#export)); exploding an import into detail lines (which waits on detail lines, issue #58), and images and PDFs as
underlays are still to come.

## The reader

`dxf.rs` reads ASCII DXF of any version from Autodesk's public DXF reference: pairs of a group code and a value, read in
sections. From the header it takes `$INSUNITS` (the drawing's unit, `Dxf::unit` and `Dxf::scale`, mm to each of its units),
`$ACADVER` and `$LTSCALE`; from the tables its layers (colour, off when the colour is negative, frozen by flag, linetype, line
weight) and its linetypes' dash patterns; from the blocks section the blocks; and from the entities section what model space
draws (an entity with code 67 set is paper space's, and left out). Entities come in as a short list of `Shape`s, in the
drawing's own units: LINE, LWPOLYLINE and POLYLINE (bulges become arcs; a 3D mesh or polyface is left out), CIRCLE, ARC,
ELLIPSE (as a polyline), SPLINE (its fit points, else its control points), POINT, TEXT and MTEXT (its formatting codes taken
out, aligned as the file says), SOLID and TRACE, and HATCH (its boundary loops, solid or patterned; a patterned one is drawn as a
pale fill of its colour, since its pattern isn't kept). An INSERT places its block's entities, scaled, turned and arrayed, up to
`MAX_DEPTH` blocks deep; an entity on layer 0 of a block takes the insert's layer, and one by block its colour. DIMENSION draws
its anonymous block. What the reader doesn't know (3DFACE, a viewport, an image, a proxy) is counted by kind in
`Dxf::left_out`, and the command lists it. The text itself is read as UTF-8 where it is, and else by the code page the header names in
`$DWGCODEPAGE`: the East Asian pages and the Windows ones decode through `encoding_rs`, and a page without a decoder here (the DOS
ones) reads as Latin-1, as it always has.

A file is hostile: `MAX_BYTES` caps its size, `MAX_PAIRS` its pairs, `MAX_SHAPES` the shapes, `MAX_WORK` the points the blocks
may multiply into, and every coordinate is a finite number within `MAX_FILE_COORD` (one past that is left out as "past what a
drawing holds"). A file that ends after a group code, a value that isn't a number, a binary DXF and a DWG each say why.

## DWG

The core reads no DWG of its own. `crates/dwg` (`bimcraft-dwg`) is a bridge: the acadrust crate (pure Rust, MPL-2.0, used
unmodified as a separate library; its DWG code is ported from ACadSharp, MIT, after the Open Design Alliance's published DWG
specification; NOTICE carries both) reads the file in failsafe mode, and its DXF writer writes what it read as ASCII DXF, which
`dxf.rs` reads as any other. `Limits` holds a DWG to 64 MB before the decoder runs and its DXF to `dxf::MAX_BYTES` while it is
written; a read that left object records out says how many, of which kinds, in `import_cad`'s `notes` (the desktop puts them on
the status bar). The project holds the DXF, so a project file opens where no DWG reader is.

A session reads a DWG through the `DwgReader` it is given (`Session::dwg`; `read_dxf_file` in `commands.rs` is the one place
`import_cad`, `reload_cad` and opening a project read a drawing), and one without says to save it as DXF. The desktop and the
command line (so MCP too) hand theirs `bimcraft_dwg::reader()` behind their default `dwg` feature: it starts the program
itself as a child (`--dwg-to-dxf <path>`, answered by `child_main` before anything else runs) and reads the DXF from its
output. The decoder allocates what a file declares and can't be interrupted, so a hostile file could abort the process; in
the child it takes only the child, which is also stopped after `CHILD_TIME` or past the limits, and the reply says the file
couldn't be read. Tests make their DWG files from DXF text with the same library (`dwg_from_dxf`, the `testing` feature),
since Autodesk's are never committed; `BIMCRAFT_DWG_SAMPLES` names a folder of real ones for
`real_drawings_where_a_folder_of_them_is_named`, and `apps/bimcraft-cli/tests/dwg.rs` drives the child.

## Export

A level's plan goes out as CAD, as Revit's Export > CAD Formats does for a view: `save` to a `.dxf` or `.dwg` path (so
`--save plan.dwg`, `convert house.bimcraft plan.dxf` and the MCP `save` tool), of the lowest level or the level or plan view
`view` names, and `render` with such a `path`. It runs the import's route backwards: `render::plan` builds the plan's
`Drawing`, `render::dxf` (`render/dxf.rs`, the core's own code from the public DXF reference) writes it as ASCII DXF, and for a
DWG the session's `DwgWriter` turns that text into the file.

- **The DXF** is AutoCAD 2018 (`$ACADVER` AC1032), in millimetres (`$INSUNITS` 4), its extents the plan's. Points go back from
  the plan's pixels to the model's mm with `View::mm`. Each `Prim::Line` is an `LWPOLYLINE` (its closed flag, the DXF line
  weight nearest its width on paper, its colour as a true colour, code 420); a dashed one takes a linetype of its dashes in mm
  (`BIM-DASH-…`) in the LTYPE table. Each `Prim::Fill` is a solid `HATCH`, its rings boundary loops. A picture (`Prim::Image`)
  is left out, and the reply's `notes` say how many. It has the tables, blocks and root dictionary AutoCAD expects, at
  AutoCAD's conventional handles (the tables 1 to A, `*Model_Space` at 1F), which libredwg's `dxf2dwg` reads without a complaint.
- **Paper is not an entity.** A plan hides what it drew before by laying a fill of the paper's colour (`render::PAPER`) over it,
  which a PNG gets from paint order and a CAD viewer, drawing hatches over lines, does not (a white hatch over a window hides
  its glass). So `render::dxf` writes no paper fill: `clipped` cuts what came before each one out of the primitives (lines by
  `geom::line_outside_convex`, fills by `walls::split`, one hatch per convex piece), so the poché is open at a door or window,
  and a wall's face lines stop at a jamb. A primitive may therefore go out as several entities, and the `Drawing` is untouched,
  so the PNG and SVG are the same. A mask with holes, or too many pieces, is left as it is.
- **Layers** follow the AIA CAD Layer Guidelines, by the category of the element a primitive draws (`layer` in `render/dxf.rs`,
  one table): `A-WALL`, `A-DOOR`, `A-GLAZ` (windows), `A-FLOR`, `A-CLNG`, `A-ROOF`, `A-COLS`, `S-COLS`, `S-BEAM`, `S-FNDN`,
  `A-STRS`, `A-FLOR-HRAL` (railings), `A-FURN` and the other components by their family's category (`P-FIXT`, `E-POWR`,
  `E-LITE`, …), `A-GRID`, `A-AREA-IDEN` (rooms and their tags), `A-ANNO-DIMS`, `A-ANNO-TEXT`, `A-ANNO-SYMB` (tags, spot
  elevations, section and elevation marks), `P-PIPE`, `M-DUCT`, and `A-ANNO-MISC` for what belongs to no element (the view's
  title) or to one no layer is kept for. A model element's fills and pattern lines go on `<layer>-PATT` (a cut wall's poché
  on `A-WALL-PATT`) and its lettering on `<layer>-IDEN`; annotation keeps all of its own on its layer.
- **Lettering** goes out as text a CAD program can edit, and its strokes do not. `Drawing::lettering` (`render::Lettering`, in
  pixels) holds each string beside the strokes it is drawn in, which `Drawing::lettered` pairs by `Part::Text`; a PNG or SVG
  ignores it. `render::dxf` writes each as TEXT on its owner's layer in style `Standard` (`Kitchen`, tags, spot elevations:
  our own single-stroke lettering, which has no typeface) or, when the plan laid it out in a typeface (a dimension's number,
  a note: `text::layout`), as an MTEXT holding `{\fArial|b0|i0|c0|p34;…}`, which is how `dxf::mtext` knows the typeface and
  the importer (`render::cad_imports`) lays it out in it again, so a re-imported plan looks as it did. A string with several
  lines, `\`, `{` or `}` is an MTEXT too (escaped, lines as `\P`, line spacing 0.96 since an MTEXT's is 5/3 of the height).
  Characters beyond ASCII are written as they are: AC1032's DXF is UTF-8. The strokes under a lettering are left out of the
  entities (the extents keep them), and a lettering that isn't a finite shape is left out harmlessly.
- **Dimensions** go out as `DIMENSION` entities, not strokes, for an aligned or linear dimension of a continuous string
  (`render/dxf/dims.rs`). `render/dimensions.rs` pushes a `Drawing::dimensions` record (`render::DimensionRecord`, model mm) for
  each segment beside the lines and ticks it draws, which a PNG or SVG ignores. Each record is a `DIMENSION` on `A-ANNO-DIMS`
  (aligned, or rotated with group 50 for a linear one; groups 13 and 14 the measured points, 10 the dimension line, 11 the
  number's middle, 1 a blank, since the number is the `TEXT` or `MTEXT` beside it), in a `DIMSTYLE` named for the dimension
  type (one name, written once, with `<>/\":;?*|,=` made `_`; DIMTXT, DIMASZ, DIMEXO, DIMEXE, DIMGAP, DIMDEC in model mm). What
  the dimension draws (dimension line, witness lines, ticks) is held by an anonymous block, `*D1`, `*D2`, ..., which the first
  segment of each dimension names (group 2), as AutoCAD keeps it, so the ink is in the file once, off the `ENTITIES`, and the
  core's reader (which draws a `DIMENSION`'s block) draws it again. Angular, radial, diameter and arc length dimensions,
  baseline and ordinate strings, and a dimension that can't be placed (a view with no scale, a point that isn't finite) are
  not written as entities and keep their strokes on `A-ANNO-DIMS`, as before. A CAD program that redraws a dimension from its
  style (as AutoCAD does when it is edited) draws it by that style, which is ours by sizes, not by every pen.
- **Doors, windows and components** go out as `INSERT`s of blocks (`render/dxf/blocks.rs`), not strokes. The strokes of each
  element (`Drawing::owners`, not its lettering) are put back into the element's own frame: a door or window's is its wall's axis
  at its offset, turned by the wall's tangent; a component's is `at` turned by `angle`. Instances of one family type that draw
  the same (to 0.05 mm) share a `BLOCK` named `family: type` (`_2`, ... for another way of drawing it, a hand or facing flipped
  or a parameter changed; at most 16 a type, 5,000 blocks, 50,000 inserts). Each `INSERT` is at the instance's origin, turned by
  its angle, at scale 1, on the element's layer (`A-DOOR`, `A-GLAZ`, `A-FURN`, ...), with a `MARK` (the element's mark) and a
  `TYPE` (`family: type`) attribute. The block declares both as `ATTDEF`s; the `ATTRIB`s are invisible (flag 1), because the
  plan's own `TEXT` is the lettering and the core's reader draws a visible one as text. The `INSERT` is written where its
  element's first stroke stood, so what the plan draws over it (the grid across a window) still does. An element that cannot be
  placed, or past a cap, keeps its strokes. Known limit: libredwg's `dwg2dxf` reads the DWG's inserts and attributes but prints
  `Invalid ATTRIB.keep_duplicate_records` for each `ATTRIB` acadrust writes in AC1032; acadrust reads them back whole.
- **Sections and elevations** go out through the same writer, in the view's own plane (x along the cut line, y the elevation, in
  mm), by `save` or `render` with a `.dxf` or `.dwg` path and `view` naming one. `render::section_drawn` builds the drawing as
  the PNG does (cut poché on `A-WALL-PATT`, cut edges, grids, level datums lettered as `TEXT` on their element's layer, notes,
  dimensions), and `commands::write_cut` hands it to `write_drawing`. The hidden-line picture of what lies beyond the cut is
  the ground the PNG is painted on, not a primitive, so it never reaches the DXF and is never traced into polylines; the reply's
  `notes` say `1 picture(s) of what lies beyond the cut aren't written to DXF; the cut itself is`. No `IMAGE` entity: it names
  its pixels in an `IMAGEDEF` pointing at a file beside the DXF, which a save would have to write too. A section's `.png`
  still comes from `render`; its `.svg` is not made.
- **The DWG** comes from `crates/dwg`: `bimcraft_dwg::to_dwg` reads the DXF with acadrust's `DxfReader`, sets the document to
  AC1032 and writes it with its `DwgWriter`, under `catch_unwind`, the DXF held to `Limits::dxf_bytes` and the DWG to
  `Limits::dwg_bytes`. It runs in the app's own process: what it reads is the core's DXF, not a stranger's file. A session
  writes a DWG through the `DwgWriter` it is given (`Session::dwg_writer`), as it reads one through its `DwgReader`; the desktop
  and the command line (so MCP too) hand theirs `bimcraft_dwg::writer()` behind their default `dwg` feature, and a session
  without one says "this build can't write DWG; save as .dxf".

Tests: `render::dxf::tests` reads the sample house's plan back with `dxf::read_text` (layers, entities per layer, mm, extents,
dashes, weights); `render::dxf::dims_tests` reads the DIMENSION, DIMSTYLE and block records as a plain group-code walk; `render::dxf::blocks_tests` does the same for the blocks, inserts and attributes; `plan_export_tests.rs` drives `save` with a fake writer; `crates/dwg`'s tests round-trip the plan through
`to_dxf`, and `apps/bimcraft-cli/tests/dwg.rs` saves the sample as DWG from the command line. Outside our own code, libredwg's
`dwg2dxf` reads the DWG back with every entity on its layer, and `import_cad` of it into a new project draws the plan again.
Still to come: DWG 2000 (AC1015).

## The project

A project holds each drawing once, by name, as a `Cad` in `Model::cads` (a library entry of kind `Cad`, so it has an id and a
GUID; `cad.rs`): the bytes of its file, kept as they were and read again when the project opens (a file holds them deflated and in
base64, `CadFile`), the file's name, and, for a linked drawing, its path. `MAX_CADS` drawings and `MAX_HELD` bytes among them
are the most a project holds, and `cad::deserialize_cads` holds a file to them while it is read. `Cad::bulk` is the box round
what the drawing draws with what lies far from the rest left out (more than `FAR` times the spread of the middle of it from the
middle; a stray point at a survey's origin), which centring, zooming and the extents use; `far` counts what it left out.

A `CadImport` element places a drawing: on a `level`, in each of its plans or, with `view`, in that plan only (Revit's Current
view only), its `origin` (in the drawing's units) on `at` (mm), at a `scale` (mm to each of its units) and turned by `angle`
about `at`. It holds the layers it hides everywhere (`hidden`, by name) and how it colours (`colors`: preserve, invert, as on a
dark screen, or black). `cad_place` and `cad_unplace` take a point of the drawing to the model and back. `Model::check` holds it
to a drawing the project has, a level, a view of that level, a scale within `CAD_SCALE`, the names of layers it has, and its
bulk within `MAX_COORD` once placed (a survey's coordinates far off say to place it nearer the origin, with `position`: center).
The engine removes a drawing nothing places after each command, and forgets the layers views hide of it; undo brings both back.

## Commands

- `import_cad` reads a file (`path`) or places one the project holds again (`cad`). Its units are auto (the file's, else mm),
  a unit given, or a `scale`; `position` puts its middle (center, the default) or its origin on `at`; `layers` shows those its
  file has on (visible), all, or a list; `link` remembers the file. It returns what came in: the unit, the counts, the extents,
  the entities far off and what was left out.
- `cad_layers` lists an import's layers with their colours and linetypes and which are shown, and hides or shows them
  everywhere or, with `view`, in that view (its Visibility/Graphics, which `graphics` also sets with `layers`).
- `reload_cad` reads a linked drawing's file again; opening a project does so for each (`cads_not_reloaded` names those whose
  file is gone, which keep what they had).
- `cad_query` names the entity nearest a point: its layer, kind, colour and linetype (Revit's Query).
- `move`, `rotate`, `copy` and `delete` take an import as any element; `mirror` is refused, since a drawing's text would read
  backwards; `modify` changes its level, `at`, `angle`, `scale` and `colors`.

## Drawing and snapping

`render::cad_imports` draws a plan's imports under all the rest but its scans, as Revit draws an import in the background: each
entity of a layer the import and the view show, in its colour (the view's ink when it has none or the import draws in black),
its pen (`Cad::pen`, the entity's or its layer's line weight) and its dashes (`Cad::dash`, its linetype by `$LTSCALE`, scaled to
the view, left solid when a dash would be under a pixel). Text is lettered with `font.rs` at its height, turned and aligned (or, when an MTEXT names a typeface in a `\f` run, laid out in
it with `text::layout`, as a note is); a
solid hatch is filled. A hatch's boundary loops nest as the DXF reference says (`Drawing::fill_nested`): the outermost is
filled, a loop inside it is a hole, a loop inside that is filled again, whichever way the file winds them, so outlined lettering
keeps its counters; the canvas fills such a hatch by the non-zero rule, as it does lettering. What lies outside the view is passed over, so a large site plan zoomed in draws only what's on screen. An
import is one element: picked by any of its lines, selected whole. Sections, elevations and the 3D view leave imports out, as
Revit's do for a drawing placed in a plan; IFC export leaves them out too. The category is `cadimport`, Revit's Imported
Categories: Visibility/Graphics hides it or halftones it in a view like any other.

`snap.rs` gathers a plan's imports' endpoints, midpoints and centres, and their segments for crossings and nearest points, so a
wall drawn from corner to corner of a drawing lies on its lines; a layer hidden isn't snapped to.

## The desktop

Insert > Link CAD (in the Link panel) and Import CAD (a panel of its own) ask for a file, then a box (`crates/ui-egui/src/cad.rs`)
for its name, colours, layers (visible or all), units (auto-detect, or one), positioning (center to center, or origin to origin)
and whether it's for the current view only; it runs `import_cad` on the plan on screen's level, selects what came in, and keeps
the box open with the reason when a file can't come in. The Properties palette shows an import's drawing, file, unit, counts,
level, position, angle and scale, and a drop-down for its colours. Visibility/Graphics has an Imported Categories tab: the
category's row, and under each drawing of the project a check box for each of its layers, cleared for those the view hides. A right click on linked drawings selected offers Reload CAD, which runs `reload_cad` for each.
