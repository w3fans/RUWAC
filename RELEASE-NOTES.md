RUWAC v0.4.77 — reliable binary privacy checks

Release verification previously mistook one compiler-generated branch address
for a short private identifier. The scanner now recognizes an optional duplicate
fallback word only after validating the dispatch, original table entries, exact
fallback target and next independently referenced object boundary.

The policy still rejects actual private text and unknown or malformed tables.
No hostname, artifact hash or file-specific exception is added. Complete-ELF
regressions cover the original failure and genuine adjacent private strings;
negative controls cover changed targets, missing or overlapping references,
unknown instructions, invalid bounds and section permissions. The public
scanner also passes the exact previously published CLI and GUI without a
private workaround; the previous scanner fails the new reproduction.

This is release-tool maintenance. Terminal, clipboard and RDP runtime source is
unchanged. Existing GUI processes, engines and active programs can stay running.
The dated development and acceptance documents now distinguish the delivered
RDP fix from remaining physical input, handover and interoperability work.
Known dependency advisories remain unsuppressed.
