# bimcraft-dwg

The DWG bridge, both ways, through the acadrust crate (MPL-2.0, used unmodified as a separate library; NOTICE carries it and
ACadSharp's MIT notice): a DWG drawing as the ASCII DXF `bimcraft-core`'s `dxf.rs` reads (`to_dxf`), and the ASCII DXF of a plan
`render::dxf` writes as a DWG (`to_dwg`), AutoCAD 2018 AC1032 or 2000 AC1015 as `save`'s `version` says. The write-ups are [CAD import](../../docs/areas/cad-import.md#dwg) and
[export](../../docs/areas/cad-import.md#export).

- **Keep acadrust behind the bridge.** Nothing outside this crate sees its types; what comes out is DXF bytes and notes, or
  DWG bytes. The core never depends on this crate: the apps hand a `Session` its `reader()` and its `writer()`, behind their
  default `dwg` feature.
- **Never change acadrust's files.** Its MPL-2.0 copyleft covers them; a fix goes upstream, or round the library here, as
  `Capped` and the size checks do.
- **A file is hostile, and the decoder can't be interrupted.** `to_dxf` checks `Limits` before and while it converts and runs
  under `catch_unwind`, but a file that makes the decoder allocate past memory aborts the process, so the apps read a DWG in a
  child of their own (`in_child`, answered by `child_main`, which every app's `main` calls first).
- **Writing runs in the app's process.** `to_dwg` reads only the core's own DXF, so it needs no child: it runs under
  `catch_unwind`, the DXF held to `Limits::dxf_bytes` and the DWG to `Limits::dwg_bytes`.
- **Autodesk's files are never committed.** Tests make DWG files from DXF text with `dwg_from_dxf` (the `testing` feature, a
  thin wrapper over `to_dwg`); `BIMCRAFT_DWG_SAMPLES` names a folder of real ones, read only where it is set.

## Known limits

- The child has a time limit (300 s) and caps on what it writes, but no memory limit: a hostile drawing can grow it until the
  system's out-of-memory killer picks a victim, which is not always the child.
- `import_cad` waits for the child on the desktop's UI thread, so the window and the control port stand still for as long as the
  read takes, up to the time limit; a project with many linked DWGs reads them one after another when it opens.
- acadrust opens the file with a memory map; a file truncated by another process while it is read can end the child, which the
  parent reports as a failed read.
- The tests round-trip drawings made by acadrust's own writer. Files made by CAD programs are read only when
  `BIMCRAFT_DWG_SAMPLES` names a folder of them, which CI does not set.
- A DWG written here has been read back by acadrust and by libredwg's `dwg2dxf`, not yet opened in AutoCAD, BricsCAD or the
  ODA viewer. libredwg's `dwg2dxf` prints `Invalid ATTRIB.keep_duplicate_records` for every `ATTRIB` acadrust writes (an
  insert's `MARK` and `TYPE`) while keeping them; its own writer's ATTRIBs read clean, so the bytes differ. Nothing the DXF says changes that: a `280` version
  group before the tag, as the reference orders it, left all 40 errors in place (acadrust's DXF reader ignores it), and
  acadrust's `write_attribute_entity` (0.6.6 and 0.6.8 alike) ends at the lock-position bit where libredwg seems to read one more
  byte (an inference from the garbage values, not confirmed), so it would take a change in acadrust. In paper space it costs more
  than a diagnostic: libredwg files those `ATTRIB`s under model space (13 of a sheet's 17 paper-space entities stayed), so a sheet's
  title block letters its words in its block and writes no attribute. Paper space and its `VIEWPORT`s (acadrust's writer takes
  the `67 = 1` entities, owner `1B`, and the viewports' places, sizes, scales and window centres) read back whole from both
  acadrust and libredwg, but no `LAYOUT` object is written and no CAD program has opened one. The same goes for the 2000 (AC1015) files: libredwg reads them as AC1015 with the same two known diagnostics and no
  loss of entities, but no CAD program has opened one. They have no true colour; acadrust writes the nearest ACI.

