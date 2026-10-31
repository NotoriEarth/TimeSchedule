# Diary navigation and parent-centered popups

User-approved follow-up to widget customization: extend year/month clicks to the expanded diary, double-click a diary date to create an event, and center every popup over its originating surface.

Keep Rust/Iced, current storage and unsaved-draft guards. Monthly diary heading opens the existing two-field jump UI with the clicked field selected; it navigates only the originating calendar. Single date click still selects; double click creates a new event even when events already exist. Apply to monthly and weekly diary date areas, without treating event-card controls as date double clicks.

Use the existing diary modal for event creation (already centered within the diary client). Place separate year/month and widget quick-editor windows using live physical client bounds of the originating window. Create them hidden, center their actual outer bounds before showing, and clamp to the origin monitor work area. Avoid mixing Iced logical and Explorer child physical coordinates. Centering changes only our process's windows on their owning UI thread. Preserve draft and close/IME behavior; do not synchronize independent diary and widget month selections.

Implementation choice: reuse one jump component with explicit Widget/Diary target, plus a native popup-position helper. Separate duplicated popups would drift; screen-centered placement does not meet the request. Same clean task branch/PR6 is retained as authorized follow-up. No merge.
