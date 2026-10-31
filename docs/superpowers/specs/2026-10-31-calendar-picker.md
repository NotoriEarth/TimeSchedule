# Windows-style year and month picker

Follow-up to PR6 at dddcbf6. User supplied a Windows date-picker screenshot and requested more intuitive year/month changing.

Use the existing Rust/Iced popup shared by widget and diary. Clicking the source year opens a four-column year grid: decade heading, previous/next10years, sixteen cells from decade-2 through decade+13, muted neighboring decades and accent-filled selected year. Choosing a year opens a four-column twelve-month grid. Source month opens the month grid immediately. Choosing a month commits that year/month and closes. The month header returns to year selection; arrows change the displayed year. All browsing stays in a draft until a month is chosen; Escape/cancel closes unchanged. Preserve origin-centered placement, calendar isolation and existing editor drafts. Keep exact-date entry as a secondary expandable action.

Use the app dark/light palettes and highlight color with Windows-like flat selection tiles. Native operating-system title bar remains. Default popup360x420; min320x400; grid rows consume available height so expanded date entry fits. Clamp navigation1900..9998 and disable out-of-range year cells. No dependencies, new runtime, storage changes, or animation timer.
