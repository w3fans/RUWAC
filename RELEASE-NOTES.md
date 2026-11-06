RUWAC v0.4.64

Workspace tabs shrink to the available header width. Controls now offers
Two rows of tabs and Compact tabs, saved for main and detached windows.
The new-terminal + stays visible. Full tab titles remain in tooltips;
very crowded rows can scroll horizontally and reveal the selected tab.

The change preserves the existing tab model, terminal processes and connections.
Native isolated tests cover 20 and 40 generated terminals, both row counts and
densities, menus and keyboard navigation, reorder, drag-out, detach/return,
new-terminal creation, close and saved preferences. Physical-owner input
acceptance remains separate.

COPR provides both ruwac and ruwac-desktop for Fedora 43, 44, 45 and Rawhide
on x86_64. For immediate updates, use:

    sudo dnf upgrade --refresh ruwac ruwac-desktop

Reopen the GUI after updating. Existing terminal engines and their programs
can remain running. This is a development patch; the existing RSA advisory
remains open and unsuppressed.
