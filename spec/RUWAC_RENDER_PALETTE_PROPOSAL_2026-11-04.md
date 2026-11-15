# Optional render palette — performance experiment (2026-11-04)

The corrected typing benchmark found roughly 155 KB per 40×120 full frame and
about 7 ms for a full render RPC versus 0.16 ms unchanged. Test compact complete
frames before changing poll intervals, transport timing or SSH privacy settings.
This is an unshipped experiment; measure before making a speed claim.

`client.render` gains optional `cell_encoding: "palette-v1"`. Without that field,
responses retain canonical plain cells. The daemon advertises the encoding in
capabilities. The existing archive palette representation is reused for live
snapshots: exact cell tuples in `cell_palette`, row/column indices in
`cell_indices`. Mostly unique screens and views whose combined canonical grids exceed the
palette expansion cell budget retain plain cells. Archived/restored
snapshots stay on their existing path. This is complete-frame compression, not
a delta stream or revision-based reconstruction.

New TUI/desktop shared clients request it and normalize before drawing, selection
or history logic. Preserved engines may ignore the optional field and return
plain frames, which remain accepted. Prove fallback with actual preserved
engines and native clients; unit mocks alone are insufficient. No protocol
version, socket framing, input ordering, canonical dimensions or checkpoint
format changes. Encoding changes invalidate the per-client render projection.

Validate all encoded grids before expansion: exact dimensions, color/style cell
schema, indices, aggregate 262,144-cell bound and 64 MiB expanded cell-byte budget.
Unknown/ambiguous encodings, invalid indices, oversize dimensions or expansion
bombs fail before cell cloning. Plain replies keep their established semantics.

Acceptance: exact decoded equality across RGB/indexed/default colors, extended
styles, combining/wide glyphs, normal/alternate buffers, input modes, split/zoom,
viewer resize, restore and natural process exit. Verify old clients against the
new engine and new clients against preserved engines. Replay the corrected
ASCII/Unicode and nested SSH benchmark with the same observer and worker schedule;
measure both wire and decoded-frame costs. Check CPU/bytes/p50/p95/p99 and record
timeouts. No production rollout until compatibility, resource, native regression
and complete normal release gates pass. No implementation claim for physical
GTK/display latency or the remaining broad product goal.
